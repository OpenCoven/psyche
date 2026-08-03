//! Strict `psyche.config.v1` loading. Unknown fields are errors; unknown
//! versions are denied before field validation so the error names the real
//! cause.

use std::fmt;
use std::path::{Path, PathBuf};

use psyche_core::schema::{SchemaError, ensure_schema_version};
use serde::Deserialize;

/// Largest configuration file this build will read, in bytes.
///
/// A daemon that reloads on SIGHUP reads whatever path it was pointed at; a
/// symlink to a huge file would otherwise be an out-of-memory switch.
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// Wire representation. Private on purpose: it is the only thing that derives
/// `Deserialize`, so the sole route to a `Config` is `load_str`/`load_path`,
/// which run `ensure_schema_version`. A public `Deserialize` on `Config` would
/// let a nested derive in a consumer crate produce an unvalidated `Config` with
/// no compile error and no failing test.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigRepr {
    schema_version: String,
    data_dir: PathBuf,
    coven: CovenConfig,
    #[serde(default)]
    extensions: toml::Table,
}

/// A validated `psyche.config.v1` document.
///
/// Obtainable only through [`load_str`] or [`load_path`], both of which deny an
/// unknown `schema_version` before any field is used.
///
/// No `Eq`: [`Extensions`] wraps a `toml::Table` whose values include
/// `Float(f64)`, so only `PartialEq` is available.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Directory owning local Psyche state.
    pub data_dir: PathBuf,
    /// Coven daemon connection settings.
    pub coven: CovenConfig,
    /// Forward-compatible extension tables, keyed by versioned identifier.
    pub extensions: Extensions,
}

impl Config {
    /// The schema version this build accepts. Always
    /// `psyche_core::schema::CONFIG_SCHEMA_VERSION`; a validated `Config`
    /// cannot hold any other value.
    #[must_use]
    pub fn schema_version(&self) -> &'static str {
        psyche_core::schema::CONFIG_SCHEMA_VERSION
    }
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

/// Extension tables from the configuration, keyed by versioned identifier.
///
/// Values are untyped, so `Debug` redacts them: a future extension may hold a
/// secret, and `tracing::debug!(?config)` after a *successful* load would
/// otherwise print it. Read values with [`Extensions::get`].
///
/// Wrapping the table also keeps `toml` out of this crate's public API, so
/// consumers are not pinned to its major version.
#[derive(Clone, PartialEq, Default)]
pub struct Extensions(toml::Table);

// Redacting Debug, hand-written for the reason in the type doc.
impl fmt::Debug for Extensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{} key(s) redacted>", self.0.len())
    }
}

impl Extensions {
    /// Number of extension tables present.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether any extension table is present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether a given versioned key is present.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    /// Deserialise one extension table into a caller-owned type.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Parse`] if the table does not match `T`.
    pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>, ConfigError> {
        match self.0.get(key) {
            None => Ok(None),
            Some(value) => T::deserialize(value.clone())
                .map(Some)
                .map_err(|e: toml::de::Error| ConfigError::Parse {
                    detail: detail_from(&e),
                    path: None,
                }),
        }
    }
}

/// Errors from loading configuration.
///
/// `Parse` deliberately does **not** hold a [`toml::de::Error`], and there is no
/// `#[from]` for one. That type's `Display` renders the offending source line
/// verbatim and its `Debug` carries `input: Some(<the entire file>)`, so holding
/// one would leave every secret in the file a single `?err` away from a log. The
/// deserializer error is reduced to a payload-free form at exactly one place —
/// [`detail_from`] — which is what review should grep for. [`reduce_toml_error`]
/// wraps it for the file-loading path only, so it is not the exhaustive one.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file is not valid TOML, or violates the strict schema.
    #[error("configuration is not valid{}: {detail}", path.as_ref().map(|p| format!(" at {}", p.display())).unwrap_or_default())]
    #[non_exhaustive]
    Parse {
        /// Deserializer message, prefixed with line and column when known.
        ///
        /// File-free, but not unconditionally value-free: serde's `invalid type`
        /// diagnostic embeds the offending scalar, so a secret placed in a field
        /// of the wrong type would appear here. A field typed
        /// `psyche_core::secret::SecretRef` is unaffected, because its
        /// `try_from` routes failures through a payload-free error instead.
        detail: String,
        /// File the failure came from, when loaded from one.
        path: Option<PathBuf>,
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
    /// The configuration file is larger than [`MAX_CONFIG_BYTES`].
    #[error("configuration at {path} is {bytes} bytes, over the {MAX_CONFIG_BYTES} byte limit")]
    TooLarge {
        /// Path that was too large to read.
        path: PathBuf,
        /// Size reported by the filesystem.
        bytes: u64,
    },
}

/// The single place a deserializer error is reduced to payload-free text.
///
/// `toml::de::Error::message()` is the bare diagnostic; its `Display` would
/// render the offending source line and its `Debug` carries the whole input.
/// Every crossing of that boundary goes through this function, so review greps
/// one name rather than auditing each `?`.
fn detail_from(err: &toml::de::Error) -> String {
    err.message().to_string()
}

/// Reduces a deserializer error from the file-loading path, adding position and
/// originating path to [`detail_from`]'s payload-free text.
///
/// Line and column are derived from `span()`, which is a byte offset and
/// therefore carries no file content.
fn reduce_toml_error(raw: &str, err: &toml::de::Error, path: Option<&Path>) -> ConfigError {
    let at = err
        .span()
        .map(|s| {
            let before = &raw[..s.start.min(raw.len())];
            let line = before.matches('\n').count() + 1;
            // Column is a byte offset within the line, not a character offset,
            // so it can skew on lines containing multibyte text. Content-free
            // either way.
            let column = before.len() - before.rfind('\n').map_or(0, |i| i + 1) + 1;
            format!("line {line}, column {column}: ")
        })
        .unwrap_or_default();
    ConfigError::Parse {
        detail: format!("{at}{}", detail_from(err)),
        path: path.map(Path::to_path_buf),
    }
}

/// Whether a key looks like `<namespace>.<name>.v<N>`.
fn is_versioned_key(key: &str) -> bool {
    let mut parts = key.rsplit('.');
    let Some(version) = parts.next() else {
        return false;
    };
    let Some(rest) = version.strip_prefix('v') else {
        return false;
    };
    !rest.is_empty()
        && rest.bytes().all(|b| b.is_ascii_digit())
        && parts.next().is_some_and(|s| !s.is_empty())
}

/// Probe used only to read `schema_version`. It intentionally does *not* deny
/// unknown fields, so a future config can be version-checked before its unknown
/// fields are reported.
///
/// The document is parsed twice on purpose. Parsing once into a `toml::Value`
/// and deserialising both shapes from that would lose span information, which
/// is what [`reduce_toml_error`] reports line and column from.
#[derive(Deserialize)]
struct VersionProbe {
    schema_version: String,
}

fn load_inner(raw: &str, path: Option<&Path>) -> Result<Config, ConfigError> {
    let probe: VersionProbe = toml::from_str(raw).map_err(|e| reduce_toml_error(raw, &e, path))?;
    ensure_schema_version(&probe.schema_version)?;

    let repr: ConfigRepr = toml::from_str(raw).map_err(|e| reduce_toml_error(raw, &e, path))?;
    // Belt and braces: the strict parse is a second, independent read of the
    // document, so its version is re-checked rather than assumed to match the
    // probe's. This is also what keeps the field live rather than dead code.
    ensure_schema_version(&repr.schema_version)?;

    for key in repr.extensions.keys() {
        if !is_versioned_key(key) {
            // The key is operator-authored structure, not a value, so naming it
            // leaks nothing. The extension's *value* is deliberately absent.
            return Err(ConfigError::Parse {
                detail: format!(
                    "extension key {key:?} must be a versioned identifier such as `psyche.experiment.v1`"
                ),
                path: path.map(Path::to_path_buf),
            });
        }
    }

    Ok(Config {
        data_dir: repr.data_dir,
        coven: repr.coven,
        extensions: Extensions(repr.extensions),
    })
}

/// Parses a configuration document from memory.
///
/// The version is probed and validated *before* the strict parse, so a document
/// declaring a version this build does not accept is reported as an unsupported
/// version rather than as whatever unknown field that version happens to add.
///
/// # Errors
///
/// Returns [`ConfigError::Parse`] if the document is not valid TOML, violates
/// the strict schema, or carries an unversioned extension key, and
/// [`ConfigError::Schema`] if `schema_version` is not the version this build
/// accepts.
pub fn load_str(raw: &str) -> Result<Config, ConfigError> {
    load_inner(raw, None)
}

/// Reads and parses a configuration file from disk.
///
/// # Errors
///
/// Returns [`ConfigError::Read`] if the file cannot be read,
/// [`ConfigError::TooLarge`] if it exceeds [`MAX_CONFIG_BYTES`], and otherwise
/// whatever [`load_str`] returns for its contents.
pub fn load_path(path: &Path) -> Result<Config, ConfigError> {
    // Bounded read: a daemon that reloads on SIGHUP should not OOM on a
    // symlink to a huge file.
    let meta = std::fs::metadata(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    if meta.len() > MAX_CONFIG_BYTES {
        return Err(ConfigError::TooLarge {
            path: path.to_path_buf(),
            bytes: meta.len(),
        });
    }
    let raw = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    load_inner(&raw, Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    const HEAD: &str = "schema_version = \"psyche.config.v1\"\ndata_dir = \"/var/lib/psyche\"\n";
    const COVEN: &str =
        "\n[coven]\nsocket = \"/run/coven.sock\"\nrequired_api_version = \"coven.daemon.v1\"\n";

    fn valid() -> String {
        format!("{HEAD}{COVEN}")
    }

    fn temp_with(contents: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn loads_a_valid_config() {
        let cfg = load_str(&valid()).unwrap();
        assert_eq!(cfg.schema_version(), "psyche.config.v1");
        assert_eq!(cfg.data_dir, PathBuf::from("/var/lib/psyche"));
        assert_eq!(cfg.coven.required_api_version, "coven.daemon.v1");
        assert!(cfg.extensions.is_empty());
    }

    // Injected between HEAD and COVEN, at document scope. The previous version
    // of this test appended after COVEN, so the key landed *inside* `[coven]`
    // and it was exercising `CovenConfig`'s `deny_unknown_fields`, never
    // `ConfigRepr`'s. Asserting on the expected-field list makes that
    // impossible to reintroduce silently.
    #[test]
    fn rejects_an_unknown_top_level_field() {
        let raw = format!("{HEAD}telegram_token = \"nope\"\n{COVEN}");
        let err = load_str(&raw).unwrap_err();
        assert!(
            matches!(err, ConfigError::Parse { .. }),
            "expected a parse error, got {err:?}"
        );
        let rendered = err.to_string();
        assert!(rendered.contains("telegram_token"), "{rendered}");
        assert!(
            rendered.contains("expected one of `schema_version`"),
            "{rendered}"
        );
    }

    #[test]
    fn rejects_an_unknown_field_in_coven() {
        let raw = format!("{}\nstray = true\n", valid());
        let err = load_str(&raw).unwrap_err();
        assert!(
            matches!(err, ConfigError::Parse { .. }),
            "expected a parse error, got {err:?}"
        );
        let rendered = err.to_string();
        assert!(rendered.contains("stray"), "{rendered}");
        assert!(rendered.contains("expected `socket`"), "{rendered}");
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
        let raw = format!(
            "{}\n[extensions.\"psyche.experiment.v1\"]\nenabled = true\n",
            valid()
        );
        let cfg = load_str(&raw).unwrap();
        assert!(cfg.extensions.contains_key("psyche.experiment.v1"));
        assert_eq!(cfg.extensions.len(), 1);
    }

    #[test]
    fn rejects_an_unversioned_extension_key() {
        let raw = format!("{}\n[extensions.not_versioned]\nenabled = true\n", valid());
        let err = load_str(&raw).unwrap_err();
        let rendered = err.to_string();
        assert!(rendered.contains("not_versioned"), "{rendered}");
        assert!(rendered.contains("versioned identifier"), "{rendered}");
    }

    #[test]
    fn debug_does_not_print_extension_values() {
        let raw = format!(
            "{}\n[extensions.\"psyche.experiment.v1\"]\nlooks_like_a_secret = \"{}\"\n[extensions.\"psyche.other.v2\"]\nalso = 1\n",
            valid(),
            "A".repeat(30)
        );
        let cfg = load_str(&raw).unwrap();
        let rendered = format!("{cfg:?}");
        assert!(!rendered.contains("looks_like_a_secret"), "{rendered}");
        assert!(!rendered.contains(&"A".repeat(30)), "{rendered}");
        assert!(rendered.contains("2 key"), "{rendered}");
    }

    #[test]
    fn extensions_get_deserialises_into_a_caller_type() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct Experiment {
            enabled: bool,
            retries: u8,
        }

        let raw = format!(
            "{}\n[extensions.\"psyche.experiment.v1\"]\nenabled = true\nretries = 3\n",
            valid()
        );
        let cfg = load_str(&raw).unwrap();

        let got: Option<Experiment> = cfg.extensions.get("psyche.experiment.v1").unwrap();
        assert_eq!(
            got,
            Some(Experiment {
                enabled: true,
                retries: 3
            })
        );

        let missing: Option<Experiment> = cfg.extensions.get("psyche.absent.v1").unwrap();
        assert_eq!(missing, None);

        // A shape mismatch is a Parse error, not a panic.
        let bad = cfg
            .extensions
            .get::<u8>("psyche.experiment.v1")
            .unwrap_err();
        assert!(matches!(bad, ConfigError::Parse { .. }), "{bad:?}");
    }

    #[test]
    fn missing_file_reports_the_path() {
        let err = load_path(Path::new("/nonexistent/psyche.toml")).unwrap_err();
        assert!(err.to_string().contains("/nonexistent/psyche.toml"));
    }

    #[test]
    fn load_path_reads_a_real_file() {
        let f = temp_with(&valid());
        let cfg = load_path(f.path()).unwrap();
        assert_eq!(cfg.data_dir, PathBuf::from("/var/lib/psyche"));
        assert_eq!(cfg.coven.socket, PathBuf::from("/run/coven.sock"));
        assert_eq!(cfg.schema_version(), "psyche.config.v1");
    }

    #[test]
    fn parse_errors_report_line_and_column() {
        // Duplicate top-level key: the bare message is "duplicate key", which
        // without a position is unactionable in a long config.
        let raw = format!("{HEAD}data_dir = \"/other\"\n{COVEN}");
        let err = load_str(&raw).unwrap_err();
        let rendered = err.to_string();
        assert!(rendered.contains("line "), "{rendered}");
        assert!(rendered.contains("column "), "{rendered}");
    }

    #[test]
    fn parse_errors_from_a_file_name_the_path() {
        let f = temp_with("this is not toml = = =\n");
        let err = load_path(f.path()).unwrap_err();
        let rendered = err.to_string();
        let path = f.path().display().to_string();
        assert!(rendered.contains(&path), "{rendered} / {path}");
    }

    #[test]
    fn accepts_a_byte_order_mark() {
        let raw = format!("\u{feff}{}", valid());
        assert!(load_str(&raw).is_ok(), "{:?}", load_str(&raw).unwrap_err());
    }

    #[test]
    fn accepts_crlf_line_endings() {
        let raw = valid().replace('\n', "\r\n");
        assert!(load_str(&raw).is_ok(), "{:?}", load_str(&raw).unwrap_err());
    }

    #[test]
    fn an_empty_file_names_the_missing_version_field() {
        let err = load_str("").unwrap_err();
        let rendered = err.to_string();
        assert!(
            rendered.contains("missing field `schema_version`"),
            "{rendered}"
        );
    }

    #[test]
    fn nested_extension_tables_count_only_top_level_keys() {
        let raw = format!(
            "{}\n[extensions.\"psyche.experiment.v1\".deeply.nested]\nvalue = 1\n",
            valid()
        );
        let cfg = load_str(&raw).unwrap();
        assert_eq!(cfg.extensions.len(), 1);
        assert!(cfg.extensions.contains_key("psyche.experiment.v1"));
    }

    #[test]
    fn versioned_key_rule_is_what_it_claims() {
        for good in [
            "psyche.experiment.v1",
            "psyche.experiment.v10",
            "a.v0",
            "one.two.three.v2",
        ] {
            assert!(is_versioned_key(good), "expected accept for {good:?}");
        }
        for bad in [
            "",
            "not_versioned",
            "v1",
            "psyche.experiment.v",
            "psyche.experiment.V1",
            "psyche.experiment.v1x",
            ".v1",
            "psyche.experiment.1",
        ] {
            assert!(!is_versioned_key(bad), "expected reject for {bad:?}");
        }
    }

    // psyche-runtime is tokio-based, so these cross task boundaries and land in
    // Box<dyn Error + Send + Sync>. Fails at compile time if that breaks.
    const _: fn() = || {
        fn assert_send_sync_static<T: Send + Sync + 'static>() {}
        assert_send_sync_static::<ConfigError>();
        assert_send_sync_static::<Config>();
    };
}
