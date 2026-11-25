# Security policy

Psyche defines identity, authority, persistence, execution, receipt, cancellation, and recovery contracts. Security reports are treated as protocol-impacting until triage proves otherwise.

## Supported versions

Psyche is currently pre-1.0. Only the current `main` contract foundation and explicitly identified release artifacts receive active security fixes. No unreleased consumer profile should be inferred to be supported.

Supported versions and future maintenance windows will be listed here when the first protocol release candidate is published.

## Reporting a vulnerability

Choose **Report a vulnerability** on Psyche’s Security page or open the private reporting form directly. Sign in to GitHub to continue. If the Psyche form is unavailable, use the OpenCoven/coven private advisory intake and name OpenCoven/psyche as the affected repository. List each affected repository for cross-project reports. [Psyche private reporting form](https://github.com/OpenCoven/psyche/security/advisories/new) · [OpenCoven private advisory intake](https://github.com/OpenCoven/coven/security/advisories/new) · [Organization reporting policy](https://github.com/OpenCoven/.github/blob/main/SECURITY.md)

Use public issues only for ordinary support. Keep security reports inside the private advisory. Use synthetic examples and omit real credentials, prompts, user data and private repository paths.

## Include privately

- affected version, commit, or artifact digest;
- affected identity/authority/persistence boundary;
- minimal reproduction;
- expected and observed behavior;
- whether exploitation crosses a trust, project, subject, device, or runtime boundary;
- data-loss, duplicate-effect, authority-widening, or recovery impact;
- suggested mitigation, if known.

## Never post publicly

- tokens, passwords, private keys, certificates, signing material, or credentials;
- raw prompts or unrestricted terminal output;
- private repository contents;
- full environment-variable dumps;
- private service URLs or infrastructure details;
- unnecessary full filesystem paths or personal data;
- exploit code before coordinated disclosure.

## Security-sensitive surfaces

Changes to these areas require focused review and exact-head evidence:

- canonical IDs, records, enums, serialization, and digests;
- structured denials and version negotiation;
- durable store, migrations, quarantine, retention, and crash recovery;
- capability, adoption, dispatch, fencing, approval, receipt, cancellation, and recovery;
- Coven and surface adapters;
- evidence allowlists and verification scripts;
- CI, dependencies, secret scanning, packaging, release, and provenance.

## Response and disclosure

Maintainers will acknowledge a private report, assess severity and affected versions, coordinate a fix and downstream canaries, and agree on disclosure timing. Timelines depend on impact and the number of affected consumers; uncertainty will be stated rather than hidden.

Do not publish before the fix, migration/rollback guidance, and affected-consumer plan are ready unless active exploitation or user safety requires a different coordinated response.
