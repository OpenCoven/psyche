//! A reference to a secret held by an external store. This type never contains
//! the secret itself, and never prints its reference through `Debug`/`Display`.

use std::fmt;

/// A pointer to a secret held by an external store — never the secret itself.
///
/// Deserialising goes through [`TryFrom<String>`], so a literal value cannot
/// enter this type at all, and both `Debug` and `Display` redact, so one cannot
/// leave it through a log line or a panic message either.
#[derive(Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct SecretRef(String);

/// Reasons a value is not usable as a secret reference.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretRefError {
    /// The value looked like a literal secret rather than a reference to one.
    #[error(
        "secret_ref must be a reference URI such as `op://VAULT/ITEM/field`, not a literal value"
    )]
    NotAReference,
}

impl TryFrom<String> for SecretRef {
    type Error = SecretRefError;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        // A reference must name an external store. This is what stops a raw
        // Telegram token being pasted into `secret_ref`.
        if raw.contains("://") && !raw.starts_with("://") {
            Ok(SecretRef(raw))
        } else {
            Err(SecretRefError::NotAReference)
        }
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
        assert_eq!(err, SecretRefError::NotAReference);
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
