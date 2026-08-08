//! Canonical JSON bytes and the SHA-256 digest computed over them.
//!
//! Canonicalisation follows RFC 8785 (JSON Canonicalization Scheme), whose
//! defining property is exercised directly in this module's tests: two JSON
//! values that differ only in object key order canonicalize to identical
//! bytes, and so hash identically.
use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Write as _;

use serde::Serialize;
use serde::ser::{
    self, Impossible, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
    SerializeTuple, SerializeTupleStruct, SerializeTupleVariant, Serializer,
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
/// serialization, is the point of this function. The original value is
/// serialized exactly once into a validated representation, including map
/// keys, before that representation is passed to the canonicalizer.
pub fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, ContractError> {
    let collected = value.serialize(ValueCollector).map_err(validation_failed)?;
    let canonicalizer_input = collected.into_json().map_err(validation_failed)?;
    serde_json_canonicalizer::to_vec(&canonicalizer_input).map_err(canonicalization_failed)
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
struct ValueCollector;

enum CollectedValue {
    Null,
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    Float(f64),
    String(String),
    Array(Vec<Self>),
    Object(BTreeMap<String, Self>),
}

impl CollectedValue {
    fn into_json(self) -> Result<Value, ValidationError> {
        match self {
            Self::Null => Ok(Value::Null),
            Self::Bool(value) => Ok(Value::Bool(value)),
            Self::Signed(value) => Ok(Value::Number(value.into())),
            Self::Unsigned(value) => Ok(Value::Number(value.into())),
            Self::Float(value) => serde_json::Number::from_f64(value)
                .map(Value::Number)
                .ok_or(ValidationError::NonInteroperableNumber),
            Self::String(value) => Ok(Value::String(value)),
            Self::Array(values) => values
                .into_iter()
                .map(Self::into_json)
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            Self::Object(values) => values
                .into_iter()
                .map(|(key, value)| Ok((key, value.into_json()?)))
                .collect::<Result<serde_json::Map<_, _>, _>>()
                .map(Value::Object),
        }
    }
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

impl Serializer for ValueCollector {
    type Ok = CollectedValue;
    type Error = ValidationError;
    type SerializeSeq = SequenceCollector;
    type SerializeTuple = SequenceCollector;
    type SerializeTupleStruct = SequenceCollector;
    type SerializeTupleVariant = SequenceCollector;
    type SerializeMap = ObjectCollector;
    type SerializeStruct = ObjectCollector;
    type SerializeStructVariant = ObjectCollector;

    fn serialize_bool(self, value: bool) -> Result<Self::Ok, Self::Error> {
        Ok(CollectedValue::Bool(value))
    }

    fn serialize_i8(self, value: i8) -> Result<Self::Ok, Self::Error> {
        collect_signed(i128::from(value))
    }

    fn serialize_i16(self, value: i16) -> Result<Self::Ok, Self::Error> {
        collect_signed(i128::from(value))
    }

    fn serialize_i32(self, value: i32) -> Result<Self::Ok, Self::Error> {
        collect_signed(i128::from(value))
    }

    fn serialize_i64(self, value: i64) -> Result<Self::Ok, Self::Error> {
        collect_signed(i128::from(value))
    }

    fn serialize_i128(self, value: i128) -> Result<Self::Ok, Self::Error> {
        collect_signed(value)
    }

    fn serialize_u8(self, value: u8) -> Result<Self::Ok, Self::Error> {
        collect_unsigned(u128::from(value))
    }

    fn serialize_u16(self, value: u16) -> Result<Self::Ok, Self::Error> {
        collect_unsigned(u128::from(value))
    }

    fn serialize_u32(self, value: u32) -> Result<Self::Ok, Self::Error> {
        collect_unsigned(u128::from(value))
    }

    fn serialize_u64(self, value: u64) -> Result<Self::Ok, Self::Error> {
        collect_unsigned(u128::from(value))
    }

    fn serialize_u128(self, value: u128) -> Result<Self::Ok, Self::Error> {
        collect_unsigned(value)
    }

    fn serialize_f32(self, value: f32) -> Result<Self::Ok, Self::Error> {
        collect_float(f64::from(value))
    }

    fn serialize_f64(self, value: f64) -> Result<Self::Ok, Self::Error> {
        collect_float(value)
    }

    fn serialize_char(self, value: char) -> Result<Self::Ok, Self::Error> {
        Ok(CollectedValue::String(value.to_string()))
    }

    fn serialize_str(self, value: &str) -> Result<Self::Ok, Self::Error> {
        Ok(CollectedValue::String(value.to_owned()))
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<Self::Ok, Self::Error> {
        Ok(CollectedValue::Array(
            value
                .iter()
                .copied()
                .map(u64::from)
                .map(CollectedValue::Unsigned)
                .collect(),
        ))
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Ok(CollectedValue::Null)
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(CollectedValue::Null)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(CollectedValue::Null)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Ok(CollectedValue::String(variant.to_owned()))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        singleton_object(variant, value.serialize(self)?)
    }

    fn serialize_seq(self, length: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(SequenceCollector::new(length.unwrap_or(0), None))
    }

    fn serialize_tuple(self, length: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(SequenceCollector::new(length, None))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        length: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(SequenceCollector::new(length, None))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        length: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Ok(SequenceCollector::new(length, Some(variant)))
    }

    fn serialize_map(self, _length: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(ObjectCollector::new(None))
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(ObjectCollector::new(None))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Ok(ObjectCollector::new(Some(variant)))
    }

    fn is_human_readable(&self) -> bool {
        true
    }
}

fn collect_signed(value: i128) -> Result<CollectedValue, ValidationError> {
    validate_signed(value)?;
    let value = i64::try_from(value).map_err(|_| ValidationError::NonInteroperableNumber)?;
    Ok(CollectedValue::Signed(value))
}

fn collect_unsigned(value: u128) -> Result<CollectedValue, ValidationError> {
    validate_unsigned(value)?;
    let value = u64::try_from(value).map_err(|_| ValidationError::NonInteroperableNumber)?;
    Ok(CollectedValue::Unsigned(value))
}

fn collect_float(value: f64) -> Result<CollectedValue, ValidationError> {
    validate_serialized_float(value)?;
    Ok(CollectedValue::Float(value))
}

fn singleton_object(key: &str, value: CollectedValue) -> Result<CollectedValue, ValidationError> {
    Ok(CollectedValue::Object(BTreeMap::from([(
        key.to_owned(),
        value,
    )])))
}

struct SequenceCollector {
    values: Vec<CollectedValue>,
    variant: Option<&'static str>,
}

impl SequenceCollector {
    fn new(_length: usize, variant: Option<&'static str>) -> Self {
        Self {
            values: Vec::new(),
            variant,
        }
    }

    fn push<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), ValidationError> {
        self.values.push(value.serialize(ValueCollector)?);
        Ok(())
    }

    fn finish(self) -> Result<CollectedValue, ValidationError> {
        let value = CollectedValue::Array(self.values);
        match self.variant {
            Some(variant) => singleton_object(variant, value),
            None => Ok(value),
        }
    }
}

impl SerializeSeq for SequenceCollector {
    type Ok = CollectedValue;
    type Error = ValidationError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.push(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.finish()
    }
}

impl SerializeTuple for SequenceCollector {
    type Ok = CollectedValue;
    type Error = ValidationError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.push(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.finish()
    }
}

impl SerializeTupleStruct for SequenceCollector {
    type Ok = CollectedValue;
    type Error = ValidationError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.push(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.finish()
    }
}

impl SerializeTupleVariant for SequenceCollector {
    type Ok = CollectedValue;
    type Error = ValidationError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.push(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.finish()
    }
}

struct ObjectCollector {
    values: BTreeMap<String, CollectedValue>,
    next_key: Option<String>,
    variant: Option<&'static str>,
}

impl ObjectCollector {
    fn new(variant: Option<&'static str>) -> Self {
        Self {
            values: BTreeMap::new(),
            next_key: None,
            variant,
        }
    }

    fn insert(&mut self, key: String, value: CollectedValue) -> Result<(), ValidationError> {
        if self.values.insert(key, value).is_some() {
            Err(ValidationError::SerializationFailed)
        } else {
            Ok(())
        }
    }

    fn finish(self) -> Result<CollectedValue, ValidationError> {
        if self.next_key.is_some() {
            return Err(ValidationError::SerializationFailed);
        }
        let value = CollectedValue::Object(self.values);
        match self.variant {
            Some(variant) => singleton_object(variant, value),
            None => Ok(value),
        }
    }
}

impl SerializeMap for ObjectCollector {
    type Ok = CollectedValue;
    type Error = ValidationError;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Self::Error> {
        if self.next_key.is_some() {
            return Err(ValidationError::SerializationFailed);
        }
        self.next_key = Some(key.serialize(MapKeyCollector)?);
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        let key = self
            .next_key
            .take()
            .ok_or(ValidationError::SerializationFailed)?;
        self.insert(key, value.serialize(ValueCollector)?)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.finish()
    }
}

impl SerializeStruct for ObjectCollector {
    type Ok = CollectedValue;
    type Error = ValidationError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.insert(key.to_owned(), value.serialize(ValueCollector)?)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.finish()
    }
}

impl SerializeStructVariant for ObjectCollector {
    type Ok = CollectedValue;
    type Error = ValidationError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.insert(key.to_owned(), value.serialize(ValueCollector)?)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.finish()
    }
}

#[derive(Clone, Copy)]
struct MapKeyCollector;

impl Serializer for MapKeyCollector {
    type Ok = String;
    type Error = ValidationError;
    type SerializeSeq = Impossible<String, ValidationError>;
    type SerializeTuple = Impossible<String, ValidationError>;
    type SerializeTupleStruct = Impossible<String, ValidationError>;
    type SerializeTupleVariant = Impossible<String, ValidationError>;
    type SerializeMap = Impossible<String, ValidationError>;
    type SerializeStruct = Impossible<String, ValidationError>;
    type SerializeStructVariant = Impossible<String, ValidationError>;

    fn serialize_bool(self, value: bool) -> Result<Self::Ok, Self::Error> {
        Ok(value.to_string())
    }

    fn serialize_i8(self, value: i8) -> Result<Self::Ok, Self::Error> {
        collect_signed_key(i128::from(value))
    }

    fn serialize_i16(self, value: i16) -> Result<Self::Ok, Self::Error> {
        collect_signed_key(i128::from(value))
    }

    fn serialize_i32(self, value: i32) -> Result<Self::Ok, Self::Error> {
        collect_signed_key(i128::from(value))
    }

    fn serialize_i64(self, value: i64) -> Result<Self::Ok, Self::Error> {
        collect_signed_key(i128::from(value))
    }

    fn serialize_i128(self, value: i128) -> Result<Self::Ok, Self::Error> {
        collect_signed_key(value)
    }

    fn serialize_u8(self, value: u8) -> Result<Self::Ok, Self::Error> {
        collect_unsigned_key(u128::from(value))
    }

    fn serialize_u16(self, value: u16) -> Result<Self::Ok, Self::Error> {
        collect_unsigned_key(u128::from(value))
    }

    fn serialize_u32(self, value: u32) -> Result<Self::Ok, Self::Error> {
        collect_unsigned_key(u128::from(value))
    }

    fn serialize_u64(self, value: u64) -> Result<Self::Ok, Self::Error> {
        collect_unsigned_key(u128::from(value))
    }

    fn serialize_u128(self, value: u128) -> Result<Self::Ok, Self::Error> {
        collect_unsigned_key(value)
    }

    fn serialize_f32(self, value: f32) -> Result<Self::Ok, Self::Error> {
        validate_serialized_float(f64::from(value))?;
        serde_json_canonicalizer::to_string(&value)
            .map_err(|_| ValidationError::SerializationFailed)
    }

    fn serialize_f64(self, value: f64) -> Result<Self::Ok, Self::Error> {
        validate_serialized_float(value)?;
        serde_json_canonicalizer::to_string(&value)
            .map_err(|_| ValidationError::SerializationFailed)
    }

    fn serialize_char(self, value: char) -> Result<Self::Ok, Self::Error> {
        Ok(value.to_string())
    }

    fn serialize_str(self, value: &str) -> Result<Self::Ok, Self::Error> {
        Ok(value.to_owned())
    }

    fn serialize_bytes(self, _value: &[u8]) -> Result<Self::Ok, Self::Error> {
        Err(ValidationError::SerializationFailed)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Err(ValidationError::SerializationFailed)
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Err(ValidationError::SerializationFailed)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Err(ValidationError::SerializationFailed)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Ok(variant.to_owned())
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Err(ValidationError::SerializationFailed)
    }

    fn serialize_seq(self, _length: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(ValidationError::SerializationFailed)
    }

    fn serialize_tuple(self, _length: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(ValidationError::SerializationFailed)
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(ValidationError::SerializationFailed)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(ValidationError::SerializationFailed)
    }

    fn serialize_map(self, _length: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Err(ValidationError::SerializationFailed)
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Err(ValidationError::SerializationFailed)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(ValidationError::SerializationFailed)
    }

    fn is_human_readable(&self) -> bool {
        true
    }
}

fn collect_signed_key(value: i128) -> Result<String, ValidationError> {
    validate_signed(value)?;
    Ok(value.to_string())
}

fn collect_unsigned_key(value: u128) -> Result<String, ValidationError> {
    validate_unsigned(value)?;
    Ok(value.to_string())
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
            canonical_bytes(&-9_007_199_254_740_992_i64),
            canonical_bytes(&9_007_199_254_740_992_i64),
            canonical_bytes(&9_007_199_254_740_992_u64),
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

    #[test]
    fn canonicalization_preserves_every_integer_map_key_type() {
        macro_rules! assert_key_canonicalizes {
            ($value:expr) => {{
                let map = BTreeMap::from([($value, "value")]);
                let canonical = String::from_utf8(canonical_bytes(&map).unwrap()).unwrap();
                assert_eq!(canonical, format!(r#"{{"{}":"value"}}"#, $value));
            }};
        }

        assert_key_canonicalizes!(-1_i8);
        assert_key_canonicalizes!(-2_i16);
        assert_key_canonicalizes!(-3_i32);
        assert_key_canonicalizes!(-9_007_199_254_740_991_i64);
        assert_key_canonicalizes!(9_007_199_254_740_991_i128);
        assert_key_canonicalizes!(u8::MAX);
        assert_key_canonicalizes!(u16::MAX);
        assert_key_canonicalizes!(u32::MAX);
        assert_key_canonicalizes!(9_007_199_254_740_991_u64);
        assert_key_canonicalizes!(9_007_199_254_740_991_u128);
    }

    #[test]
    fn canonicalization_rejects_unsafe_i128_and_u128_map_keys() {
        for result in [
            canonical_bytes(&BTreeMap::from([(-9_007_199_254_740_992_i128, "unsafe")])),
            canonical_bytes(&BTreeMap::from([(9_007_199_254_740_992_i128, "unsafe")])),
            canonical_bytes(&BTreeMap::from([(9_007_199_254_740_992_u128, "unsafe")])),
        ] {
            assert_eq!(result, Err(ContractError::NonInteroperableNumber));
        }
    }

    #[test]
    fn canonicalization_validates_float_map_keys_before_string_conversion() {
        struct FloatKeyMap(f64);

        impl Serialize for FloatKeyMap {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                let mut map = serializer.serialize_map(Some(1))?;
                serde::ser::SerializeMap::serialize_entry(&mut map, &self.0, "value")?;
                serde::ser::SerializeMap::end(map)
            }
        }

        assert_eq!(
            canonical_bytes(&FloatKeyMap(1.5)).unwrap(),
            br#"{"1.5":"value"}"#
        );
        assert_eq!(
            canonical_bytes(&FloatKeyMap(9_007_199_254_740_992.0)),
            Err(ContractError::NonInteroperableNumber)
        );
        assert_eq!(
            canonical_bytes(&FloatKeyMap(f64::NAN)),
            Err(ContractError::NonInteroperableNumber)
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
    }

    #[test]
    fn canonicalization_serializes_stateful_values_exactly_once() {
        use std::cell::Cell;

        struct StatefulInteger {
            invocations: Cell<u32>,
            unsafe_first: bool,
        }

        impl Serialize for StatefulInteger {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                let invocation = self.invocations.get();
                self.invocations.set(invocation + 1);
                match (self.unsafe_first, invocation) {
                    (true, 0) => serializer.serialize_u64(MAX_SAFE_INTEGER + 1),
                    (false, 0) => serializer.serialize_u64(0),
                    (_, 1) => serializer.serialize_u64(MAX_SAFE_INTEGER + 1),
                    _ => serializer.serialize_u64(MAX_SAFE_INTEGER + 2),
                }
            }
        }

        let safe_first = StatefulInteger {
            invocations: Cell::new(0),
            unsafe_first: false,
        };
        assert_eq!(canonical_bytes(&safe_first).unwrap(), b"0");
        assert_eq!(safe_first.invocations.get(), 1);

        let unsafe_first = StatefulInteger {
            invocations: Cell::new(0),
            unsafe_first: true,
        };
        assert_eq!(
            canonical_bytes(&unsafe_first),
            Err(ContractError::NonInteroperableNumber)
        );
        assert_eq!(unsafe_first.invocations.get(), 1);
    }

    #[test]
    fn canonicalization_does_not_trust_compound_length_hints() {
        struct UntrustedLengthHint;

        impl Serialize for UntrustedLengthHint {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                let sequence = serializer.serialize_seq(Some(usize::MAX))?;
                serde::ser::SerializeSeq::end(sequence)
            }
        }

        assert_eq!(canonical_bytes(&UntrustedLengthHint).unwrap(), b"[]");
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
