# First-profile publication surface

Selection for #11 of what the first Build profile would publish, made from the rows the owner approved on #12 on 23 November 2026. It maps each approved row to the record kinds that carry it, gives every kind a class, says what rows 5–7 still lack, lists what is excluded, and sets out the compatibility range and rollback steps under the approved policy. It is a selection and publishes nothing. No kind is stable on main, no artifact, tag or release exists for this profile, and live Build activation is a separate decision. [Publication issue](https://github.com/OpenCoven/psyche/issues/11) · [Owner decision](https://github.com/OpenCoven/psyche/issues/12#issuecomment-7117320214)

## Basis

| Input | Read at | Used for |
|---|---|---|
| [Owner decision on #12](https://github.com/OpenCoven/psyche/issues/12#issuecomment-7117320214) | Recorded 23:44 UTC on 23 November 2026 | ADR 1–4, the row dispositions and POLICY. Its scope is the first Build profile only. |
| [Ownership decision packet, revision 3](https://github.com/OpenCoven/psyche/blob/4b40fbf6090a85d2a8e57b0cbf0d626cbddd29f0/docs/protocol/OWNERSHIP-DECISION-PACKET.md) | `4b40fbf6090a85d2a8e57b0cbf0d626cbddd29f0`, the revision the decision names | The numbered producer and consumer matrix and the five policy steps. |
| [Publication inventory](https://github.com/OpenCoven/psyche/blob/1985f24b92314c10b673e85204da71f4abecac05/docs/protocol/PUBLICATION-INVENTORY.md), draft PR #28 | Head `1985f24b92314c10b673e85204da71f4abecac05` | Kinds, prefixes, existing fixtures and per-kind obligations. |
| [Canonical vectors](https://github.com/OpenCoven/psyche/blob/a5f69492b4c4d062f8c83bd7b4e6c1157dd047f5/crates/psyche-core/tests/fixtures/vectors/README.md), draft PR #50 | Head `a5f69492b4c4d062f8c83bd7b4e6c1157dd047f5` | One synthetic canonical document and SHA-256 for each of the sixteen kinds. |
| Psyche main | `6158869fed160739489e042b67df379aa1f7c01f` | Field lists, re-read from source. |

The inventory surveyed main `ef67c1f`. `git diff --stat ef67c1f 6158869 -- crates profiles` is empty, so its source references hold at this base. PRs #28 and #50 are unmerged drafts as of 00:30 UTC on 24 November 2026. This document cites them at the heads above and needs neither to merge. If either head moves, re-read the mapping against it.

The decision rules on rows, and the packet's matrix names facts and not kinds. The assignment of kinds to rows below is this document's reading of the fields each kind carries.

## Classes and the selection rule

The classes are the ones #11 asks for. `deprecated` is unused because nothing has been published.

| Class | Meaning here |
|---|---|
| `stable-v1` | A target for the first profile's stable set. The kind stays experimental on main until a release ships its schema, pinned vector and rejection vectors under protected-owner review. |
| `experimental` | Shipped in the profile labelled experimental. It carries no compatibility promise and a consumer must not treat its shape as frozen. |
| `internal` | Not published by Psyche. A store, harness or product-local detail. |

A kind owned by a held row is marked excluded. It is not in the surface and stays experimental on main.

A kind is a `stable-v1` target only when all three hold.

1. Psyche is the approved producer of the row that owns the kind.
2. That row is not waiting on a released representation (rows 5–7) and its policy owners are not deferred (row 15).
3. Every record the kind references by ID is also a `stable-v1` target, and none of its fields carries a fact produced by a held or deferred row.

The rule is this document's proposal. The decision approves producers and leaves selection to #11. It is narrow on purpose: G2 records deny unknown fields, so a kind frozen now cannot gain a field later without a negotiated revision or a new wire shape. [Schema notes](../SCHEMAS.md)

## Approved rows

Rows 1, 2, 8, 9 and 16 have producers other than Psyche. Psyche publishes no record for them. The profile pins each producer's revision, and the table names the Psyche kinds that carry a reference to their facts.

| Row | Fact and approved producer | Record kinds | Class |
|---|---|---|---|
| 1 | Familiar identity, declaration revision and person binding. Familiar Contract. | No Psyche kind. `identity_snapshot` snapshots it, and `intent`, `graph_node`, `execution_binding` and `surface_effect` pin that snapshot by ID. | Not Psyche's to publish. |
| 2 | Protected surface action and destination authorization. Coven Threads. | No Psyche kind. `delivery.surface_decision` carries the decision, and `surface_effect` carries the action class and effect digest it covers. Both kinds are in held row 13. | Not Psyche's to publish. No selected kind carries it. |
| 4 | Identity snapshot, intent and surface event. Psyche. Rebind is excluded. | `identity_snapshot`, `intent`, `surface_event` | `stable-v1` target |
| 5 | Graph, node, task, lane, attempt, delegation and budget. Psyche. | `graph`, `graph_node`, `execution_binding` (stored as the Attempt record), `delegation`, `budget`. No kind exists for task or lane. | `experimental` |
| 6 | Lease, grant and revocation correlation. Psyche. | None. | Nothing to classify. |
| 7 | Approval request, lifecycle and response correlation. Psyche. | `approval` | `experimental` |
| 8 | Decision granting an effect on a protected runtime resource. Coven. | No Psyche kind. Psyche presents `psyche.execution_request.v1`, which is outside the registry. | Not Psyche's to publish. The request is `experimental`. |
| 9 | Session, request admission, adoption, event cursor and physical terminal fact. Coven. | No Psyche kind. `execution_binding` records them in `coven_contract_version`, `coven_session_id`, `adoption_state`, `event_cursor` and `terminal_state`. | Not Psyche's to publish. The carrier is `experimental` under row 5. |
| 15 | Evidence, sealed verdict, addon, cancellation and recovery orchestration, and the graph terminal record. Psyche. | `evidence`, `verdict`, `addon`, `recovery`. Cancellation has no kind of its own; it is `execution_binding.cancellation_state` and the cancelling and cancelled graph and node states. The terminal record is the `graph` kind. | `experimental` |
| 16 | Host-local pane, worktree, branch, terminal attachment, product lease generation and UI selection. Psyche Build. | No Psyche kind, and none is selected. A local ID may reference a Psyche ID and never replaces one. | `internal` to Build |

Approving rows 1, 2, 8 and 9 extends the Telegram-scoped record to the first profile. Row 9 follows the Telegram assignment, which governs where the proposed baseline contradicts itself. Row 3, the Telegram transport component, is not in the decision's lists, and the packet places it outside the Build profile. It is not in this surface.

## Kinds

The vector column is the SHA-256 of the file bytes at the #50 head, recomputed for this document and equal to that head's pinned literal for all sixteen kinds. The vectors are synthetic and experimental. Their IDs do not resolve across files and most digests inside them are placeholders, so the column records what exists and is not a pin.

| Kind | Prefix | Row | Class | Reason, and what a release still needs | #50 vector SHA-256 |
|---|---|---|---|---|---|
| identity_snapshot | `ids_` | 4 | `stable-v1` target | References no other record. Needs a mismatched-binding rejection and the pinned Familiar Contract revision that row 1 supplies. | `1da8dfa1d333edef27cbb3e1e2c384951d32299bda8d75f5712f690f07dde619` |
| intent | `int_` | 4 | `stable-v1` target | References only `identity_snapshot` and `surface_event`. Needs a round trip of the existing fixture and a published unknown-field rejection. | `8ef6700f6d72a7666b57cc051342bc04491c47782f73950cc60fb2666071f5ed` |
| surface_event | `sev_` | 4 | `stable-v1` target | References no other record. Needs published malformed-actor, locator and oversized-content rejections. This document found no row naming the adapter that supplies events in the Build profile. Rule 3 tests held and deferred rows and does not catch a fact with no row, so naming that adapter is a release condition for this kind and for `intent`, which references it. | `c458776df557249dbfba1e3d9e876e5f22f1353a0a1d2488bd822777d77761d2` |
| graph | `grf_` | 5, 15 | `experimental` | Task has no representation and may map here. No transition policy is published. No test on main decodes a graph document successfully. | `fd10742cb0633589ed8e150a8b865bebcf7115ac96502ca097a5a39e28a54e93` |
| graph_node | `nod_` | 5 | `experimental` | Lane has no representation and may map here. References `delegation` and `budget`. Its `termination_unknown` state names a result that deferred row 12 produces. | `e54a359eaa131b62da6ef743ae45be5a0bc9e203ce13b607c1cb9ccc209bf1cf` |
| execution_binding | `att_` | 5, carrying 9 | `experimental` | Carries the `fenced` adoption state (held row 11) and the cancellation acknowledgement and unresolved evidence (deferred row 12). Real-daemon qualification is open on Coven #1230. | `1a3496c1483e958f399ba2fc23386396c1b8fa6e38d6a1538307843b2844b705` |
| delegation | `dlg_` | 5 | `experimental` | References `graph_node` and `budget`. Needs narrowed-scope and widening vectors. | `0893d64053a40fbd6d7ac09c36719a801a4967ba70d376c83efccf6b84c396f7` |
| budget | `bud_` | 5 | `experimental` | Budget policy has no owner yet. The decoder accepts `consumed` above `limit`. Needs an enforceable-limit case and a widened-reservation rejection. | `f69fac29e8176690c0c4998649d92248a9a5f3e84ba510f6d02d2986acbf48be` |
| approval | `apr_` | 7 | `experimental` | See row 7 below. Approval policy has no owner yet. | `32913ed310cd15bf99796c2ef780334c555303001ccc328a28780e134006665f` |
| evidence | `evd_` | 15 | `experimental` | Evidence policy has no owner yet. References `graph_node` and the Attempt record. Needs a content and correlation mismatch pair. | `4e16da1b930ac3632a48139d9ab93e803adcb603d742736ae45c5e86263955b0` |
| verdict | `vrd_` | 15 | `experimental` | Verdict policy has no owner yet. Needs sealed-evidence mismatch and authority cases. | `6f468f27a6705b1f5dc69193d276d6c796ed7c72e879ed6f41aa4750e7a784cb` |
| recovery | `rcv_` | 15 | `experimental` | Recovery policy has no owner yet. Carries `fence_token` (held row 11) and `lease_id` (row 6, no representation). | `cc34e94d665cb945d2af6404675c06266e6892d5ff60e0cb76c106ea068c810d` |
| addon | `adn_` | 15 | `experimental` | Addon policy has no owner yet. No test on main builds, decodes or stores an addon document. | `73a3d18b2dceb2e307ef5038e1466493b46fba4962962ceac3c8e7b1ed57df2f` |
| error | none | none | `experimental` | No matrix row names the denial envelope, so no producer approval covers it. It may serve as the serialized rejection result, which is undecided (slice 3). Never stored. | `9b33c0f2d972ce9496ff2c4d02938712572ac68b87eb2a54c6d375b5a3035f97` |
| surface_effect | `sfx_` | 13 | excluded | Row 13 is held. | not selected |
| delivery | `del_` | 13 | excluded | Row 13 is held. | not selected |

Three kinds are `stable-v1` targets, eleven are `experimental` and two are excluded. The three targets still share these release conditions.

- A schema document. None exists, and no choice between JSON Schema and an equivalent is recorded.
- A merged pinned vector. The #50 vectors are unmerged and labelled as not for downstream pinning.
- Rejection vectors with a serialized expected result. Decode rejections are Rust enums with no wire form today.
- A disposition for each decoder behaviour reported on #11: intended and pinned by a vector, or a defect fixed first. Several items reach these three kinds. A timestamp offset survives into canonical bytes and changes the digest, and the report says only one field on another kind enforces UTC, which leaves `resolved_at`, `created_at` and `received_at` unrestricted. Fractional seconds are trimmed and a leap second is rewritten. An absent optional field digests the same as an explicit null (`intent.surface_event_id`). The decoder does not verify `intent.digest` or the five identity digests. A typed integer accepts `-0`, which reaches `identity_snapshot.revision`. Numbers in free-form values are normalized, and `intent.constraints` and the surface event's actor, locator and content are free-form. The error-vocabulary items apply to every kind. [Decoder report](https://github.com/OpenCoven/psyche/issues/11#issuecomment-7078559204)
- The historical obligations. Under ADR 1 and ADR 2 every one stays open, and the inventory records that which kinds the historical specification constrains is not established. That is unresolved for these three kinds as well.
- Owner review of this selection, then the separate stable-publication decision.

Membership of each enum is fixed by strict decoding. No transition between states is selected: the inventory's vocabularies stay experimental until each transition's authority and durable evidence are reviewed.

## Rows 5–7: missing released representation

The decision approves the producer for these rows and says they still wait on a released representation from #11. Strict fields mean that closing any gap below by adding a field to an existing kind needs a negotiated revision or a new wire shape.

**Row 5, task and lane.** The registry has kinds for graph, node, attempt, delegation and budget, and none for task or lane. No crate source defines a task or lane identifier. Missing:

- a decision on whether task and lane are new kinds with their own prefixes or named projections of existing kinds, and which ones;
- how Build's native task and lane IDs relate to Psyche IDs, as opaque references that never replace them;
- for `graph` and `budget`, a first test on main that decodes a document successfully through `decode_document`, and for `graph`, `execution_binding`, `delegation` and `budget`, a fixture file;
- portable stale-correlation, ambiguity, crash and restart cases for `execution_binding`, with the fence cases waiting on row 11;
- a published transition policy for the graph and node states.

**Row 6, lease.** Nothing is released and nothing exists to release. The only lease field on main is `recovery.lease_id`, a bounded string. Missing:

- a record kind and prefix, or a defined projection;
- the fields row 6 says Build validates: actor, revisions, epoch, exact capability, scope and expiry;
- grant and revocation correlation, and the issuance, expiry, revocation and renewal lifecycle;
- a rejection for a stale or wider credential after reconnect.

Build's product lease generation stays product-local under row 16.

**Row 7, approval.** `psyche.approval.v1` carries an approval ID, the node, the requester, an optional free-string decision and an expiry. Row 7 has consumers persist the exact target and action, digest, scope, expiry, consume-once state and outcome. Missing:

- fields for the action, the digest of what was approved, the scope, the responder and consumption, or a recorded reason they live elsewhere;
- a closed vocabulary for `decision`, which is a bounded string today;
- consume-once and stale-authority cases, and a decoded positive with a decision on main (the #50 vector supplies one, unmerged);
- the binding between Build's native approval ID and `apr_`. The experimental kit keeps that binding in Build.

A Psyche approval record never replaces the decision of the owning authority.

## Outside the registry

The inventory asks a released profile to include or exclude the execution request, the result bundle, `req_`, `qua_`, and the configuration and CLI schemas. The other rows are added here so that each has a stated disposition.

| Name | Disposition | Reason |
|---|---|---|
| `psyche.execution_request.v1` and the `req_` request ID | Included, `experimental` | Rows 8 and 9 consume the sealed request, and the ADR 4 binding refers to its ID and digest. Main pins a launch golden and an input golden byte for byte. `decode_document` rejects the schema string as unknown. |
| Result bundle | Excluded | Row 14 is deferred. |
| `psyche.build_receipt_binding.v1`, as proposed in unmerged PR #59 | Not selected here | Row 13 is held. Its wire format and verifier belong to the ADR 4 slice in PR #59. |
| `psyche.experimental.receipt-replay.v1` profile | Unchanged, `experimental` | The decision keeps the comparator available for experimental consumers and rollback. |
| Port capability names and disposition enums | Not published | Most have no serialized form, and capability descriptors are deferred row 10. |
| `qua_`, store transitions, quarantine resolution states, `ContractError`, `StoreError`, fixture enums | `internal` | Store and harness detail. |
| `psyche.config.v1`, `psyche.status.v1`, `psyche.doctor.v1` | Outside the consumer profile | Operator interfaces. This selection does not classify them. |

## Excluded

| Row | Disposition | What the surface leaves out or makes no claim about |
|---|---|---|
| 4, rebind only | Excluded by the decision | Any rebind record, action or route, including `coven.familiar.identityRebind.v1`. A snapshot is an exact pin. Under ADR 1 an identity change keeps a route blocked until that route's old-to-current mapping row has executable evidence. Under ADR 2 the specification at `9b07c500` is unavailable, and the Coven #1230 acceptance text and the 20 October matrix are the retained statement. |
| 11 | Held | Process-attempt fence issuance, enforcement and coverage. `fence_token` and the `fenced` state stay in their experimental kinds with no published meaning. ADR 3 approves the host-issued disposition for the #19 profile only and returns the in-process enforcer to #19 and Coven #804. |
| 13 | Held | `surface_effect`, `delivery`, action, receipt and idempotency. The row returns for approval once it has evidence under ADR 4. |
| 10 | Deferred | Runtime descriptors, capability and conformance declarations. |
| 12 | Deferred | Termination acknowledgement and unresolved result as published facts. Their evidence structures stay inside the experimental `execution_binding`. |
| 14 | Deferred | Result and blob evidence, artifact byte identity and runtime lifetime. |
| 17–21 | Deferred | Cave interaction, the SDK query profile, the Memory projection, the Docs ledger and Coven automation records. |

## POLICY

The decision approves the packet's policy for one exact pinned profile and leaves the emergency and deprecation steps proposed until they have been exercised. In the packet those are step 5, which leaves steps 1–4 as the approved policy. No profile is pinned yet, so nothing is in force.

### Compatibility range

The range is a single point: one exact profile, and no span of versions.

- **The pin.** Step 3 has Build pin the manifest, artifact digest, signature identity, producer revision, runner version and vector digests. None of these values exists yet. The release slice assigns them, and until then there is nothing a consumer can pin.
- **Inside the pin.** Every schema string is exactly `psyche.<kind>.v1`. One major is supported for every kind, the string has no minor version, and unknown fields are denied. An unknown kind, major or enum value fails closed and is never dispatchable. An added field is therefore a breaking change.
- **Stable set.** The three targets become frozen bytes only at release. After that, changing a vector needs an explicit compatibility decision. [Contribution contract](../../CONTRIBUTING.md)
- **Experimental kinds.** They sit outside the range. The #50 README has them change by regenerating the vector and re-pinning its literal in one change under R4 review.
- **Widening.** The support window starts with one qualified exact profile and widens only after each producer and consumer pair is tested. Compatibility is never inferred from a version number alone.
- **Consumers.** Build is the only downstream client in range. Cave, SDK and Memory are deferred rows, so the profile makes no claim for them.

### Rollback

Step 4, for the consumer:

1. Roll out behind the consumer adapter switch. Keep the old correlation and durable read paths, and use transactional writes or an explicitly reversible migration.
2. To roll back, disable new writes.
3. Restore the previously accepted pin.
4. Reopen the previously migrated records.
5. Verify the exact original IDs, authorities and receipt chain.

A reader that cannot prove compatibility blocks the write and preserves the original bytes. None of these steps permits another effect: a missing or ambiguous outcome stays unknown.

The first profile has no earlier pin. What there is to restore in that case is not stated in the policy. The experimental canary rolled back to the original task, against a synthetic handler and an isolated journal, which qualifies the review branches and not a release. On the Psyche side the only downgrade refusal on main is the store rejecting a newer database version. A document-level downgrade case is not established, and withdrawing a released artifact falls under step 5, which is not approved.

Rolling back this document is reverting its commit. It changes no record, fixture, schema or pin.

## Gaps

1. No schema document exists for any kind (inventory slice 1).
2. Main pins no canonical vector for a registry kind. #50 is unmerged, its vectors are per-kind encode and decode vectors and not a linked data set, and it leaves the six measured fixture digests unpinned.
3. Rejection vectors and a serialized expected-result format are unpublished (slice 3).
4. Rows 5–7 lack a released representation, as listed above. Task, lane and lease have no kind at all.
5. Row 2 is approved, and nothing in the selected surface carries its decision while row 13 is held.
6. The error envelope is not assigned to a row.
7. The decoder behaviours reported on #11 have no recorded disposition.
8. No upstream revision is recorded for Familiar Contract, Threads or Coven, and real-daemon conformance stays with Coven #1230.
9. The profile has no name, manifest, artifact digest, signing identity or runner version. The repository manifest still lists `psyche.protocol.v1` as planned.
10. The standalone runner's interface is undefined (slice 4).
11. The policy does not say what a rollback restores when no earlier pin exists, and step 5 is not approved.
12. The adapter that supplies surface events in the Build profile is not named.
13. #11 lists a minimum consumer profile that includes ambiguity reconciliation and fencing, cancellation acknowledgement and unresolved outcomes, and result and artifact binding. Rows 11, 12 and 14 hold or defer those, and row 10 defers the capability descriptors that negotiation uses. The surface selected here does not meet that minimum.

## Not in this change

Stable publication, a tag, a release, a merge to main and live Build activation. The ADR 4 wire format and verifier in PR #59. The ADR 3 change to redispatch-after-fence test semantics, which goes through its own reviewed PR. The ADR 1 mapping rows on Coven #1230. Any crate, fixture, schema, manifest or checker edit.
