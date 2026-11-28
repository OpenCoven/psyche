CREATE TABLE receipt_verification_requests_v1 (
  request_id TEXT PRIMARY KEY,
  request BLOB NOT NULL CHECK (length(request) BETWEEN 1 AND 65536),
  request_digest TEXT NOT NULL
) STRICT;
CREATE TABLE receipt_verification_registrations_v1 (
  verification_id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL REFERENCES receipt_verification_requests_v1(request_id),
  context BLOB NOT NULL CHECK (length(context) BETWEEN 1 AND 65536),
  allowed_signers BLOB NOT NULL CHECK (length(allowed_signers) <= 65536),
  registration_digest TEXT NOT NULL,
  head_digest TEXT NOT NULL
) STRICT;
CREATE TABLE receipt_verification_observations_v1 (
  verification_id TEXT NOT NULL REFERENCES receipt_verification_registrations_v1(verification_id),
  sequence INTEGER NOT NULL CHECK (sequence > 0),
  receipt BLOB CHECK (length(receipt) <= 65536),
  binding BLOB CHECK (length(binding) <= 65536),
  signature BLOB CHECK (length(signature) <= 65536),
  verifier_code TEXT NOT NULL,
  effective_code TEXT NOT NULL,
  observation_digest TEXT NOT NULL,
  PRIMARY KEY (verification_id, sequence)
) STRICT;
