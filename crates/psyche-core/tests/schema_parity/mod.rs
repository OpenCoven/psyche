//! Shared harness for the experimental record JSON Schemas under #11.
//!
//! Each record kind has a hand-written Draft 2020-12 schema in `schemas/`
//! and a corpus derived in code from its pinned #50 vector. Every case runs
//! through both [`decode_document`] and the schema, and pins the decoder's
//! exact result and the schema's verdict. The schema must accept everything
//! the decoder accepts. A case the decoder rejects and the schema accepts is
//! allowed only when it is declared [`Expect::ParserOnly`] (or
//! [`Expect::ParserDependent`]) and listed in the kind's parser-only table,
//! because the typed decoder, not the schema, is the authority on validity.
//!
//! Nothing here reads the network or any file outside the repository: the
//! validator's URL loader refuses every load, and the 2020-12 metaschema is
//! compiled into the validator.
#![allow(dead_code, clippy::expect_used, clippy::unwrap_used)]

use std::collections::{BTreeSet, HashMap};
use std::error::Error;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use psyche_core::contracts::{ContractError, SchemaKind, decode_document};
use psyche_core::digest::canonical_bytes;
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};

pub(crate) const DRAFT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";
pub(crate) const CHECKSUMS_FILE: &str = "tests/fixtures/vectors/SHA256SUMS";
pub(crate) const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;
/// A canonical ULID that no vector uses as its own record ID suffix.
pub(crate) const ULID: &str = "01J00000000000000000000001";

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Expect {
    /// Both implementations accept.
    Accept,
    /// Both reject; the decoder with exactly this error.
    Reject(ContractError),
    /// The decoder rejects with this error and the schema accepts, because
    /// the rule is outside what a structural schema states. Each such case
    /// is named in the kind's parser-only table.
    ParserOnly(ContractError),
    /// The decoder rejects with this error. Whether a schema-only consumer
    /// rejects depends on its JSON parser: this test's parser refuses the
    /// bytes, but RFC 8259 leaves them to the implementation and common
    /// parsers accept some of them. Named alongside the parser-only cases.
    ParserDependent(ContractError),
}

pub(crate) struct Case {
    pub(crate) name: &'static str,
    pub(crate) bytes: Vec<u8>,
    pub(crate) expect: Expect,
}

impl Case {
    pub(crate) fn new(name: impl Into<String>, bytes: Vec<u8>, expect: Expect) -> Self {
        Self {
            name: leak(name.into()),
            bytes,
            expect,
        }
    }
}

pub(crate) fn leak(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}

/// One record kind: where its schema, vector and parser-only table live.
pub(crate) struct Kind {
    /// The `<kind>` segment of `psyche.<kind>.v1`.
    pub(crate) name: &'static str,
    pub(crate) schema: SchemaKind,
    /// The #50 pin for the vector (`SHA256SUMS` and `canonical_vectors.rs`).
    pub(crate) pinned_sha256: &'static str,
    /// Top-level members the decoder accepts when absent.
    pub(crate) optional_members: &'static [&'static str],
    /// The document holding the parser-only tables.
    pub(crate) doc: &'static str,
    /// The marker names of the tables that together list exactly this
    /// kind's parser-only and parser-dependent cases.
    pub(crate) doc_sections: &'static [&'static str],
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub(crate) fn repo_file(relative: &str) -> Vec<u8> {
    let path = manifest_dir().join("../..").join(relative);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

pub(crate) fn crate_file(relative: &str) -> Vec<u8> {
    let path = manifest_dir().join(relative);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(crate) fn json_error() -> ContractError {
    ContractError::InvalidShape {
        schema: SchemaKind::Error,
        field: "json",
    }
}

pub(crate) fn probe_error() -> ContractError {
    ContractError::InvalidShape {
        schema: SchemaKind::Error,
        field: "schema_version",
    }
}

pub(crate) fn replace_once(source: &[u8], from: &str, to: &str) -> Vec<u8> {
    let text = std::str::from_utf8(source).unwrap();
    assert_eq!(text.matches(from).count(), 1, "{from:?} must occur once");
    text.replacen(from, to, 1).into_bytes()
}

pub(crate) fn nested(depth: usize) -> Value {
    (0..depth).fold(json!(true), |inner, _| json!([inner]))
}

pub(crate) fn id(prefix: &str, suffix: &str) -> Value {
    Value::String(format!("{prefix}{suffix}"))
}

/// Refuses every load, so a `$ref` to anything other than the embedded
/// metaschema or the schema itself fails compilation instead of reading a
/// file or the network.
struct NoLoader;

impl boon::UrlLoader for NoLoader {
    fn load(&self, url: &str) -> Result<Value, Box<dyn Error>> {
        Err(format!("offline parity test refuses to load {url}").into())
    }
}

type Compiled = (boon::Schemas, boon::SchemaIndex);

fn path_at<'a>(map: &'a mut Map<String, Value>, path: &[&str]) -> &'a mut Map<String, Value> {
    let mut current = map;
    for segment in path {
        current = current
            .get_mut(*segment)
            .and_then(Value::as_object_mut)
            .unwrap_or_else(|| panic!("{segment} is not an object member"));
    }
    current
}

fn path_name(path: &[&str]) -> String {
    path.join(".")
}

impl Kind {
    pub(crate) fn schema_version(&self) -> String {
        format!("psyche.{}.v1", self.name)
    }

    pub(crate) fn schema_file(&self) -> String {
        format!("schemas/psyche.{}.v1.schema.json", self.name)
    }

    pub(crate) fn schema_id(&self) -> String {
        format!("urn:opencoven:psyche:schema:psyche.{}.v1", self.name)
    }

    fn vector_file_name(&self) -> String {
        format!("psyche.{}.v1.json", self.name)
    }

    pub(crate) fn vector(&self) -> Vec<u8> {
        crate_file(&format!(
            "tests/fixtures/vectors/{}",
            self.vector_file_name()
        ))
    }

    pub(crate) fn vector_object(&self) -> Map<String, Value> {
        match serde_json::from_slice(&self.vector()).unwrap() {
            Value::Object(map) => map,
            _ => unreachable!("every vector is an object"),
        }
    }

    pub(crate) fn schema_value(&self) -> Value {
        serde_json::from_slice(&repo_file(&self.schema_file())).unwrap()
    }

    /// The decoder's generic error for this kind: every serde failure,
    /// including a failed `validate()` inside `Deserialize`, maps to it.
    pub(crate) fn document(&self) -> ContractError {
        ContractError::InvalidShape {
            schema: self.schema,
            field: "document",
        }
    }

    pub(crate) fn invalid(&self, field: &'static str) -> ContractError {
        ContractError::InvalidShape {
            schema: self.schema,
            field,
        }
    }

    fn compiled(&self) -> &'static Compiled {
        static COMPILED: OnceLock<Mutex<HashMap<&'static str, &'static Compiled>>> =
            OnceLock::new();
        let mut cache = COMPILED
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap();
        cache.entry(self.name).or_insert_with(|| {
            let id = self.schema_id();
            let mut schemas = boon::Schemas::new();
            let mut compiler = boon::Compiler::new();
            compiler.set_default_draft(boon::Draft::V2020_12);
            compiler.use_loader(Box::new(NoLoader));
            compiler.add_resource(&id, self.schema_value()).unwrap();
            let index = compiler.compile(&id, &mut schemas).unwrap_or_else(|e| {
                panic!("{} does not compile against 2020-12: {e:#}", self.name)
            });
            Box::leak(Box::new((schemas, index)))
        })
    }

    /// The schema side: a consumer with an ordinary JSON parser and a
    /// 2020-12 validator. Bytes the ordinary parser cannot read are rejected
    /// before the schema runs.
    pub(crate) fn schema_accepts(&self, bytes: &[u8]) -> bool {
        let Ok(instance) = serde_json::from_slice::<Value>(bytes) else {
            return false;
        };
        let (schemas, index) = self.compiled();
        schemas.validate(&instance, *index).is_ok()
    }

    pub(crate) fn doc_of(map: &Map<String, Value>) -> Vec<u8> {
        serde_json::to_vec(map).unwrap()
    }

    /// The vector with the member at `path` replaced.
    pub(crate) fn with_at(&self, path: &[&str], value: Value) -> Vec<u8> {
        let (last, parent) = path.split_last().unwrap();
        let mut map = self.vector_object();
        let target = path_at(&mut map, parent);
        assert!(
            target.contains_key(*last),
            "{} is not a vector member",
            path_name(path)
        );
        target.insert((*last).to_owned(), value);
        Self::doc_of(&map)
    }

    /// The vector with several members replaced, applied in order.
    pub(crate) fn with_all(&self, changes: &[(&[&str], Value)]) -> Vec<u8> {
        let mut map = self.vector_object();
        for (path, value) in changes {
            let (last, parent) = path.split_last().unwrap();
            let target = path_at(&mut map, parent);
            assert!(
                target.contains_key(*last),
                "{} is not a vector member",
                path_name(path)
            );
            target.insert((*last).to_owned(), value.clone());
        }
        Self::doc_of(&map)
    }

    pub(crate) fn with(&self, field: &str, value: Value) -> Vec<u8> {
        self.with_at(&[field], value)
    }

    pub(crate) fn without_at(&self, path: &[&str]) -> Vec<u8> {
        let (last, parent) = path.split_last().unwrap();
        let mut map = self.vector_object();
        assert!(
            path_at(&mut map, parent).remove(*last).is_some(),
            "{} is not a vector member",
            path_name(path)
        );
        Self::doc_of(&map)
    }

    pub(crate) fn without(&self, field: &str) -> Vec<u8> {
        self.without_at(&[field])
    }

    pub(crate) fn with_extra_at(&self, path: &[&str], field: &str, value: Value) -> Vec<u8> {
        let mut map = self.vector_object();
        assert!(
            path_at(&mut map, path)
                .insert(field.to_owned(), value)
                .is_none()
        );
        Self::doc_of(&map)
    }

    pub(crate) fn with_extra(&self, field: &str, value: Value) -> Vec<u8> {
        self.with_extra_at(&[], field, value)
    }

    pub(crate) fn string_at(&self, path: &[&str]) -> String {
        let (last, parent) = path.split_last().unwrap();
        let mut map = self.vector_object();
        path_at(&mut map, parent)[*last]
            .as_str()
            .unwrap_or_else(|| panic!("{} is not a string", path_name(path)))
            .to_owned()
    }

    // --- case generators ------------------------------------------------

    /// Cases every kind shares: the vector and its spellings, the version
    /// probe, unknown and missing members, and malformed input. `nested`
    /// names each closed object member present in the vector, the members
    /// the decoder accepts when absent from it, and the error the decoder
    /// returns for an unknown or missing member there.
    pub(crate) fn standard_cases(&self, nested: &[(&[&str], &[&str], ContractError)]) -> Vec<Case> {
        use Expect::{Accept, ParserDependent, ParserOnly, Reject};
        let vector = self.vector();
        // Intent has no enum the decoder inspects before decoding, so a
        // document relabelled as intent fails on shape alone. Kinds with a
        // top-level `state` would fail as a graph on that state first, which
        // graph_node and delivery pin as their own cases.
        let other = if matches!(
            self.schema,
            SchemaKind::Graph | SchemaKind::GraphNode | SchemaKind::Delivery
        ) {
            ("psyche.intent.v1", SchemaKind::Intent)
        } else {
            ("psyche.graph.v1", SchemaKind::Graph)
        };
        let mut cases = vec![
            Case::new("pinned_vector", vector.clone(), Accept),
            Case::new(
                "pretty_printed_vector",
                serde_json::to_vec_pretty(&self.vector_object()).unwrap(),
                Accept,
            ),
            Case::new(
                "trailing_newline",
                [vector.clone(), b"\n".to_vec()].concat(),
                Accept,
            ),
            Case::new(
                "unknown_top_level_field",
                self.with_extra("aa_unknown_field", json!(true)),
                Reject(self.document()),
            ),
            Case::new(
                "schema_version_major_2",
                self.with("schema_version", json!(format!("psyche.{}.v2", self.name))),
                Reject(ContractError::UnsupportedMajor {
                    found: 2,
                    supported: 1,
                }),
            ),
            Case::new(
                "schema_version_major_0",
                self.with("schema_version", json!(format!("psyche.{}.v0", self.name))),
                Reject(ContractError::UnsupportedMajor {
                    found: 0,
                    supported: 1,
                }),
            ),
            Case::new(
                "schema_version_unknown_kind",
                self.with(
                    "schema_version",
                    json!(format!("psyche.{}_unknown.v1", self.name)),
                ),
                Reject(ContractError::UnknownSchema),
            ),
            Case::new(
                "schema_version_other_kind",
                self.with("schema_version", json!(other.0)),
                Reject(ContractError::InvalidShape {
                    schema: other.1,
                    field: "document",
                }),
            ),
            Case::new(
                "schema_version_not_a_string",
                self.with("schema_version", json!(1)),
                Reject(probe_error()),
            ),
            Case::new("empty_input", Vec::new(), Reject(json_error())),
            Case::new(
                "truncated",
                vector[..vector.len() - 1].to_vec(),
                Reject(json_error()),
            ),
            Case::new(
                "trailing_data",
                [vector.clone(), b"{}".to_vec()].concat(),
                Reject(json_error()),
            ),
            Case::new(
                "trailing_comma",
                [&vector[..vector.len() - 1], b",}"].concat(),
                Reject(json_error()),
            ),
            Case::new("array_root", b"[]".to_vec(), Reject(probe_error())),
            Case::new(
                "string_root",
                json!(self.schema_version()).to_string().into_bytes(),
                Reject(probe_error()),
            ),
            Case::new(
                "byte_order_mark",
                [b"\xef\xbb\xbf".to_vec(), vector.clone()].concat(),
                ParserDependent(json_error()),
            ),
            Case::new(
                "invalid_utf8",
                // Inside the first top-level key: vectors are canonical, so the
                // document opens with `{"`.
                [&vector[..2], b"\xff", &vector[2..]].concat(),
                ParserDependent(json_error()),
            ),
            Case::new(
                "lone_surrogate_escape",
                [&vector[..2], b"\\ud800", &vector[2..]].concat(),
                ParserDependent(json_error()),
            ),
            Case::new(
                "duplicate_top_level_key",
                [
                    format!("{{\"schema_version\":\"{}\",", self.schema_version()).into_bytes(),
                    vector[1..].to_vec(),
                ]
                .concat(),
                ParserOnly(json_error()),
            ),
            Case::new(
                "document_over_1_mib",
                [vector.clone(), vec![b' '; MAX_DOCUMENT_BYTES]].concat(),
                ParserOnly(ContractError::DocumentTooLarge),
            ),
        ];
        for field in self.vector_object().keys() {
            if self.optional_members.contains(&field.as_str()) {
                continue;
            }
            let expect = if field == "schema_version" {
                Reject(probe_error())
            } else {
                Reject(self.document())
            };
            cases.push(Case::new(
                format!("missing_{field}"),
                self.without(field),
                expect,
            ));
        }
        for (path, optional, error) in nested {
            let name = path_name(path);
            cases.push(Case::new(
                format!("unknown_{name}_field"),
                self.with_extra_at(path, "aa_unknown_field", json!(true)),
                Reject(error.clone()),
            ));
            let mut map = self.vector_object();
            let members: Vec<String> = path_at(&mut map, path).keys().cloned().collect();
            for member in members {
                if optional.contains(&member.as_str()) {
                    continue;
                }
                let mut full = path.to_vec();
                full.push(&member);
                cases.push(Case::new(
                    format!("missing_{name}_{member}"),
                    self.without_at(&full),
                    Reject(error.clone()),
                ));
            }
        }
        cases
    }

    /// Length bounds on a string member the decoder checks with `bounded()`,
    /// which counts UTF-8 bytes: `maxLength` counts code points, so the two
    /// agree for ASCII and a multibyte value over the byte bound is
    /// parser-only.
    pub(crate) fn bounded_string_cases(
        &self,
        path: &[&str],
        max: usize,
        error: &ContractError,
    ) -> Vec<Case> {
        self.bounded_string_cases_with(path, max, error, error)
    }

    /// As [`Self::bounded_string_cases`], for a decoder that reports a bound
    /// failure (`bound`) apart from a wrong JSON type (`wrong_type`).
    pub(crate) fn bounded_string_cases_with(
        &self,
        path: &[&str],
        max: usize,
        bound: &ContractError,
        wrong_type: &ContractError,
    ) -> Vec<Case> {
        use Expect::{Accept, ParserOnly, Reject};
        let error = bound;
        let name = path_name(path);
        let over = max / 2 + 1;
        vec![
            Case::new(
                format!("{name}_{max}_bytes"),
                self.with_at(path, json!("a".repeat(max))),
                Accept,
            ),
            Case::new(
                format!("{name}_multibyte_within_bytes"),
                self.with_at(path, json!("\u{e9}".repeat(max / 2))),
                Accept,
            ),
            Case::new(
                format!("{name}_empty"),
                self.with_at(path, json!("")),
                Reject(error.clone()),
            ),
            Case::new(
                format!("{name}_{}_bytes", max + 1),
                self.with_at(path, json!("a".repeat(max + 1))),
                Reject(error.clone()),
            ),
            Case::new(
                format!("{name}_number"),
                self.with_at(path, json!(7)),
                Reject(wrong_type.clone()),
            ),
            Case::new(
                format!("{name}_multibyte_over_{max}_bytes"),
                self.with_at(path, json!("\u{e9}".repeat(over))),
                ParserOnly(error.clone()),
            ),
        ]
    }

    /// A record identifier member: other and unregistered prefixes, and the
    /// ULID shapes the sweep does not reach. The sweep pins exactness.
    /// `wrong_kind` is the decoder's error for a registered prefix of
    /// another kind, which differs from `error` only where the kind's
    /// `validate()` runs outside `Deserialize`.
    pub(crate) fn id_cases(
        &self,
        path: &[&str],
        prefix: &str,
        wrong_prefix: &str,
        wrong_kind: &ContractError,
        error: &ContractError,
    ) -> Vec<Case> {
        use Expect::Reject;
        let name = path_name(path);
        vec![
            Case::new(
                format!("{name}_wrong_prefix"),
                self.with_at(path, id(wrong_prefix, ULID)),
                Reject(wrong_kind.clone()),
            ),
            Case::new(
                format!("{name}_unregistered_prefix"),
                self.with_at(path, id("dly_", ULID)),
                Reject(error.clone()),
            ),
            Case::new(
                format!("{name}_lowercase_ulid"),
                self.with_at(path, id(prefix, &ULID.to_lowercase())),
                Reject(error.clone()),
            ),
            Case::new(
                format!("{name}_ulid_overflow"),
                self.with_at(path, id(prefix, &format!("8{}", &ULID[1..]))),
                Reject(error.clone()),
            ),
            Case::new(
                format!("{name}_ulid_27_characters"),
                self.with_at(path, id(prefix, &format!("{ULID}0"))),
                Reject(error.clone()),
            ),
            Case::new(
                format!("{name}_number"),
                self.with_at(path, json!(1)),
                Reject(error.clone()),
            ),
        ]
    }

    pub(crate) fn digest_cases(&self, path: &[&str], error: &ContractError) -> Vec<Case> {
        use Expect::Reject;
        let name = path_name(path);
        vec![
            Case::new(
                format!("{name}_uppercase_hex"),
                self.with_at(path, json!(format!("sha256:{}", "A".repeat(64)))),
                Reject(error.clone()),
            ),
            Case::new(
                format!("{name}_other_algorithm"),
                self.with_at(path, json!(format!("sha512:{}", "1".repeat(64)))),
                Reject(error.clone()),
            ),
            Case::new(
                format!("{name}_trailing_newline"),
                self.with_at(path, json!(format!("sha256:{}\n", "1".repeat(64)))),
                Reject(error.clone()),
            ),
            Case::new(
                format!("{name}_null"),
                self.with_at(path, Value::Null),
                Reject(error.clone()),
            ),
        ]
    }

    /// An RFC 3339 member with no ordering rule against another member.
    pub(crate) fn timestamp_cases(&self, path: &[&str], error: &ContractError) -> Vec<Case> {
        use Expect::{Accept, ParserOnly, Reject};
        let name = path_name(path);
        let at = |value: &str| self.with_at(path, json!(value));
        vec![
            Case::new(
                format!("{name}_lowercase_t_and_z"),
                at("2026-08-01t00:00:00z"),
                Accept,
            ),
            Case::new(
                format!("{name}_separator_x"),
                at("2026-08-01X00:00:00Z"),
                Accept,
            ),
            Case::new(
                format!("{name}_positive_offset"),
                at("2026-08-01T00:00:00+01:00"),
                Accept,
            ),
            Case::new(
                format!("{name}_ten_fraction_digits"),
                at("2026-08-01T00:00:00.1234567891Z"),
                Accept,
            ),
            Case::new(
                format!("{name}_no_offset"),
                at("2026-08-01T00:00:00"),
                Reject(error.clone()),
            ),
            Case::new(
                format!("{name}_hour_24"),
                at("2026-08-01T24:00:00Z"),
                Reject(error.clone()),
            ),
            Case::new(
                format!("{name}_offset_without_colon"),
                at("2026-08-01T00:00:00+0100"),
                Reject(error.clone()),
            ),
            Case::new(
                format!("{name}_trailing_newline"),
                at("2026-08-01T00:00:00Z\n"),
                Reject(error.clone()),
            ),
            Case::new(
                format!("{name}_number"),
                self.with_at(path, json!(0)),
                Reject(error.clone()),
            ),
            Case::new(
                format!("{name}_impossible_date"),
                at("2026-02-30T00:00:00Z"),
                ParserOnly(error.clone()),
            ),
            Case::new(
                format!("{name}_leap_second_off_boundary"),
                at("2026-08-01T23:59:60Z"),
                ParserOnly(error.clone()),
            ),
        ]
    }

    /// An unsigned integer member bounded by the decoder's Rust type and the
    /// interoperable range. `least` is the smallest accepted value. The
    /// document-wide number check runs before the typed decode, so a value
    /// above 2^53-1 fails with `NonInteroperableNumber`.
    pub(crate) fn integer_cases(
        &self,
        path: &[&str],
        least: u64,
        greatest: u64,
        error: &ContractError,
    ) -> Vec<Case> {
        use Expect::{Accept, ParserOnly, Reject};
        let name = path_name(path);
        let text = |raw: &str| {
            let mut map = self.vector_object();
            let (last, parent) = path.split_last().unwrap();
            path_at(&mut map, parent).insert((*last).to_owned(), json!("__N__"));
            replace_once(&Self::doc_of(&map), "\"__N__\"", raw)
        };
        let mut cases = vec![
            Case::new(
                format!("{name}_least"),
                self.with_at(path, json!(least)),
                Accept,
            ),
            Case::new(
                format!("{name}_greatest"),
                self.with_at(path, json!(greatest)),
                Accept,
            ),
            Case::new(
                format!("{name}_negative"),
                self.with_at(path, json!(-1)),
                Reject(error.clone()),
            ),
            Case::new(
                format!("{name}_fraction"),
                text(&format!("{least}.5")),
                Reject(error.clone()),
            ),
            Case::new(
                format!("{name}_string"),
                self.with_at(path, json!(least.to_string())),
                Reject(error.clone()),
            ),
            Case::new(
                format!("{name}_integral_fraction"),
                text(&format!("{least}.0")),
                ParserOnly(error.clone()),
            ),
            Case::new(
                format!("{name}_exponent"),
                text(&format!("{least}e0")),
                ParserOnly(error.clone()),
            ),
        ];
        if least > 0 {
            cases.push(Case::new(
                format!("{name}_below_least"),
                self.with_at(path, json!(least - 1)),
                Reject(error.clone()),
            ));
        }
        if greatest < 9_007_199_254_740_991 {
            cases.push(Case::new(
                format!("{name}_above_greatest"),
                self.with_at(path, json!(greatest + 1)),
                Reject(error.clone()),
            ));
        } else {
            cases.push(Case::new(
                format!("{name}_unsafe"),
                text("9007199254740992"),
                Reject(ContractError::NonInteroperableNumber),
            ));
        }
        cases
    }

    // --- checks ---------------------------------------------------------

    pub(crate) fn check_compiles(&self) {
        let schema = self.schema_value();
        assert_eq!(schema["$schema"], DRAFT_2020_12);
        assert_eq!(schema["$id"], json!(self.schema_id()));
        let _ = self.compiled();
    }

    /// The top-level `properties` equal the members of the typed record's
    /// canonical encoding, `required` is every member except the optional
    /// ones, unknown members are refused and `schema_version` is the exact
    /// constant. The member list comes from the vector rather than the Rust
    /// type: a member added to the record changes the vector's bytes and
    /// fails the #50 pin first, then this check.
    pub(crate) fn check_members(&self) {
        let schema = self.schema_value();
        check_object_members(
            &schema,
            &schema,
            &self.vector_object(),
            self.optional_members,
            self.name,
        );
        assert_eq!(
            schema["properties"]["schema_version"]["const"],
            json!(self.schema_version())
        );
    }

    /// The same check for a closed object member, against the canonical
    /// encoding of an accepted document that carries it.
    pub(crate) fn check_nested_members(&self, path: &[&str], accepted: &[u8], optional: &[&str]) {
        let schema = self.schema_value();
        let canonical = canonical_bytes(&decode_document(accepted).unwrap()).unwrap();
        let mut map: Map<String, Value> = serde_json::from_slice(&canonical).unwrap();
        let object = path_at(&mut map, path).clone();
        let sub = subschema(&schema, path);
        check_object_members(&schema, sub, &object, optional, &path_name(path));
    }

    /// The schema's `enum` at `path` is exactly `values`. No listed value is
    /// refused as an unknown enum value by the decoder, and the schema
    /// accepts every document the decoder accepts with it.
    pub(crate) fn check_enum(&self, path: &[&str], values: &[&str]) {
        let schema = self.schema_value();
        let listed: BTreeSet<String> = resolve(&schema, subschema(&schema, path))["enum"]
            .as_array()
            .unwrap_or_else(|| panic!("{} has no enum", path_name(path)))
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect();
        let expected: BTreeSet<String> = values.iter().map(|v| (*v).to_owned()).collect();
        assert_eq!(listed, expected, "{} enum", path_name(path));
        let mut accepted = 0;
        for value in values {
            let bytes = self.with_at(path, json!(value));
            let decoded = decode_document(&bytes);
            assert!(
                !matches!(decoded, Err(ContractError::UnknownEnumValue { .. })),
                "{}={value} is refused as unknown",
                path_name(path)
            );
            if decoded.is_ok() {
                accepted += 1;
                assert!(
                    self.schema_accepts(&bytes),
                    "{}={value}: schema rejects a decoder-accepted document",
                    path_name(path)
                );
            }
        }
        assert!(
            accepted > 0,
            "no value of {} decodes alone",
            path_name(path)
        );
    }

    pub(crate) fn check_pinned_vector(&self) {
        let bytes = self.vector();
        assert_eq!(sha256_hex(&bytes), self.pinned_sha256);
        let sums = String::from_utf8(crate_file(CHECKSUMS_FILE)).unwrap();
        let line = format!("{} *{}", self.pinned_sha256, self.vector_file_name());
        assert!(
            sums.lines().any(|l| l == line),
            "SHA256SUMS must pin the {} vector",
            self.name
        );
        let decoded = decode_document(&bytes).unwrap();
        assert_eq!(decoded.schema_version().kind, self.schema);
        assert_eq!(
            canonical_bytes(&decoded).unwrap(),
            bytes,
            "canonical bytes drifted"
        );
        assert!(
            self.schema_accepts(&bytes),
            "schema rejects the pinned vector"
        );
    }

    pub(crate) fn check_corpus(&self, corpus: &[Case]) {
        let mut names = BTreeSet::new();
        let mut failures = Vec::new();
        for case in corpus {
            assert!(names.insert(case.name), "duplicate case {}", case.name);
            let decoded = decode_document(&case.bytes).map(|_| ());
            let schema = self.schema_accepts(&case.bytes);
            let ok = match &case.expect {
                Expect::Accept => decoded.is_ok() && schema,
                Expect::Reject(error) => decoded.as_ref().err() == Some(error) && !schema,
                Expect::ParserOnly(error) => decoded.as_ref().err() == Some(error) && schema,
                Expect::ParserDependent(error) => {
                    decoded.as_ref().err() == Some(error)
                        && serde_json::from_slice::<Value>(&case.bytes).is_err()
                }
            };
            if !ok {
                failures.push(format!(
                    "{}: expected {:?}, decoder {:?}, schema accepts {schema}",
                    case.name, case.expect, decoded
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "{} parity failures:\n{}",
            self.name,
            failures.join("\n")
        );
    }

    /// Re-encoding through the typed record is what a producer emits, so the
    /// canonical form of every accepted case must pass the schema as well.
    pub(crate) fn check_canonical_forms(&self, corpus: &[Case]) {
        for case in corpus.iter().filter(|c| c.expect == Expect::Accept) {
            let decoded = decode_document(&case.bytes).unwrap();
            let canonical = canonical_bytes(&decoded).unwrap();
            assert!(
                self.schema_accepts(&canonical),
                "{}: canonical form rejected",
                case.name
            );
            assert_eq!(
                canonical_bytes(&decode_document(&canonical).unwrap()).unwrap(),
                canonical,
                "{}: canonical form is not a fixed point",
                case.name
            );
        }
    }

    pub(crate) fn check_documented(&self, corpus: &[Case]) {
        let doc = String::from_utf8(repo_file(self.doc)).unwrap();
        let parser_only: BTreeSet<&str> = corpus
            .iter()
            .filter(|c| matches!(c.expect, Expect::ParserOnly(_) | Expect::ParserDependent(_)))
            .map(|c| c.name)
            .collect();
        assert!(!parser_only.is_empty());
        let mut listed = BTreeSet::new();
        for section in self.doc_sections {
            let body = doc
                .split(&format!("<!-- {section}:start -->"))
                .nth(1)
                .and_then(|rest| rest.split(&format!("<!-- {section}:end -->")).next())
                .unwrap_or_else(|| panic!("{} lacks the {section} markers", self.doc));
            for name in body
                .lines()
                .filter_map(|line| line.strip_prefix("| `"))
                .filter_map(|rest| rest.split('`').next())
            {
                assert!(listed.insert(name), "{section}: `{name}` listed twice");
            }
        }
        assert_eq!(
            listed, parser_only,
            "{}: the documented parser-only cases differ from the corpus",
            self.name
        );
    }

    /// Guards the corpus itself: a helper that silently returned the vector
    /// would turn a rejection case into a second copy of the positive case.
    pub(crate) fn check_rejections_mutate(&self, corpus: &[Case]) {
        let original = self.vector();
        for case in corpus {
            if case.expect != Expect::Accept {
                assert_ne!(
                    case.bytes, original,
                    "{} did not mutate the vector",
                    case.name
                );
            }
        }
    }

    /// Exhaustive single-character substitutions in pattern-checked string
    /// members. Where the schema states the decoder's whole rule (`exact`),
    /// the verdicts must be equal. Otherwise the schema must accept whatever
    /// the decoder accepts. Returns the number of documents checked.
    pub(crate) fn sweep(&self, fields: &[(&[&str], bool)]) -> usize {
        let replacements: Vec<char> = (0u8..=0x7f)
            .map(char::from)
            .chain(['\u{e9}', '\u{0662}', '\u{ff10}', '\u{1f600}'])
            .collect();
        let mut unsound = Vec::new();
        let mut inexact = Vec::new();
        let mut checked = 0usize;
        for (path, exact) in fields {
            let chars: Vec<char> = self.string_at(path).chars().collect();
            for position in 0..chars.len() {
                for &replacement in &replacements {
                    if replacement == chars[position] {
                        continue;
                    }
                    let mut mutated = chars.clone();
                    mutated[position] = replacement;
                    let value: String = mutated.into_iter().collect();
                    let bytes = self.with_at(path, json!(value));
                    let decoder = decode_document(&bytes).is_ok();
                    let schema = self.schema_accepts(&bytes);
                    checked += 1;
                    if decoder && !schema {
                        unsound.push(format!("{}={value:?}", path_name(path)));
                    } else if *exact && decoder != schema {
                        inexact.push(format!(
                            "{}={value:?} decoder {decoder} schema {schema}",
                            path_name(path)
                        ));
                    }
                }
            }
        }
        assert!(
            unsound.is_empty(),
            "{}: schema rejects decoder-accepted values:\n{}",
            self.name,
            unsound.join("\n")
        );
        assert!(
            inexact.is_empty(),
            "{}: schema and decoder disagree:\n{}",
            self.name,
            inexact.join("\n")
        );
        checked
    }
}

fn check_object_members(
    root: &Value,
    schema: &Value,
    object: &Map<String, Value>,
    optional: &[&str],
    label: &str,
) {
    let schema = resolve(root, schema);
    let members: BTreeSet<String> = object.keys().cloned().collect();
    let properties: BTreeSet<String> = schema["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("{label} has no properties"))
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        properties, members,
        "{label}: schema properties vs typed members"
    );
    let required: BTreeSet<String> = schema["required"]
        .as_array()
        .unwrap_or_else(|| panic!("{label} has no required list"))
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    let expected: BTreeSet<String> = members
        .iter()
        .filter(|m| !optional.contains(&m.as_str()))
        .cloned()
        .collect();
    assert_eq!(required, expected, "{label}: required members");
    assert_eq!(
        schema["additionalProperties"],
        json!(false),
        "{label}: unknown members must be refused"
    );
}

/// Follows a local `$ref`, and the one non-null branch of a nullable
/// `anyOf`, to the subschema that describes the value itself.
pub(crate) fn resolve<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let name = reference
            .strip_prefix("#/$defs/")
            .unwrap_or_else(|| panic!("non-local $ref {reference}"));
        return resolve(root, &root["$defs"][name]);
    }
    if let Some(branches) = schema.get("anyOf").and_then(Value::as_array) {
        let non_null: Vec<&Value> = branches
            .iter()
            .filter(|b| b.get("type") != Some(&json!("null")))
            .collect();
        if let [only] = non_null.as_slice() {
            return resolve(root, only);
        }
    }
    schema
}

/// The subschema for the member at `path`, through `properties`.
pub(crate) fn subschema<'a>(root: &'a Value, path: &[&str]) -> &'a Value {
    let mut current = root;
    for segment in path {
        current = &resolve(root, current)["properties"][*segment];
        assert!(!current.is_null(), "schema has no member {segment}");
    }
    current
}
