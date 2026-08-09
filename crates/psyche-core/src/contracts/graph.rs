//! Graph and node contracts.
#![allow(missing_docs)]

use serde::{Deserialize, Serialize};

use crate::contracts::{
    ContractError, RecordKind, SchemaKind, SchemaVersion, VersionedRecord, bounded, require_id,
    require_schema, safe_integer, string_list,
};
use crate::id::RecordId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphState {
    Draft,
    Admitted,
    Rejected,
    Running,
    WaitingApproval,
    WaitingEvidence,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
    RecoveryRequired,
}

validated_struct! {
    pub struct Graph, GraphWire {
        pub schema_version: SchemaVersion,
        pub graph_id: RecordId,
        pub root_intent_id: RecordId,
        pub owner_principal_id: String,
        pub policy_revision: String,
        pub state: GraphState,
        pub version: u64,
    }
}

impl Graph {
    pub fn validate(&self) -> Result<(), ContractError> {
        let schema = SchemaKind::Graph;
        require_schema(self.schema_version, schema)?;
        require_id(&self.graph_id, RecordKind::Graph, schema, "graph_id")?;
        require_id(
            &self.root_intent_id,
            RecordKind::Intent,
            schema,
            "root_intent_id",
        )?;
        bounded(&self.owner_principal_id, 255, schema, "owner_principal_id")?;
        bounded(&self.policy_revision, 255, schema, "policy_revision")?;
        if self.version == 0 {
            return Err(super::invalid(schema, "version"));
        }
        safe_integer(self.version, schema, "version")?;
        Ok(())
    }
}

impl VersionedRecord for Graph {
    fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }
    fn record_id(&self) -> &RecordId {
        &self.graph_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    Proposed,
    Admitted,
    Rejected,
    Blocked,
    Ready,
    Skipped,
    Reserved,
    Dispatching,
    Adopted,
    AdoptionUnknown,
    ProvenNotAdopted,
    Failed,
    Running,
    WaitingApproval,
    Candidate,
    AwaitingVerification,
    Verified,
    EscalationRequired,
    Cancelling,
    Cancelled,
    TerminationUnknown,
    RecoveryRequired,
}

validated_struct! {
    pub struct GraphNode, GraphNodeWire {
        pub schema_version: SchemaVersion,
        pub node_id: RecordId,
        pub graph_id: RecordId,
        pub familiar_snapshot_id: RecordId,
        pub dependencies: Vec<RecordId>,
        pub delegation_id: Option<RecordId>,
        pub budget_id: RecordId,
        pub required_evidence: Vec<String>,
        pub state: NodeState,
        pub version: u64,
    }
}

impl GraphNode {
    pub fn validate(&self) -> Result<(), ContractError> {
        let schema = SchemaKind::GraphNode;
        require_schema(self.schema_version, schema)?;
        require_id(&self.node_id, RecordKind::GraphNode, schema, "node_id")?;
        require_id(&self.graph_id, RecordKind::Graph, schema, "graph_id")?;
        require_id(
            &self.familiar_snapshot_id,
            RecordKind::IdentitySnapshot,
            schema,
            "familiar_snapshot_id",
        )?;
        self.dependencies
            .iter()
            .try_for_each(|id| require_id(id, RecordKind::GraphNode, schema, "dependencies"))?;
        if let Some(id) = &self.delegation_id {
            require_id(id, RecordKind::Delegation, schema, "delegation_id")?;
        }
        require_id(&self.budget_id, RecordKind::Budget, schema, "budget_id")?;
        string_list(&self.required_evidence, schema, "required_evidence")?;
        if self.version == 0 {
            return Err(super::invalid(schema, "version"));
        }
        safe_integer(self.version, schema, "version")?;
        Ok(())
    }
}

impl VersionedRecord for GraphNode {
    fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }
    fn record_id(&self) -> &RecordId {
        &self.node_id
    }
}
