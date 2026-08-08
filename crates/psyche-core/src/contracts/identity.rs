//! Identity snapshot contract.
#![allow(missing_docs)]

use serde::{Deserialize, Serialize};

use crate::contracts::{
    ContractError, RecordKind, SchemaKind, SchemaVersion, VersionedRecord, bounded, require_id,
    require_schema,
};
use crate::digest::Sha256Digest;
use crate::id::RecordId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityProvenance {
    pub familiar_home_id: String,
    pub resolver_version: String,
}

validated_struct! {
    pub struct IdentitySnapshot, IdentitySnapshotWire {
        pub schema_version: SchemaVersion,
        pub snapshot_id: RecordId,
        pub familiar_id: String,
        pub principal_id: String,
        pub revision: u64,
        pub declaration_digest: Sha256Digest,
        pub identity_file_digest: Sha256Digest,
        pub identity_digest: Sha256Digest,
        pub soul_digest: Sha256Digest,
        pub role_skill_digest: Sha256Digest,
        pub provenance: IdentityProvenance,
        #[serde(with = "time::serde::rfc3339")]
        pub resolved_at: time::OffsetDateTime,
    }
}

impl IdentitySnapshot {
    pub fn validate(&self) -> Result<(), ContractError> {
        let schema = SchemaKind::IdentitySnapshot;
        require_schema(self.schema_version, schema)?;
        require_id(
            &self.snapshot_id,
            RecordKind::IdentitySnapshot,
            schema,
            "snapshot_id",
        )?;
        bounded(&self.familiar_id, 255, schema, "familiar_id")?;
        bounded(&self.principal_id, 255, schema, "principal_id")?;
        if self.revision == 0 {
            return Err(super::invalid(schema, "revision"));
        }
        bounded(
            &self.provenance.familiar_home_id,
            255,
            schema,
            "provenance.familiar_home_id",
        )?;
        bounded(
            &self.provenance.resolver_version,
            255,
            schema,
            "provenance.resolver_version",
        )?;
        Ok(())
    }
}

impl VersionedRecord for IdentitySnapshot {
    fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    fn record_id(&self) -> &RecordId {
        &self.snapshot_id
    }
}
