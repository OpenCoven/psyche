//! Strict `psyche.config.v1` loading. Unknown fields are errors; unknown
//! versions are denied before field validation so the error names the real
//! cause.

use std::fmt;
use std::path::{Path, PathBuf};

use psyche_core::schema::{SchemaError, ensure_schema_version};
use serde::Deserialize;

/// A parsed `psyche.config.v1` document.
///
/// No `Eq`: `extensions` is a `toml::Table` whose values include `Float(f64)`,
/// so `toml::Value` derives only `PartialEq`. No derived `Debug` either — see
/// the manual impl below.
#[derive(Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Must be `psyche.config.v1`; any other value is denied.
    pub schema_version: String,
    /// Directory owning local Psyche state.
    pub data_dir: PathBuf,
    /// Coven daemon connection settings.
    pub coven: CovenConfig,
    /// The only place unknown keys are tolerated, and only under an explicitly
    /// versioned table.
    #[serde(default)]
    pub extensions: toml::Table,
}

/// Coven daemon connection settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CovenConfig {
    /// Path to the Coven daemon socket.
    pub socket: PathBuf,
    /// Named daemon contract required before any dependent action.
    pub required_api_version: String,
}

/// Errors from loading configuration.
///
/// `Parse` deliberately does **not** hold a [`toml::de::Error`], and there is no
/// `#[from]` for one. That type's `Display` renders the offending source line
/// verbatim and its `Debug` carries `input: Some(<the entire file>)`, so holding
/// one would leave every secret in the file a single `?err` away from a log. The
/// deserializer error is reduced to a payload-free form at exactly one place —
/// [`reduce_toml_error`] — which is what review should grep for.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file is not valid TOML, or violates the strict schema.
    #[error("configuration is not valid: {detail}")]
    Parse {
        /// Deserializer message only — never the source line, never the file.
        ///
        /// File-free, but not unconditionally value-free: serde's `invalid type`
        /// diagnostic embeds the offending scalar, so a secret placed in a field
        /// of the wrong type would appear here. A field typed
        /// `psyche_core::secret::SecretRef` is unaffected, because its
        /// `try_from` routes failures through a payload-free error instead.
        detail: String,
    },
    /// The declared `schema_version` is not accepted by this build.
    #[error(transparent)]
    Schema(#[from] SchemaError),
    /// The configuration file could not be read.
    #[error("cannot read configuration at {path}: {source}")]
    Read {
        /// Path that could not be read.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
}

// Manual `Debug`, not derived. `extensions` holds arbitrary untyped
// `toml::Value`, so a derived impl would print whatever is in it — including a
// secret placed there by a future extension — on `tracing::debug!(?config)`
// after a *successful* load. `reduce_toml_error` guards the failure path; this
// guards the success path, which is the one more often logged.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("schema_version", &self.schema_version)
            .field("data_dir", &self.data_dir)
            .field("coven", &self.coven)
            .field(
                "extensions",
                &format_args!("<{} key(s) redacted>", self.extensions.len()),
            )
            .finish()
    }
}

/// The single conversion from a deserializer error into a payload-free one.
///
/// `toml::de::Error::message()` is the bare diagnostic without the rendered
/// source line that `Display` adds. Keeping this in one function means a review
/// can grep `reduce_toml_error` to find every place a TOML error crosses the
/// boundary, rather than auditing every `?`.
fn reduce_toml_error(err: &toml::de::Error) -> ConfigError {
    ConfigError::Parse {
        detail: err.message().to_string(),
    }
}

/// Probe used only to read `schema_version`. It intentionally does *not* deny
/// unknown fields, so a future config can be version-checked before its unknown
/// fields are reported.
#[derive(Deserialize)]
struct VersionProbe {
    schema_version: String,
}

/// Parses a configuration document from memory.
///
/// The version is probed and validated *before* the strict parse, so a document
/// declaring a version this build does not accept is reported as an unsupported
/// version rather than as whatever unknown field that version happens to add.
///
/// # Errors
///
/// Returns [`ConfigError::Parse`] if the document is not valid TOML or violates
/// the strict schema, and [`ConfigError::Schema`] if `schema_version` is not the
/// version this build accepts.
pub fn load_str(raw: &str) -> Result<Config, ConfigError> {
    let probe: VersionProbe = toml::from_str(raw).map_err(|e| reduce_toml_error(&e))?;
    ensure_schema_version(&probe.schema_version)?;
    let config: Config = toml::from_str(raw).map_err(|e| reduce_toml_error(&e))?;
    Ok(config)
}

/// Reads and parses a configuration file from disk.
///
/// # Errors
///
/// Returns [`ConfigError::Read`] if the file cannot be read, and otherwise
/// whatever [`load_str`] returns for its contents.
pub fn load_path(path: &Path) -> Result<Config, ConfigError> {
    let raw = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    load_str(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
schema_version = "psyche.config.v1"
data_dir = "/var/lib/psyche"

[coven]
socket = "/run/coven.sock"
required_api_version = "coven.daemon.v1"
"#;

    #[test]
    fn loads_a_valid_config() {
        let cfg = load_str(VALID).unwrap();
        assert_eq!(cfg.schema_version, "psyche.config.v1");
        assert_eq!(cfg.data_dir, PathBuf::from("/var/lib/psyche"));
        assert_eq!(cfg.coven.required_api_version, "coven.daemon.v1");
        assert!(cfg.extensions.is_empty());
    }

    #[test]
    fn rejects_an_unknown_top_level_field() {
        let raw = format!("{VALID}\ntelegram_token = \"nope\"\n");
        let err = load_str(&raw).unwrap_err();
        assert!(
            matches!(err, ConfigError::Parse { .. }),
            "expected a parse error, got {err:?}"
        );
        assert!(err.to_string().contains("telegram_token"));
    }

    #[test]
    fn reports_unknown_version_as_a_version_error_not_a_field_error() {
        // A v2 config will carry fields this build has never seen. The version
        // denial must win, or the operator gets a misleading "unknown field".
        let raw = r#"
schema_version = "psyche.config.v2"
data_dir = "/var/lib/psyche"
brand_new_v2_field = true

[coven]
socket = "/run/coven.sock"
required_api_version = "coven.daemon.v1"
"#;
        let err = load_str(raw).unwrap_err();
        assert!(
            matches!(err, ConfigError::Schema(_)),
            "expected a schema error, got {err:?}"
        );
        assert!(err.to_string().contains("psyche.config.v2"));
    }

    #[test]
    fn accepts_a_versioned_extensions_table() {
        let raw = format!("{VALID}\n[extensions.\"psyche.experiment.v1\"]\nenabled = true\n");
        let cfg = load_str(&raw).unwrap();
        assert!(cfg.extensions.contains_key("psyche.experiment.v1"));
    }

    #[test]
    fn debug_does_not_print_extension_values() {
        let raw = format!(
            "{VALID}\n[extensions.\"psyche.experiment.v1\"]\nlooks_like_a_secret = \"{}\"\n",
            "A".repeat(30)
        );
        let cfg = load_str(&raw).unwrap();
        let rendered = format!("{cfg:?}");
        assert!(!rendered.contains("looks_like_a_secret"), "{rendered}");
        assert!(!rendered.contains(&"A".repeat(30)), "{rendered}");
        assert!(rendered.contains("1 key(s) redacted"), "{rendered}");
    }

    #[test]
    fn missing_file_reports_the_path() {
        let err = load_path(Path::new("/nonexistent/psyche.toml")).unwrap_err();
        assert!(err.to_string().contains("/nonexistent/psyche.toml"));
    }
}
