//! Intent contract.
#![allow(missing_docs)]

use std::collections::BTreeMap;

use serde_json::Value;

use crate::contracts::{
    ContractError, MAX_DOCUMENT_BYTES, RecordKind, SchemaKind, SchemaVersion, VersionedRecord,
    bounded, require_id, require_schema, string_list, timestamp,
};
use crate::digest::Sha256Digest;
use crate::id::RecordId;

validated_struct! {
    pub struct Intent, IntentWire {
        pub schema_version: SchemaVersion,
        pub intent_id: RecordId,
        pub principal_id: String,
        pub familiar_snapshot_id: RecordId,
        pub project_id: String,
        pub requested_outcome: String,
        pub constraints: BTreeMap<String, Value>,
        pub required_evidence: Vec<String>,
        pub surface_event_id: Option<RecordId>,
        pub created_at: String,
        pub digest: Sha256Digest,
    }
}

impl Intent {
    pub fn validate(&self) -> Result<(), ContractError> {
        let schema = SchemaKind::Intent;
        require_schema(self.schema_version, schema)?;
        require_id(&self.intent_id, RecordKind::Intent, schema, "intent_id")?;
        require_id(
            &self.familiar_snapshot_id,
            RecordKind::IdentitySnapshot,
            schema,
            "familiar_snapshot_id",
        )?;
        if let Some(id) = &self.surface_event_id {
            require_id(id, RecordKind::SurfaceEvent, schema, "surface_event_id")?;
        }
        bounded(&self.principal_id, 255, schema, "principal_id")?;
        bounded(&self.project_id, 255, schema, "project_id")?;
        bounded(&self.requested_outcome, 16_384, schema, "requested_outcome")?;
        string_list(&self.required_evidence, schema, "required_evidence")?;
        for key in self.constraints.keys() {
            bounded(key, 256, schema, "constraints")?;
        }
        if crate::digest::canonical_bytes(&self.constraints)?.len() > MAX_DOCUMENT_BYTES {
            return Err(super::invalid(schema, "constraints"));
        }
        timestamp(&self.created_at, schema, "created_at")?;
        Ok(())
    }
}

impl VersionedRecord for Intent {
    fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    fn record_id(&self) -> &RecordId {
        &self.intent_id
    }
}
