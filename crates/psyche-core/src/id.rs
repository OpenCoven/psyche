//! Validated record and request identifiers.
//!
//! [`RecordId`] and [`RequestId`] are opaque newtypes: there is no public way
//! to build one except through validation, so once a caller holds a value of
//! either type it is already known-good. Neither type exposes its raw string
//! via `TryFrom`/constructor bypass — only through [`RecordId::as_str`] /
//! [`RequestId::as_str`] and `Display`, which read rather than construct.
use std::fmt;

use crate::contracts::{ContractError, RecordKind};

/// Length of a canonical ULID string: 26 Crockford Base32 characters.
const ULID_LEN: usize = 26;

/// Crockford's Base32 alphabet: digits plus uppercase letters, excluding
/// `I`, `L`, `O`, and `U` to avoid visual confusion with `1`, `1`, `0`, and
/// `V`. Lowercase letters are absent on purpose — a canonical ULID suffix is
/// uppercase only, so a lowercase character fails this membership check
/// without any separate case check.
const CROCKFORD_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// True if `suffix` is a canonical, uppercase, 26-character ULID.
///
/// A ULID's 128 bits encode into 26 Base32 characters (130 bits of capacity),
/// leaving 2 spare bits at the top: the first character's value is
/// consequently restricted to `0..=7` rather than the full alphabet, or a
/// "valid-looking" string could denote a value the 128-bit ULID space cannot
/// hold.
fn is_canonical_ulid(suffix: &str) -> bool {
    let bytes = suffix.as_bytes();
    bytes.len() == ULID_LEN
        && bytes.iter().all(|b| CROCKFORD_ALPHABET.contains(b))
        && matches!(bytes[0], b'0'..=b'7')
}

/// A validated identifier for one of the fifteen [`RecordKind`]s.
///
/// The stored string always has the shape `<prefix><26-char canonical
/// ULID>`, where `<prefix>` is exactly the four characters
/// [`RecordKind::prefix`] returns for this id's kind — there is no path that
/// produces a `RecordId` holding a mismatched, malformed, or lowercase value.
#[derive(Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RecordId(String);

impl RecordId {
    /// Validates `value` as a `RecordId` of exactly `kind`.
    ///
    /// Rejects a prefix that names a different kind (including a
    /// plausible-looking but wrong one, e.g. `dly_` where [`RecordKind::Delivery`]
    /// requires `del_`), a suffix that is not exactly 26 characters (so
    /// trailing data after a valid ULID is rejected, not silently dropped),
    /// and a suffix that is not a canonical uppercase ULID.
    pub fn parse(kind: RecordKind, value: &str) -> Result<Self, ContractError> {
        let prefix = kind.prefix();
        let Some(suffix) = value.strip_prefix(prefix) else {
            return Err(ContractError::WrongRecordPrefix { kind });
        };
        if suffix.len() != ULID_LEN {
            return Err(ContractError::MalformedIdentifier);
        }
        if !is_canonical_ulid(suffix) {
            return Err(ContractError::InvalidUlid);
        }
        Ok(RecordId(value.to_string()))
    }

    /// Validates `value` as a `RecordId`, determining its kind from whichever
    /// of the fifteen fixed prefixes it begins with.
    ///
    /// This is what `TryFrom<String>` (and so `serde` deserialization) uses:
    /// a deserializer has no way to supply an expected kind, so it must
    /// accept any recognised kind rather than one caller-chosen kind. Use
    /// [`RecordId::parse`] instead when a specific kind is expected.
    pub fn parse_any(value: &str) -> Result<Self, ContractError> {
        for kind in RecordKind::ALL {
            if value.starts_with(kind.prefix()) {
                return Self::parse(kind, value);
            }
        }
        Err(ContractError::MalformedIdentifier)
    }

    /// The [`RecordKind`] this id was validated against.
    pub fn kind(&self) -> RecordKind {
        for kind in RecordKind::ALL {
            if self.0.starts_with(kind.prefix()) {
                return kind;
            }
        }
        // Invariant: every `RecordId` is constructed through `parse` or
        // `parse_any`, both of which require one of `RecordKind::ALL`'s
        // prefixes before returning `Ok`. No other constructor exists.
        unreachable!("RecordId held a value without a recognised RecordKind prefix")
    }

    /// The full identifier string, e.g. `"att_01ARZ3NDEKTSV4RRFFQ69G5FAV"`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for RecordId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RecordId").field(&self.0).finish()
    }
}

impl fmt::Display for RecordId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for RecordId {
    type Error = ContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse_any(&value)
    }
}

impl From<RecordId> for String {
    fn from(value: RecordId) -> Self {
        value.0
    }
}

/// A validated identifier for an ephemeral request, distinct from every
/// [`RecordKind`] — a `RequestId` is never a stored record, so it is not one
/// of the fifteen kinds and carries its own fixed `req_` prefix instead of
/// [`RecordKind::prefix`].
#[derive(Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RequestId(String);

impl RequestId {
    /// The fixed prefix every `RequestId` begins with.
    pub const PREFIX: &'static str = "req_";

    /// Validates `value` as `req_` followed by a canonical uppercase
    /// 26-character ULID, with no trailing data.
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        let Some(suffix) = value.strip_prefix(Self::PREFIX) else {
            return Err(ContractError::MalformedIdentifier);
        };
        if suffix.len() != ULID_LEN {
            return Err(ContractError::MalformedIdentifier);
        }
        if !is_canonical_ulid(suffix) {
            return Err(ContractError::InvalidUlid);
        }
        Ok(RequestId(value.to_string()))
    }

    /// The full identifier string, e.g. `"req_01ARZ3NDEKTSV4RRFFQ69G5FAV"`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RequestId").field(&self.0).finish()
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for RequestId {
    type Error = ContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<RequestId> for String {
    fn from(value: RequestId) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use crate::contracts::{ContractError, RecordKind};
    use crate::id::{RecordId, RequestId};

    const A_ULID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    const ANOTHER_ULID: &str = "01BX5ZZKBKACTAV9WEVGEMMVRZ";

    #[test]
    fn rejects_a_record_id_with_the_wrong_kind_prefix() {
        let value = format!("int_{A_ULID}");
        let err = RecordId::parse(RecordKind::Attempt, &value).unwrap_err();
        assert!(matches!(
            err,
            ContractError::WrongRecordPrefix {
                kind: RecordKind::Attempt
            }
        ));
    }

    #[test]
    fn delivery_is_authoritatively_del_and_rejects_dly() {
        let del = format!("del_{A_ULID}");
        assert!(RecordId::parse(RecordKind::Delivery, &del).is_ok());

        let dly = format!("dly_{A_ULID}");
        assert!(RecordId::parse(RecordKind::Delivery, &dly).is_err());
        assert!(RecordId::try_from(dly).is_err());
    }

    #[test]
    fn delegation_is_distinctly_dlg_and_rejects_del() {
        let dlg = format!("dlg_{A_ULID}");
        assert!(RecordId::parse(RecordKind::Delegation, &dlg).is_ok());

        let del = format!("del_{A_ULID}");
        assert!(RecordId::parse(RecordKind::Delegation, &del).is_err());
    }

    #[test]
    fn attempt_accepts_only_att_prefix() {
        let att = format!("att_{ANOTHER_ULID}");
        let id = RecordId::parse(RecordKind::Attempt, &att).unwrap();
        assert_eq!(id.kind(), RecordKind::Attempt);
        assert_eq!(id.as_str(), att);
    }

    #[test]
    fn rejects_lowercase_ulid_suffix() {
        let lower = format!("att_{}", A_ULID.to_lowercase());
        assert!(RecordId::parse(RecordKind::Attempt, &lower).is_err());
    }

    #[test]
    fn rejects_non_canonical_ulid_characters() {
        // 'U' is excluded from Crockford's Base32 alphabet.
        let bad = format!("att_{}", "0".repeat(25) + "U");
        assert!(RecordId::parse(RecordKind::Attempt, &bad).is_err());
    }

    #[test]
    fn rejects_trailing_data_after_the_ulid() {
        let trailing = format!("att_{A_ULID}X");
        assert!(RecordId::parse(RecordKind::Attempt, &trailing).is_err());
    }

    #[test]
    fn record_id_serde_round_trips_and_rejects() {
        let id = RecordId::parse(RecordKind::Graph, &format!("grf_{A_ULID}")).unwrap();
        let json = serde_json::to_string(&id).unwrap();
        let back: RecordId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);

        assert!(serde_json::from_str::<RecordId>("\"grf_not-a-ulid\"").is_err());
    }

    #[test]
    fn request_id_is_not_a_record_kind_and_validates_its_own_prefix() {
        let req = format!("req_{A_ULID}");
        let id = RequestId::parse(&req).unwrap();
        assert_eq!(id.as_str(), req);

        assert!(RequestId::parse(&format!("rqx_{A_ULID}")).is_err());
        assert!(RequestId::parse(&format!("req_{}", A_ULID.to_lowercase())).is_err());
    }

    #[test]
    fn request_id_serde_round_trips_and_rejects() {
        let id = RequestId::parse(&format!("req_{A_ULID}")).unwrap();
        let json = serde_json::to_string(&id).unwrap();
        let back: RequestId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);

        assert!(serde_json::from_str::<RequestId>("\"req_short\"").is_err());
    }

    #[test]
    fn record_id_all_fifteen_prefixes_round_trip_through_parse_any() {
        // Every declared kind's canonical prefix + a valid ULID must both
        // parse under its own kind and be recovered by `kind()` — pins that
        // `RecordKind::ALL`, `RecordKind::prefix`, and `parse_any`'s lookup
        // never drift out of sync with each other.
        for kind in RecordKind::ALL {
            let value = format!("{}{A_ULID}", kind.prefix());
            let via_kind = RecordId::parse(kind, &value).unwrap();
            let via_any = RecordId::try_from(value.clone()).unwrap();
            assert_eq!(via_kind, via_any);
            assert_eq!(via_any.kind(), kind);
            assert_eq!(via_any.as_str(), value);
        }
    }

    #[test]
    fn record_id_rejects_near_misses() {
        for near in [
            "",
            "att_",
            A_ULID,                                    // missing prefix entirely
            &format!("att{A_ULID}"),                   // missing underscore
            &format!("att_{}", &A_ULID[..25]),         // one char short
            &format!("att_{A_ULID}Z"),                 // one char over (trailing)
            &format!("att_{}", A_ULID.to_lowercase()), // lowercase suffix
            &format!("ATT_{A_ULID}"),                  // uppercase prefix
        ] {
            assert!(
                RecordId::try_from(near.to_string()).is_err(),
                "expected rejection for {near:?}"
            );
        }
    }

    #[test]
    fn request_id_rejects_near_misses() {
        for near in [
            "",
            "req_",
            A_ULID,                            // missing prefix
            &format!("req{A_ULID}"),           // missing underscore
            &format!("req_{}", &A_ULID[..25]), // one char short
            &format!("req_{A_ULID}Z"),         // one char over
            &format!("REQ_{A_ULID}"),          // uppercase prefix
            &format!("del_{A_ULID}"),          // a RecordKind prefix, not req_
        ] {
            assert!(
                RequestId::parse(near).is_err(),
                "expected rejection for {near:?}"
            );
        }
    }
}
