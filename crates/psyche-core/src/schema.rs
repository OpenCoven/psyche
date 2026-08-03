//! Versioned schema identifiers. Unknown versions are denied, never coerced.

/// The only configuration schema this build accepts.
pub const CONFIG_SCHEMA_VERSION: &str = "psyche.config.v1";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchemaError {
    #[error("unsupported schema_version `{found}`; this build accepts `{expected}`")]
    UnsupportedVersion {
        expected: &'static str,
        found: String,
    },
}

/// Returns `Ok` only for the exact supported version. No range matching, no
/// coercion — an unknown version is a denial, which is what G2 requires.
pub fn ensure_schema_version(found: &str) -> Result<(), SchemaError> {
    if found == CONFIG_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(SchemaError::UnsupportedVersion {
            expected: CONFIG_SCHEMA_VERSION,
            found: found.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_current_version() {
        assert!(ensure_schema_version(CONFIG_SCHEMA_VERSION).is_ok());
    }

    #[test]
    fn denies_a_future_version() {
        let err = ensure_schema_version("psyche.config.v2").unwrap_err();
        assert_eq!(
            err,
            SchemaError::UnsupportedVersion {
                expected: "psyche.config.v1",
                found: "psyche.config.v2".to_string(),
            }
        );
    }

    #[test]
    fn denies_an_empty_version() {
        assert!(ensure_schema_version("").is_err());
    }
}
