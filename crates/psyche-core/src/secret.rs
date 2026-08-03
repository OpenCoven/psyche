//! A reference to a secret held by an external store. This type never contains
//! the secret itself, and never prints its reference through `Debug`/`Display`.

use std::fmt;

/// A pointer to a secret held by an external store — never the secret itself.
///
/// Deserialising goes through [`TryFrom<String>`], which accepts only an
/// allowlisted secret-store scheme — so neither a bare credential nor a URL
/// with one embedded in it can enter. Both `Debug` and `Display` redact, so a
/// reference cannot leave through a log line or a panic message either.
///
/// `Serialize` is deliberately not implemented. A type that redacts in logs but
/// prints plaintext in a config dump is a trap. If a dump ever needs it (e.g. a
/// `config show` command), it must be a hand-written impl rather than a derive,
/// so the choice is visible at the site that makes it.
#[derive(Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct SecretRef(String);

/// Reasons a value is not usable as a secret reference.
///
/// Every variant is deliberately payload-free. The rejection path is exactly
/// where a real secret is most likely to be present, so the rejected value is
/// dropped rather than echoed into an error message or a log line.
///
/// **That guarantee ends at this type.** A deserializer may wrap it in an error
/// that is not payload-free: `toml::de::Error` — the one the configuration
/// contract actually uses — echoes the offending source line through `Display`,
/// and its `Debug` carries the entire input file. Logging one with
/// `tracing::error!(?err)` after a failed config load would emit every secret in
/// that file, not just the rejected value. A config loader must render its own
/// message and must never log a `toml::de::Error` from a file that can contain
/// `secret_ref`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretRefError {
    /// The value did not begin with a supported secret-store scheme.
    ///
    /// This rejects ordinary URLs on purpose. A general "contains `://`" check
    /// would accept `https://host/bot<token>/send` or
    /// `https://user:pass@host/path`, both of which carry the secret *inside*
    /// the URI — the likelier paste, since it is the form API docs show.
    #[error(
        "secret_ref must name a supported secret store (case-sensitive), e.g. `op://VAULT/ITEM/field`"
    )]
    UnsupportedScheme,
    /// A supported scheme with nothing after it, such as a bare `op://`.
    ///
    /// Accepting it would defer the failure to resolution time, far from the
    /// configuration file that caused it.
    #[error("secret_ref has a scheme but no path, e.g. `op://` with no vault/item/field")]
    EmptyPath,
    /// The path is present but not a usable reference: too few segments, an
    /// empty segment, surrounding whitespace, or a control character.
    ///
    /// Whitespace is rejected rather than trimmed. Storing something other than
    /// what the operator wrote is worse than telling them, and control
    /// characters would otherwise reach a resolver's log line as injection —
    /// the same argument `schema` makes for `{found:?}`.
    #[error(
        "secret_ref path must be VAULT/ITEM/FIELD with no empty segments, surrounding whitespace, or control characters"
    )]
    MalformedPath,
    /// The reference exceeds the maximum accepted length.
    #[error("secret_ref is too long")]
    TooLong,
}

/// Schemes naming a supported external secret store.
///
/// `op://` is the only store the configuration contract defines today. Adding
/// one is a line here plus a test — deliberately an allowlist rather than a
/// general URI check, so a value can only ever *point at* a secret.
const SUPPORTED_SCHEMES: [&str; 1] = ["op://"];

/// Minimum `/`-separated segments after the scheme: vault, item, field.
///
/// Path grammar is per-store, so a second scheme means more than another entry
/// in [`SUPPORTED_SCHEMES`] — it needs per-scheme dispatch, e.g.
/// `[(&str, fn(&str) -> bool); N]`. Recorded here so that is a deliberate
/// decision rather than one made under pressure when the second store lands.
const REQUIRED_SEGMENTS: usize = 3;

/// Longest reference accepted. A vault path is short; anything near this is a
/// paste accident. Bounds the blast radius of any future redaction bug.
const MAX_REFERENCE_LEN: usize = 2048;

/// Characters rejected inside a reference path.
///
/// `char::is_control` covers Unicode `Cc` only. The `Cf` format characters below
/// enable visual spoofing — a right-to-left override can make a path render as
/// something other than what it resolves to — so they are rejected explicitly
/// rather than pulling in a Unicode-category dependency for the general case.
fn is_forbidden_in_path(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{200b}'..='\u{200f}'   // zero-width and directional marks
            | '\u{202a}'..='\u{202e}' // bidi embedding and overrides
            | '\u{2066}'..='\u{2069}' // bidi isolates
        )
}

impl TryFrom<String> for SecretRef {
    type Error = SecretRefError;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        let Some(scheme) = SUPPORTED_SCHEMES.iter().find(|s| raw.starts_with(**s)) else {
            return Err(SecretRefError::UnsupportedScheme);
        };
        if raw.len() > MAX_REFERENCE_LEN {
            return Err(SecretRefError::TooLong);
        }
        // `starts_with` guarantees scheme.len() is a char boundary, so this
        // slice cannot panic — worth stating in a crate that denies `unwrap`.
        let path = &raw[scheme.len()..];
        if path.is_empty() {
            return Err(SecretRefError::EmptyPath);
        }
        // Surrounding whitespace only: vault and item names may legitimately
        // contain internal spaces.
        if path.trim() != path || path.chars().any(is_forbidden_in_path) {
            return Err(SecretRefError::MalformedPath);
        }
        let mut segments = 0usize;
        for segment in path.split('/') {
            if segment.is_empty() {
                return Err(SecretRefError::MalformedPath);
            }
            segments += 1;
        }
        if segments < REQUIRED_SEGMENTS {
            return Err(SecretRefError::MalformedPath);
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
    fn error_renderings_never_echo_any_of_the_rejected_value() {
        // Display AND Debug: Debug is what panics and `tracing` `?err` print, so
        // a payload field added to a variant would leak there first. Checks every
        // 8-byte window, not just the whole string, so a truncated echo fails too.
        let secretish = format!("{}:{}", "1234567890", "A".repeat(35));
        let long = format!("op://{}", "B".repeat(4096));
        // Deliberately NOT `op://VAULT/ITEM`: `MalformedPath`'s message states
        // the grammar as the literal text `VAULT/ITEM/FIELD`, so that input
        // collides with a static example rather than proving an echo. The
        // assertion below is unchanged in strength — only the sample differs.
        //
        // Every sample must be >= 8 bytes: windows(8) yields no iterations for
        // shorter input, so a short sample would pass vacuously. "op://" (the
        // EmptyPath case, 5 bytes) is deliberately not in this list.
        for input in [
            secretish,
            long,
            "op://alpha/beta".to_string(),
            "op:// x/y/z".to_string(),
        ] {
            let err = SecretRef::try_from(input.clone()).unwrap_err();
            for rendering in [err.to_string(), format!("{err:?}")] {
                for window in input.as_bytes().windows(8) {
                    let needle = String::from_utf8_lossy(window);
                    assert!(
                        !rendering.contains(needle.as_ref()),
                        "error echoed {needle:?} from input: {rendering}"
                    );
                }
            }
        }
    }

    // Mirrors schema.rs's denies_near_misses. Without it, someone "helpfully"
    // adding .trim() or a case-insensitive scheme match breaks these silently.
    #[test]
    fn rejects_near_misses() {
        for (input, expected) in [
            ("OP://VAULT/ITEM/field", SecretRefError::UnsupportedScheme),
            (" op://VAULT/ITEM/field", SecretRefError::UnsupportedScheme),
            ("op://VAULT/ITEM", SecretRefError::MalformedPath),
            ("op://VAULT", SecretRefError::MalformedPath),
            ("op:///", SecretRefError::MalformedPath),
            ("op://VAULT//field", SecretRefError::MalformedPath),
            ("op:// VAULT/ITEM/field", SecretRefError::MalformedPath),
            ("op://VAULT/ITEM/field ", SecretRefError::MalformedPath),
            ("op://VAULT/ITEM/field\n", SecretRefError::MalformedPath),
            (
                "op://VAULT\u{1b}[31m/ITEM/field",
                SecretRefError::MalformedPath,
            ),
            (
                "op://VAULT/ITEM/fi\u{202e}eld",
                SecretRefError::MalformedPath,
            ),
            (
                "op://VA\u{200b}ULT/ITEM/field",
                SecretRefError::MalformedPath,
            ),
        ] {
            assert_eq!(
                SecretRef::try_from(input.to_string()).unwrap_err(),
                expected,
                "wrong error for {input:?}"
            );
        }
    }

    #[test]
    fn accepts_internal_spaces_in_segment_names() {
        // 1Password vault and item names legitimately contain spaces.
        assert!(SecretRef::try_from("op://My Vault/My Item/token".to_string()).is_ok());
    }

    #[test]
    fn rejects_an_over_long_reference() {
        let long = format!("op://{}", "B".repeat(4096));
        assert_eq!(
            SecretRef::try_from(long).unwrap_err(),
            SecretRefError::TooLong
        );
    }

    // The serde attribute is the load-bearing mechanism and was previously
    // untested: deleting `#[serde(try_from = "String")]` would make SecretRef
    // accept any string, and every other test here would still pass.
    #[test]
    fn deserialising_goes_through_validation() {
        // `unwrap_err` on `Result<Holder, _>` needs `Holder: Debug`. Deriving it
        // is safe precisely because `SecretRef`'s own `Debug` redacts, so the
        // nested rendering is `Holder { secret_ref: SecretRef(<redacted>) }`.
        #[derive(Debug, serde::Deserialize)]
        struct Holder {
            secret_ref: SecretRef,
        }

        let ok: Holder = serde_json::from_str(r#"{"secret_ref":"op://V/I/f"}"#).unwrap();
        assert_eq!(ok.secret_ref.expose_reference(), "op://V/I/f");

        let token_shaped = format!("{}:{}", "1234567890", "A".repeat(35));
        let json = format!(r#"{{"secret_ref":"{token_shaped}"}}"#);
        let err = serde_json::from_str::<Holder>(&json).unwrap_err();
        assert!(
            !err.to_string().contains(&token_shaped),
            "serde echoed input: {err}"
        );
    }

    // psyche-runtime is tokio-based; these cross task boundaries.
    const _: fn() = || {
        fn assert_send_sync_static<T: Send + Sync + 'static>() {}
        assert_send_sync_static::<SecretRef>();
        assert_send_sync_static::<SecretRefError>();
    };

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
