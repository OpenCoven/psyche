//! Canonical JSON bytes and the SHA-256 digest computed over them.
//!
//! Canonicalisation follows RFC 8785 (JSON Canonicalization Scheme), whose
//! defining property is exercised directly in this module's tests: two JSON
//! values that differ only in object key order canonicalize to identical
//! bytes, and so hash identically.
use std::fmt;
use std::fmt::Write as _;

use serde::Serialize;
use sha2::{Digest as _, Sha256};

use crate::contracts::ContractError;

/// Length of the hex-encoded digest after the `sha256:` prefix.
const HEX_DIGEST_LEN: usize = 64;

/// The RFC 8785 canonical JSON bytes for `value`.
///
/// Two values that serialize to the same JSON data but differ in object key
/// order produce identical bytes — canonicalisation, not merely
/// serialization, is the point of this function.
pub fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, ContractError> {
    serde_json_canonicalizer::to_vec(value).map_err(|err| ContractError::CanonicalizationFailed {
        reason: err.to_string(),
    })
}

/// The [`Sha256Digest`] of `value`'s canonical JSON bytes.
pub fn digest<T: Serialize>(value: &T) -> Result<Sha256Digest, ContractError> {
    let bytes = canonical_bytes(value)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(Sha256Digest(format!(
        "{}{}",
        Sha256Digest::PREFIX,
        to_lower_hex(hasher.finalize().as_slice())
    )))
}

/// Encodes `bytes` as lowercase hex, two characters per byte.
fn to_lower_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // `write!` to a `String` cannot fail; the result is discarded rather
        // than unwrapped, since this crate denies `unwrap`/`expect`.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// A validated `sha256:` digest: the fixed prefix followed by exactly 64
/// lowercase hex characters.
#[derive(Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sha256Digest(String);

impl Sha256Digest {
    /// The fixed prefix every digest begins with.
    pub const PREFIX: &'static str = "sha256:";

    /// Validates `value` as `sha256:` followed by exactly 64 lowercase hex
    /// characters, with no trailing data and no uppercase hex digits.
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        let Some(hex) = value.strip_prefix(Self::PREFIX) else {
            return Err(ContractError::UnsupportedDigestPrefix);
        };
        if hex.len() != HEX_DIGEST_LEN
            || !hex
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(ContractError::MalformedDigest);
        }
        Ok(Sha256Digest(value.to_string()))
    }

    /// The full digest string, e.g. `"sha256:<64 lowercase hex chars>"`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Sha256Digest").field(&self.0).finish()
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for Sha256Digest {
    type Error = ContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<Sha256Digest> for String {
    fn from(value: Sha256Digest) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use crate::digest::{Sha256Digest, canonical_bytes, digest};
    use proptest::prelude::*;
    use serde::Serialize;
    use serde_json::json;

    #[test]
    fn canonical_digest_ignores_key_order() {
        let a = json!({"b": 1, "a": 2});
        let b = json!({"a": 2, "b": 1});
        assert_eq!(digest(&a).unwrap(), digest(&b).unwrap());
        assert_eq!(canonical_bytes(&a).unwrap(), canonical_bytes(&b).unwrap());
    }

    #[test]
    fn digest_has_the_sha256_prefix_and_64_lowercase_hex_chars() {
        let d = digest(&json!({"x": 1})).unwrap();
        let s = d.as_str();
        assert!(s.starts_with("sha256:"));
        let hex = &s["sha256:".len()..];
        assert_eq!(hex.len(), 64);
        assert!(
            hex.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }

    #[test]
    fn sha256_digest_round_trips_through_serde() {
        let d = digest(&json!({"a": 1})).unwrap();
        let json_str = serde_json::to_string(&d).unwrap();
        let back: Sha256Digest = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn sha256_digest_rejects_malformed_values() {
        let good = digest(&json!({"a": 1})).unwrap().as_str().to_string();
        for bad in [
            good.replacen("sha256:", "sha255:", 1),
            good[..good.len() - 1].to_string(), // too short
            format!("{good}0"),                 // too long / trailing
            good.to_uppercase(),                // uppercase hex
            good.replace('a', "g"),             // non-hex char (if any 'a' present)
        ] {
            assert!(
                Sha256Digest::parse(&bad).is_err(),
                "expected rejection for {bad:?}"
            );
        }
    }

    #[derive(Serialize)]
    struct Wrapper {
        value: i64,
        label: String,
    }

    proptest! {
        #[test]
        fn any_value_change_changes_the_digest(a in -1000i64..1000, b in -1000i64..1000, label in "[a-z]{1,8}") {
            prop_assume!(a != b);
            let d1 = digest(&Wrapper { value: a, label: label.clone() }).unwrap();
            let d2 = digest(&Wrapper { value: b, label }).unwrap();
            prop_assert_ne!(d1, d2);
        }
    }
}
