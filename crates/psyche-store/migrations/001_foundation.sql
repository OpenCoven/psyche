CREATE TABLE schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL
) STRICT;
CREATE TABLE canonical_records (
  kind TEXT NOT NULL,
  record_id TEXT NOT NULL,
  schema_version TEXT NOT NULL,
  digest TEXT NOT NULL,
  canonical_json BLOB NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (kind, record_id),
  UNIQUE (kind, record_id, digest)
) STRICT;
CREATE TABLE execution_binding_revisions (
  attempt_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK (revision >= 1),
  schema_version TEXT NOT NULL,
  digest TEXT NOT NULL,
  previous_revision_digest TEXT,
  canonical_json BLOB NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (attempt_id, revision),
  UNIQUE (attempt_id, digest),
  CHECK (
    (revision = 1 AND previous_revision_digest IS NULL)
    OR
    (revision > 1 AND previous_revision_digest IS NOT NULL)
  ),
  FOREIGN KEY (attempt_id, previous_revision_digest)
    REFERENCES execution_binding_revisions(attempt_id, digest)
) STRICT;
CREATE TABLE transitions (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL,
  record_id TEXT NOT NULL,
  from_state TEXT,
  to_state TEXT NOT NULL,
  record_version INTEGER NOT NULL,
  transition_digest TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (kind, record_id, record_version)
) STRICT;
CREATE TABLE quarantine_records (
  quarantine_id TEXT PRIMARY KEY,
  schema_version TEXT,
  payload_digest TEXT NOT NULL,
  original_payload_len INTEGER NOT NULL CHECK (original_payload_len >= 0),
  retained_payload_digest TEXT NOT NULL,
  bounded_payload BLOB NOT NULL,
  reason TEXT NOT NULL,
  discovered_at TEXT NOT NULL,
  resolved_at TEXT,
  resolution_code TEXT,
  resolution_digest TEXT,
  CHECK (
    (resolved_at IS NULL AND resolution_code IS NULL AND resolution_digest IS NULL)
    OR
    (resolved_at IS NOT NULL AND resolution_code IS NOT NULL AND resolution_digest IS NOT NULL)
  )
) STRICT;
CREATE TABLE audit_events (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  event_code TEXT NOT NULL,
  correlation_id TEXT NOT NULL,
  public_details_json BLOB NOT NULL,
  created_at TEXT NOT NULL
) STRICT;
