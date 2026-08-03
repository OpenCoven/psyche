//! A reference to a secret held by an external store. This type never contains
//! the secret itself, and never prints its reference through `Debug`/`Display`.

use std::fmt;

/// A pointer to a secret held by an external store — never the secret itself.
///
/// Deserialising goes through [`TryFrom<String>`], which accepts only an
/// allowlisted secret-store scheme — so neither a bare credential nor a URL
/// with one embedded in it can enter. Both `Debug` and `Display` redact, so a
/// reference cannot leave through a log line or a panic message either.
#[derive(Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct SecretRef(String);

/// Reasons a value is not usable as a secret reference.
///
/// Every variant is deliberately payload-free. The rejection path is exactly
/// where a real secret is most likely to be present, so the rejected value is
/// dropped rather than echoed into an error message or a log line.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretRefError {
    /// The value did not begin with a supported secret-store scheme.
    ///
    /// This rejects ordinary URLs on purpose. A general "contains `://`" check
    /// would accept `https://host/bot<token>/send` or
    /// `https://user:pass@host/path`, both of which carry the secret *inside*
    /// the URI — the likelier paste, since it is the form API docs show.
    #[error("secret_ref must name a supported secret store, e.g. `op://VAULT/ITEM/field`")]
    UnsupportedScheme,
    /// A supported scheme with nothing after it, such as a bare `op://`.
    ///
    /// Accepting it would defer the failure to resolution time, far from the
    /// configuration file that caused it.
    #[error("secret_ref has a scheme but no path, e.g. `op://` with no vault/item/field")]
    EmptyPath,
}

/// Schemes naming a supported external secret store.
///
/// `op://` is the only store the configuration contract defines today. Adding
/// one is a line here plus a test — deliberately an allowlist rather than a
/// general URI check, so a value can only ever *point at* a secret.
const SUPPORTED_SCHEMES: [&str; 1] = ["op://"];

impl TryFrom<String> for SecretRef {
    type Error = SecretRefError;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        let Some(scheme) = SUPPORTED_SCHEMES.iter().find(|s| raw.starts_with(**s)) else {
            return Err(SecretRefError::UnsupportedScheme);
        };
        if raw.len() == scheme.len() {
            return Err(SecretRefError::EmptyPath);
        }
        Ok(SecretRef(raw))
    }
}

impl fmt::Debug for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretRef(<redacted>)")
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl SecretRef {
    /// The only way to read the reference text. Deliberately verbose so that
    /// `rg expose_reference` finds every call site during review.
    pub fn expose_reference(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_reference_uri() {
        let r = SecretRef::try_from("op://VAULT/ITEM/token".to_string()).unwrap();
        assert_eq!(r.expose_reference(), "op://VAULT/ITEM/token");
    }

    #[test]
    fn rejects_a_literal_token() {
        // Built at runtime rather than written as a literal: a bot-token-shaped
        // string committed to the repo trips secret scanners and trains
        // reviewers to wave through that shape.
        let token_shaped = format!("{}:{}", "1234567890", "A".repeat(35));
        let err = SecretRef::try_from(token_shaped).unwrap_err();
        assert_eq!(err, SecretRefError::UnsupportedScheme);
    }

    // The cases a "contains ://" check would have accepted. These are the point
    // of the allowlist: a secret carried *inside* a URI is the likelier paste.
    #[test]
    fn rejects_a_url_with_the_secret_in_its_path() {
        let url = format!("https://api.example.com/bot{}/send", "A".repeat(35));
        assert_eq!(
            SecretRef::try_from(url).unwrap_err(),
            SecretRefError::UnsupportedScheme
        );
    }

    #[test]
    fn rejects_a_url_carrying_inline_credentials() {
        let url = format!("https://user:{}@example.com/path", "A".repeat(20));
        assert_eq!(
            SecretRef::try_from(url).unwrap_err(),
            SecretRefError::UnsupportedScheme
        );
    }

    #[test]
    fn rejects_an_unallowlisted_scheme() {
        for other in ["file:///etc/shadow", "http://example.com/x", "x://y"] {
            assert_eq!(
                SecretRef::try_from(other.to_string()).unwrap_err(),
                SecretRefError::UnsupportedScheme,
                "expected rejection for {other:?}"
            );
        }
    }

    #[test]
    fn rejects_a_scheme_with_no_path() {
        assert_eq!(
            SecretRef::try_from("op://".to_string()).unwrap_err(),
            SecretRefError::EmptyPath
        );
    }

    #[test]
    fn error_messages_never_echo_the_rejected_value() {
        // The rejection path is where a real secret is most likely present.
        let secretish = format!("{}:{}", "1234567890", "A".repeat(35));
        let rendered = SecretRef::try_from(secretish.clone())
            .unwrap_err()
            .to_string();
        assert!(
            !rendered.contains(&secretish),
            "error echoed the input: {rendered}"
        );
    }

    #[test]
    fn debug_never_reveals_the_reference() {
        let r = SecretRef::try_from("op://VAULT/ITEM/token".to_string()).unwrap();
        let rendered = format!("{r:?}");
        assert_eq!(rendered, "SecretRef(<redacted>)");
        assert!(!rendered.contains("VAULT"));
    }

    #[test]
    fn display_never_reveals_the_reference() {
        let r = SecretRef::try_from("op://VAULT/ITEM/token".to_string()).unwrap();
        let rendered = format!("{r}");
        assert_eq!(rendered, "<redacted>");
        assert!(!rendered.contains("VAULT"));
    }
}
