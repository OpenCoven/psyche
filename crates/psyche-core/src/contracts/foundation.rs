//! Minimum policy-free foundation records.
#![allow(missing_docs)]

use crate::contracts::{
    ContractError, RecordKind, SchemaKind, SchemaVersion, VersionedRecord, bounded,
    optional_bounded, require_id, require_schema, string_list,
};
use crate::digest::Sha256Digest;
use crate::id::RecordId;

macro_rules! versioned {
    ($ty:ty, $id:ident) => {
        impl VersionedRecord for $ty {
            fn schema_version(&self) -> SchemaVersion {
                self.schema_version
            }
            fn record_id(&self) -> &RecordId {
                &self.$id
            }
        }
    };
}

validated_struct! {
    pub struct Delegation, DelegationWire {
        pub schema_version: SchemaVersion,
        pub delegation_id: RecordId,
        pub parent_node_id: RecordId,
        pub child_node_id: RecordId,
        pub scope_digest: Sha256Digest,
        pub budget_id: RecordId,
        pub evidence_scope_digest: Sha256Digest,
        pub cancellation_policy: String,
    }
}

impl Delegation {
    pub fn validate(&self) -> Result<(), ContractError> {
        let s = SchemaKind::Delegation;
        require_schema(self.schema_version, s)?;
        require_id(
            &self.delegation_id,
            RecordKind::Delegation,
            s,
            "delegation_id",
        )?;
        require_id(
            &self.parent_node_id,
            RecordKind::GraphNode,
            s,
            "parent_node_id",
        )?;
        require_id(
            &self.child_node_id,
            RecordKind::GraphNode,
            s,
            "child_node_id",
        )?;
        require_id(&self.budget_id, RecordKind::Budget, s, "budget_id")?;
        bounded(&self.cancellation_policy, 256, s, "cancellation_policy")
    }
}
versioned!(Delegation, delegation_id);

validated_struct! {
    pub struct Budget, BudgetWire {
        pub schema_version: SchemaVersion,
        pub budget_id: RecordId,
        pub graph_id: RecordId,
        pub resource_class: String,
        pub limit: u64,
        pub reserved: u64,
        pub consumed: u64,
        pub released: u64,
    }
}

impl Budget {
    pub fn validate(&self) -> Result<(), ContractError> {
        let s = SchemaKind::Budget;
        require_schema(self.schema_version, s)?;
        require_id(&self.budget_id, RecordKind::Budget, s, "budget_id")?;
        require_id(&self.graph_id, RecordKind::Graph, s, "graph_id")?;
        bounded(&self.resource_class, 256, s, "resource_class")
    }
}
versioned!(Budget, budget_id);

validated_struct! {
    pub struct Approval, ApprovalWire {
        pub schema_version: SchemaVersion,
        pub approval_id: RecordId,
        pub node_id: RecordId,
        pub requester_principal_id: String,
        pub decision: Option<String>,
        #[serde(with = "time::serde::rfc3339")]
        pub expires_at: time::OffsetDateTime,
    }
}

impl Approval {
    pub fn validate(&self) -> Result<(), ContractError> {
        let s = SchemaKind::Approval;
        require_schema(self.schema_version, s)?;
        require_id(&self.approval_id, RecordKind::Approval, s, "approval_id")?;
        require_id(&self.node_id, RecordKind::GraphNode, s, "node_id")?;
        bounded(
            &self.requester_principal_id,
            255,
            s,
            "requester_principal_id",
        )?;
        optional_bounded(&self.decision, 256, s, "decision")?;
        Ok(())
    }
}
versioned!(Approval, approval_id);

validated_struct! {
    pub struct Evidence, EvidenceWire {
        pub schema_version: SchemaVersion,
        pub evidence_id: RecordId,
        pub node_id: RecordId,
        pub attempt_id: RecordId,
        pub content_digest: Sha256Digest,
        pub producer: String,
        pub collection_method: String,
        pub media_type: String,
        pub size: u64,
        #[serde(with = "time::serde::rfc3339")]
        pub created_at: time::OffsetDateTime,
        pub retention_policy: String,
    }
}

impl Evidence {
    pub fn validate(&self) -> Result<(), ContractError> {
        let s = SchemaKind::Evidence;
        require_schema(self.schema_version, s)?;
        require_id(&self.evidence_id, RecordKind::Evidence, s, "evidence_id")?;
        require_id(&self.node_id, RecordKind::GraphNode, s, "node_id")?;
        require_id(&self.attempt_id, RecordKind::Attempt, s, "attempt_id")?;
        for (value, field) in [
            (&self.producer, "producer"),
            (&self.collection_method, "collection_method"),
            (&self.media_type, "media_type"),
            (&self.retention_policy, "retention_policy"),
        ] {
            bounded(value, 256, s, field)?;
        }
        Ok(())
    }
}
versioned!(Evidence, evidence_id);

validated_struct! {
    pub struct Verdict, VerdictWire {
        pub schema_version: SchemaVersion,
        pub verdict_id: RecordId,
        pub node_id: RecordId,
        pub sealed_evidence_digest: Sha256Digest,
        pub policy_revision: String,
        pub verdict_type: String,
        pub reviewer_id: String,
        pub outcome: String,
        pub reason_codes: Vec<String>,
        #[serde(with = "time::serde::rfc3339")]
        pub created_at: time::OffsetDateTime,
    }
}

impl Verdict {
    pub fn validate(&self) -> Result<(), ContractError> {
        let s = SchemaKind::Verdict;
        require_schema(self.schema_version, s)?;
        require_id(&self.verdict_id, RecordKind::Verdict, s, "verdict_id")?;
        require_id(&self.node_id, RecordKind::GraphNode, s, "node_id")?;
        for (value, field) in [
            (&self.policy_revision, "policy_revision"),
            (&self.verdict_type, "verdict_type"),
            (&self.reviewer_id, "reviewer_id"),
            (&self.outcome, "outcome"),
        ] {
            bounded(value, 256, s, field)?;
        }
        string_list(&self.reason_codes, s, "reason_codes")?;
        Ok(())
    }
}
versioned!(Verdict, verdict_id);

validated_struct! {
    pub struct Recovery, RecoveryWire {
        pub schema_version: SchemaVersion,
        pub recovery_id: RecordId,
        pub attempt_id: RecordId,
        pub lease_id: String,
        pub fence_token: Option<String>,
        pub ambiguity: String,
        pub reconciliation_count: u64,
        pub operator_disposition: Option<String>,
    }
}

impl Recovery {
    pub fn validate(&self) -> Result<(), ContractError> {
        let s = SchemaKind::Recovery;
        require_schema(self.schema_version, s)?;
        require_id(&self.recovery_id, RecordKind::Recovery, s, "recovery_id")?;
        require_id(&self.attempt_id, RecordKind::Attempt, s, "attempt_id")?;
        bounded(&self.lease_id, 255, s, "lease_id")?;
        bounded(&self.ambiguity, 256, s, "ambiguity")?;
        optional_bounded(&self.fence_token, 255, s, "fence_token")?;
        optional_bounded(&self.operator_disposition, 256, s, "operator_disposition")
    }
}
versioned!(Recovery, recovery_id);

validated_struct! {
    pub struct Addon, AddonWire {
        pub schema_version: SchemaVersion,
        pub addon_id: RecordId,
        pub package: String,
        pub version: String,
        pub package_digest: Sha256Digest,
        pub provenance_digest: Sha256Digest,
        pub contributions_digest: Sha256Digest,
        pub allowlist_digest: Sha256Digest,
        pub revocation_state: String,
    }
}

impl Addon {
    pub fn validate(&self) -> Result<(), ContractError> {
        let s = SchemaKind::Addon;
        require_schema(self.schema_version, s)?;
        require_id(&self.addon_id, RecordKind::Addon, s, "addon_id")?;
        for (value, field) in [
            (&self.package, "package"),
            (&self.version, "version"),
            (&self.revocation_state, "revocation_state"),
        ] {
            bounded(value, 256, s, field)?;
        }
        Ok(())
    }
}
versioned!(Addon, addon_id);
