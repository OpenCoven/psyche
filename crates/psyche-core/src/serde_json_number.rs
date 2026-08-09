//! Isolated handling for serde_json's `arbitrary_precision` wire protocol.
//!
//! serde_json represents number text to third-party Serde implementations as a
//! private one-field struct/map. Both sides authenticate the concrete source or
//! map-access type before accepting that marker, so user data cannot spoof it.

use std::any::type_name;
use std::str::FromStr;

use serde_json::{Number, Value};

pub(crate) const TOKEN: &str = "$serde_json::private::Number";

const NUMBER_MAP_ACCESS_TYPE: &str = "serde_json::number::NumberDeserializer";

pub(crate) fn source_may_emit_private_number<T: ?Sized>() -> bool {
    let mut source = type_name::<T>();
    while let Some(referenced) = source.strip_prefix('&') {
        source = referenced.strip_prefix("mut ").unwrap_or(referenced);
    }
    source == type_name::<Value>() || source == type_name::<Number>()
}

pub(crate) fn is_private_number_map<T: ?Sized>() -> bool {
    // An upstream representation change fails closed as an ordinary object
    // containing the reserved marker, while the canonical tests expose drift.
    type_name::<T>() == NUMBER_MAP_ACCESS_TYPE
}

pub(crate) fn parse_exact(text: &str) -> Option<Number> {
    let number = Number::from_str(text).ok()?;
    (number.to_string() == text).then_some(number)
}
