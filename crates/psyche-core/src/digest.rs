//! Canonical JSON bytes and the SHA-256 digest computed over them.
//!
//! Canonicalisation follows RFC 8785 (JSON Canonicalization Scheme), whose
//! defining property is exercised directly in this module's tests: two JSON
//! values that differ only in object key order canonicalize to identical
//! bytes, and so hash identically.
use std::fmt;
use std::fmt::Write as _;

use serde::Serialize;
use serde::ser::{
    self, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant, Serializer,
};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::contracts::{ContractError, MAX_SAFE_INTEGER};

/// Length of the hex-encoded digest after the `sha256:` prefix.
const HEX_DIGEST_LEN: usize = 64;
const MIN_SAFE_INTEGER: i64 = -(MAX_SAFE_INTEGER as i64);
const MAX_SAFE_INTEGER_I128: i128 = MAX_SAFE_INTEGER as i128;
const MIN_SAFE_INTEGER_I128: i128 = -MAX_SAFE_INTEGER_I128;

/// The RFC 8785 canonical JSON bytes for `value`.
///
/// Two values that serialize to the same JSON data but differ in object key
/// order produce identical bytes — canonicalisation, not merely
/// serialization, is the point of this function. Every integer emitted by
/// `Serialize`, including map keys, is validated before the original value is
/// passed to the canonicalizer.
pub fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, ContractError> {
    value
        .serialize(DomainValidator)
        .map_err(validation_failed)?;
    serde_json_canonicalizer::to_vec(value).map_err(canonicalization_failed)
}

fn canonicalization_failed(_error: impl fmt::Display) -> ContractError {
    ContractError::CanonicalizationFailed
}

fn validation_failed(error: ValidationError) -> ContractError {
    match error {
        ValidationError::NonInteroperableNumber => ContractError::NonInteroperableNumber,
        ValidationError::SerializationFailed => ContractError::CanonicalizationFailed,
    }
}

pub(crate) fn validate_json_domain(value: &Value) -> Result<(), ContractError> {
    match value {
        Value::Array(values) => values.iter().try_for_each(validate_json_domain),
        Value::Object(values) => values.values().try_for_each(validate_json_domain),
        Value::Number(number) => {
            let interoperable = if let Some(value) = number.as_i64() {
                value >= MIN_SAFE_INTEGER && value <= MAX_SAFE_INTEGER as i64
            } else if let Some(value) = number.as_u64() {
                value <= MAX_SAFE_INTEGER
            } else if let Some(value) = number.as_f64() {
                value.is_finite()
                    && (value.fract() != 0.0
                        || (value >= MIN_SAFE_INTEGER as f64 && value <= MAX_SAFE_INTEGER as f64))
            } else {
                false
            };
            if interoperable {
                Ok(())
            } else {
                Err(ContractError::NonInteroperableNumber)
            }
        }
        Value::Null | Value::Bool(_) | Value::String(_) => Ok(()),
    }
}

fn validate_float(value: f64) -> Result<(), ContractError> {
    if value.is_finite()
        && (value.fract() != 0.0
            || (value >= MIN_SAFE_INTEGER as f64 && value <= MAX_SAFE_INTEGER as f64))
    {
        Ok(())
    } else {
        Err(ContractError::NonInteroperableNumber)
    }
}

#[derive(Debug, Clone, Copy)]
enum ValidationError {
    NonInteroperableNumber,
    SerializationFailed,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonInteroperableNumber => f.write_str("non-interoperable number"),
            Self::SerializationFailed => f.write_str("serialization failed"),
        }
    }
}

impl std::error::Error for ValidationError {}

impl ser::Error for ValidationError {
    fn custom<T: fmt::Display>(_message: T) -> Self {
        Self::SerializationFailed
    }
}

#[derive(Clone, Copy)]
struct DomainValidator;

fn validate_nested<T: ?Sized + Serialize>(value: &T) -> Result<(), ValidationError> {
    value.serialize(DomainValidator)
}

fn validate_signed(value: i128) -> Result<(), ValidationError> {
    if (MIN_SAFE_INTEGER_I128..=MAX_SAFE_INTEGER_I128).contains(&value) {
        Ok(())
    } else {
        Err(ValidationError::NonInteroperableNumber)
    }
}

fn validate_unsigned(value: u128) -> Result<(), ValidationError> {
    if value <= MAX_SAFE_INTEGER as u128 {
        Ok(())
    } else {
        Err(ValidationError::NonInteroperableNumber)
    }
}

fn validate_serialized_float(value: f64) -> Result<(), ValidationError> {
    validate_float(value).map_err(|_| ValidationError::NonInteroperableNumber)
}

impl Serializer for DomainValidator {
    type Ok = ();
    type Error = ValidationError;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    fn serialize_bool(self, _value: bool) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_i8(self, value: i8) -> Result<Self::Ok, Self::Error> {
        validate_signed(i128::from(value))
    }

    fn serialize_i16(self, value: i16) -> Result<Self::Ok, Self::Error> {
        validate_signed(i128::from(value))
    }

    fn serialize_i32(self, value: i32) -> Result<Self::Ok, Self::Error> {
        validate_signed(i128::from(value))
    }

    fn serialize_i64(self, value: i64) -> Result<Self::Ok, Self::Error> {
        validate_signed(i128::from(value))
    }

    fn serialize_i128(self, value: i128) -> Result<Self::Ok, Self::Error> {
        validate_signed(value)
    }

    fn serialize_u8(self, value: u8) -> Result<Self::Ok, Self::Error> {
        validate_unsigned(u128::from(value))
    }

    fn serialize_u16(self, value: u16) -> Result<Self::Ok, Self::Error> {
        validate_unsigned(u128::from(value))
    }

    fn serialize_u32(self, value: u32) -> Result<Self::Ok, Self::Error> {
        validate_unsigned(u128::from(value))
    }

    fn serialize_u64(self, value: u64) -> Result<Self::Ok, Self::Error> {
        validate_unsigned(u128::from(value))
    }

    fn serialize_u128(self, value: u128) -> Result<Self::Ok, Self::Error> {
        validate_unsigned(value)
    }

    fn serialize_f32(self, value: f32) -> Result<Self::Ok, Self::Error> {
        validate_serialized_float(f64::from(value))
    }

    fn serialize_f64(self, value: f64) -> Result<Self::Ok, Self::Error> {
        validate_serialized_float(value)
    }

    fn serialize_char(self, _value: char) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_str(self, _value: &str) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_bytes(self, _value: &[u8]) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        validate_nested(value)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        validate_nested(value)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        validate_nested(value)
    }

    fn serialize_seq(self, _length: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(self)
    }

    fn serialize_tuple(self, _length: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(self)
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(self)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Ok(self)
    }

    fn serialize_map(self, _length: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(self)
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(self)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Ok(self)
    }

    fn is_human_readable(&self) -> bool {
        true
    }
}

impl SerializeSeq for DomainValidator {
    type Ok = ();
    type Error = ValidationError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        validate_nested(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeTuple for DomainValidator {
    type Ok = ();
    type Error = ValidationError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        validate_nested(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeTupleStruct for DomainValidator {
    type Ok = ();
    type Error = ValidationError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        validate_nested(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeTupleVariant for DomainValidator {
    type Ok = ();
    type Error = ValidationError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        validate_nested(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeMap for DomainValidator {
    type Ok = ();
    type Error = ValidationError;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Self::Error> {
        validate_nested(key)
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        validate_nested(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeStruct for DomainValidator {
    type Ok = ();
    type Error = ValidationError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        _key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        validate_nested(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeStructVariant for DomainValidator {
    type Ok = ();
    type Error = ValidationError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        _key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        validate_nested(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
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
    use std::collections::BTreeMap;

    use crate::contracts::{ContractError, MAX_SAFE_INTEGER};
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
    fn canonicalization_accepts_safe_integer_boundaries_and_fractional_numbers() {
        let value = json!({
            "nested": [
                -9_007_199_254_740_991_i64,
                {"maximum": 9_007_199_254_740_991_u64},
                1.5
            ]
        });

        canonical_bytes(&value).unwrap();
        digest(&value).unwrap();
    }

    #[test]
    fn canonicalization_rejects_unsafe_integers_anywhere_in_the_json_domain() {
        for value in [
            json!({"unsafe": 9_007_199_254_740_992_u64}),
            json!([{"nested": -9_007_199_254_740_992_i64}]),
            json!(u64::MAX),
            json!(9_007_199_254_740_992.0_f64),
        ] {
            assert_eq!(
                canonical_bytes(&value),
                Err(ContractError::NonInteroperableNumber)
            );
        }
    }

    #[test]
    fn adjacent_unsafe_integers_cannot_collapse_to_one_successful_digest() {
        let first = digest(&9_007_199_254_740_992_u64);
        let second = digest(&9_007_199_254_740_993_u64);

        assert!(first.is_err());
        assert!(second.is_err());
    }

    #[test]
    fn canonicalization_rejects_non_finite_numbers_before_they_become_null() {
        #[derive(Serialize)]
        struct NestedFloat {
            values: Vec<f64>,
        }

        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                canonical_bytes(&NestedFloat {
                    values: vec![value],
                }),
                Err(ContractError::NonInteroperableNumber)
            );
        }
    }

    #[test]
    fn canonicalization_validates_i128_and_u128_without_narrowing() {
        macro_rules! assert_canonicalizes {
            ($($value:expr),+ $(,)?) => {
                $(assert!(canonical_bytes(&$value).is_ok());)+
            };
        }

        assert_canonicalizes!(
            i8::MIN,
            i16::MIN,
            i32::MIN,
            -9_007_199_254_740_991_i64,
            -9_007_199_254_740_991_i128,
            9_007_199_254_740_991_i128,
            u8::MAX,
            u16::MAX,
            u32::MAX,
            9_007_199_254_740_991_u64,
            9_007_199_254_740_991_u128,
        );

        for value in [
            canonical_bytes(&-9_007_199_254_740_992_i128),
            canonical_bytes(&9_007_199_254_740_992_i128),
            canonical_bytes(&9_007_199_254_740_992_u128),
            canonical_bytes(&i128::MIN),
            canonical_bytes(&i128::MAX),
            canonical_bytes(&u128::MAX),
        ] {
            assert_eq!(value, Err(ContractError::NonInteroperableNumber));
        }
    }

    #[test]
    fn canonicalization_validates_numeric_map_keys_before_they_can_collapse() {
        #[derive(Serialize)]
        struct NestedMap<'a> {
            values: BTreeMap<u64, &'a str>,
        }

        for key in [9_007_199_254_740_992_u64, 9_007_199_254_740_993_u64] {
            let value = NestedMap {
                values: BTreeMap::from([(key, "unsafe")]),
            };
            assert_eq!(
                canonical_bytes(&value),
                Err(ContractError::NonInteroperableNumber)
            );
        }

        let safe = BTreeMap::from([
            (-9_007_199_254_740_991_i128, "minimum"),
            (9_007_199_254_740_991_i128, "maximum"),
        ]);
        assert_eq!(
            String::from_utf8(canonical_bytes(&safe).unwrap()).unwrap(),
            r#"{"-9007199254740991":"minimum","9007199254740991":"maximum"}"#
        );
    }

    /// Both 9007199254740992 and 9007199254740993 round through f64 to the same
    /// value (9007199254740992.0), so a map containing both would emit duplicate
    /// JSON keys — silently discarding one entry. The validator must reject
    /// either key before canonicalization reaches that point.
    #[test]
    fn map_key_collision_pair_rejected_before_f64_canonicalization() {
        let mut map: BTreeMap<u64, &str> = BTreeMap::new();
        map.insert(MAX_SAFE_INTEGER + 1, "first"); // 9007199254740992
        map.insert(MAX_SAFE_INTEGER + 2, "second"); // 9007199254740993
        assert_eq!(
            canonical_bytes(&map),
            Err(ContractError::NonInteroperableNumber),
            "both collision-prone keys must be rejected before canonicalization"
        );
    }

    /// The exact MAX_SAFE_INTEGER boundary is a valid u64 map key; one past it
    /// must be rejected as NonInteroperableNumber.
    #[test]
    fn map_key_u64_boundary_accepted_and_one_over_rejected() {
        let at_boundary: BTreeMap<u64, &str> = BTreeMap::from([(MAX_SAFE_INTEGER, "boundary")]);
        assert!(
            canonical_bytes(&at_boundary).is_ok(),
            "u64 MAX_SAFE_INTEGER key must be accepted"
        );

        let one_over: BTreeMap<u64, &str> = BTreeMap::from([(MAX_SAFE_INTEGER + 1, "one-over")]);
        assert_eq!(
            canonical_bytes(&one_over),
            Err(ContractError::NonInteroperableNumber),
            "u64 key one past MAX_SAFE_INTEGER must be rejected"
        );
    }

    /// Key validation must recurse into maps nested inside other structures.
    #[test]
    fn nested_map_unsafe_u64_keys_are_rejected() {
        #[derive(Serialize)]
        struct Outer<'a> {
            inner: BTreeMap<u64, &'a str>,
        }

        let unsafe_outer = Outer {
            inner: BTreeMap::from([(MAX_SAFE_INTEGER + 1, "unsafe")]),
        };
        assert_eq!(
            canonical_bytes(&unsafe_outer),
            Err(ContractError::NonInteroperableNumber),
        );

        // A nested map with a safe u64 key must be accepted.
        let safe_outer = Outer {
            inner: BTreeMap::from([(MAX_SAFE_INTEGER, "safe")]),
        };
        assert!(canonical_bytes(&safe_outer).is_ok());
    }

    #[test]
    fn canonicalization_traverses_every_compound_serialize_branch() {
        const UNSAFE: i128 = 9_007_199_254_740_992;

        #[derive(Serialize)]
        struct Struct {
            value: i128,
        }

        #[derive(Serialize)]
        struct Newtype(i128);

        #[derive(Serialize)]
        struct TupleStruct(i128, bool);

        #[derive(Serialize)]
        enum Enum {
            Newtype(i128),
            Tuple(bool, i128),
            Struct { value: i128 },
        }

        let map_value = BTreeMap::from([("value", UNSAFE)]);
        let sequence = vec![UNSAFE];
        let tuple = (false, UNSAFE);

        assert_eq!(
            canonical_bytes(&Struct { value: UNSAFE }),
            Err(ContractError::NonInteroperableNumber)
        );
        assert_eq!(
            canonical_bytes(&Newtype(UNSAFE)),
            Err(ContractError::NonInteroperableNumber)
        );
        assert_eq!(
            canonical_bytes(&TupleStruct(UNSAFE, false)),
            Err(ContractError::NonInteroperableNumber)
        );
        assert_eq!(
            canonical_bytes(&Enum::Newtype(UNSAFE)),
            Err(ContractError::NonInteroperableNumber)
        );
        assert_eq!(
            canonical_bytes(&Enum::Tuple(false, UNSAFE)),
            Err(ContractError::NonInteroperableNumber)
        );
        assert_eq!(
            canonical_bytes(&Enum::Struct { value: UNSAFE }),
            Err(ContractError::NonInteroperableNumber)
        );
        assert_eq!(
            canonical_bytes(&map_value),
            Err(ContractError::NonInteroperableNumber)
        );
        assert_eq!(
            canonical_bytes(&sequence),
            Err(ContractError::NonInteroperableNumber)
        );
        assert_eq!(
            canonical_bytes(&tuple),
            Err(ContractError::NonInteroperableNumber)
        );
        assert_eq!(
            canonical_bytes(&Some(UNSAFE)),
            Err(ContractError::NonInteroperableNumber)
        );
    }

    #[test]
    fn canonicalization_errors_do_not_retain_custom_serializer_messages() {
        struct MaliciousSerialize;

        impl Serialize for MaliciousSerialize {
            fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                Err(serde::ser::Error::custom(format!(
                    "SERIALIZER_SENTINEL_{}",
                    "x".repeat(900_000)
                )))
            }
        }

        let err = canonical_bytes(&MaliciousSerialize).unwrap_err();
        let debug = format!("{err:?}");
        let display = format!("{err}");
        assert!(!debug.contains("SERIALIZER_SENTINEL"));
        assert!(!display.contains("SERIALIZER_SENTINEL"));
        assert!(debug.len() < 256);
        assert!(display.len() < 256);

        struct MaliciousSecondPass(std::cell::Cell<bool>);

        impl Serialize for MaliciousSecondPass {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                if self.0.replace(true) {
                    Err(serde::ser::Error::custom(format!(
                        "SECOND_PASS_SENTINEL_{}",
                        "x".repeat(900_000)
                    )))
                } else {
                    serializer.serialize_unit()
                }
            }
        }

        let err = canonical_bytes(&MaliciousSecondPass(std::cell::Cell::new(false))).unwrap_err();
        let debug = format!("{err:?}");
        let display = format!("{err}");
        assert!(!debug.contains("SECOND_PASS_SENTINEL"));
        assert!(!display.contains("SECOND_PASS_SENTINEL"));
        assert!(debug.len() < 256);
        assert!(display.len() < 256);
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
