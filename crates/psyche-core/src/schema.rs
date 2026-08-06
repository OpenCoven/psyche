//! Versioned schema identifiers. Unknown versions are denied, never coerced.

/// The only configuration schema this build accepts.
pub const CONFIG_SCHEMA_VERSION: &str = "psyche.config.v1";

/// Reasons a declared schema version is not usable by this build.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchemaError {
    // {found:?} not {found}: a hand-edited `schema_version = " psyche.config.v1"`
    // would otherwise log as visually identical to the accepted value, and the
    // value is untrusted text going into a log line — newlines and ANSI escapes
    // are log injection. `expected` is not a field: it is always this const, and
    // a public field would let callers construct a state that cannot exist.
    /// The configuration declared a version this build does not accept. Denial
    /// is unconditional: there is no compatibility range and no coercion.
    #[error("unsupported schema_version {found:?}; this build accepts {CONFIG_SCHEMA_VERSION:?}")]
    UnsupportedVersion {
        /// The rejected value, byte-for-byte as it appeared in the configuration.
        found: String,
    },
}

/// Returns `Ok` only for the exact supported version. No range matching, no
/// coercion — an unknown version is a denial, which is what G2 requires.
pub fn ensure_schema_version(declared: &str) -> Result<(), SchemaError> {
    if declared == CONFIG_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(SchemaError::UnsupportedVersion {
            found: declared.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The literal, not the const: this string is the on-disk contract with every
    // user's config file, so a typo in the const must fail a test rather than
    // silently redefine the format.
    #[test]
    fn the_accepted_version_string_is_stable() {
        assert_eq!(CONFIG_SCHEMA_VERSION, "psyche.config.v1");
        assert!(ensure_schema_version("psyche.config.v1").is_ok());
    }

    #[test]
    fn denies_a_future_version() {
        let err = ensure_schema_version("psyche.config.v2").unwrap_err();
        // matches!, not a full struct literal: pins what an operator can observe
        // without coupling the test to the variant's field list.
        assert!(
            matches!(err, SchemaError::UnsupportedVersion { ref found } if found == "psyche.config.v2")
        );
    }

    #[test]
    fn the_error_names_both_versions() {
        // The #[error] format string is the operator-facing contract; without
        // this it could be mangled to anything and every other test would pass.
        let rendered = ensure_schema_version("psyche.config.v2")
            .unwrap_err()
            .to_string();
        assert!(rendered.contains("psyche.config.v2"), "{rendered}");
        assert!(rendered.contains("psyche.config.v1"), "{rendered}");
    }

    // These pin the deliberate strictness. Without them, someone "helpfully"
    // adding .trim() or eq_ignore_ascii_case would break G2 denial silently.
    #[test]
    fn denies_near_misses() {
        for near in [
            "",
            " psyche.config.v1",
            "psyche.config.v1 ",
            "psyche.config.v1\n",
            "PSYCHE.CONFIG.V1",
            "psyche.config.v10",
            "psyche.config.v1.1",
        ] {
            assert!(
                ensure_schema_version(near).is_err(),
                "expected denial for {near:?}"
            );
        }
    }

    // psyche-runtime is tokio-based, so this error crosses task boundaries and
    // lands in Box<dyn Error + Send + Sync>. Fails at compile time if that breaks.
    const _: fn() = || {
        fn assert_send_sync_static<T: Send + Sync + 'static>() {}
        assert_send_sync_static::<SchemaError>();
    };
}
