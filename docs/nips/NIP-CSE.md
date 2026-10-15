# NIP-CSE — Coding-session receipt error codes

`draft` `optional` `client` `relay`

Every `kind:44224` lifecycle receipt ([NIP-CSL](NIP-CSL.md)) that reports a
failure, a refusal, a drop, a degraded delivery or an unknown delivery carries
`error: {code, message}`. The `code` is a closed, stable vocabulary: a
consumer branches on it, a person reads the `message`. This page is that
vocabulary, in one place, with what each code means and which receipt status
carries it.

**This page is fenced to the code.** Every code below is a
`pub const NAME: &str = "NAME"` in
[`crates/beekeeper-core/src/coding_session_payload.rs`](../../crates/beekeeper-core/src/coding_session_payload.rs),
and the relay's unit test
`nip11::tests::nip_cse_lists_exactly_the_lifecycle_error_codes`
(`crates/beekeeper-relay/src/nip11.rs`) reads that file and this one and fails
when a code exists in only one of them. A code added to the constants without
a row here fails CI; a row here with no constant behind it fails CI. The test
parses rows whose first cell is a backticked `SCREAMING_CASE` token, so keep
the table shape.

One code lives in another crate — the provider's `MODEL_AUTH_UNAVAILABLE`
(`runtime_credentials.rs`) — and is outside the fence until it moves beside
the others; it is listed in a separate, unfenced table at the end so a reader
is not told the fenced table is the whole truth.

## Where a code travels

A `kind:44224` receipt has exactly one `status`. The lifecycle statuses
(`created`, `created_with_failed_initial_turn`, `failed`, `resumed`,
`resumed_without_context`, `stopped`, `continuation_registered`,
`model_applied`) answer one 44221 command with one terminal outcome. The turn
stages (`turn_queued`, `turn_started`, `turn_injected`, `turn_degraded`,
`turn_delivery_unknown`, `turn_dropped`, `turn_refused`,
`interrupt_delivered`) answer one 44220 command *per stage*. A code appears in
`error` only on a status that reports something other than a clean outcome:

| status | meaning of `error.code` |
| --- | --- |
| `failed` | why the lifecycle command (create, resume, restart, rewind, stop, hire-seat create) did not produce its outcome |
| `created_with_failed_initial_turn` | always `INITIAL_TURN_FAILED`: the session exists, the embedded first turn did not run |
| `resumed_without_context` | always `CONTEXT_NOT_RECOVERED`: a new generation opened, the prior context did not come with it |
| `turn_refused` | the turn was not accepted and spent nothing; the sender decides what to do |
| `turn_dropped` | the turn was accepted into the provider but will not run; terminal for this command |
| `turn_degraded` | the turn runs, but not the way the sender asked (steer became a boundary turn, images were dropped) |
| `turn_delivery_unknown` | the provider cannot say whether the input reached the running turn; never replayed automatically |

A code is never the only evidence of an outcome: the same facts are republished
in `kind:44223` metadata (`status`, `turnBudget`) and, for turns, in the
transcript (`kind:44225`). The relay validates the receipt's shape only
(`error` is `null` or `{code, message}` with a bounded code); it does not know
the vocabulary and admits any bounded code string.

## Codes

The *class* column says who answers it and when. "provider" means the session
provider that owns the addressed execution; "relay" means the relay refused
the command at ingest, so no receipt exists and the code appears in the
`OK`/HTTP rejection instead.

| code | class | carried by | meaning |
| --- | --- | --- | --- |
| `INITIAL_TURN_FAILED` | provider, create | `created_with_failed_initial_turn` | The create succeeded; the `initialTurn` embedded in it did not run. |
| `CREATE_START_OUTCOME_UNCONFIRMED` | provider, create | `failed` | Startup may have produced effects but no accepted outcome was established. The reserved target may be retained; nothing is certified. |
| `PROJECT_CWD_UNRESOLVED` | provider, create | `failed` | No working directory could be resolved for the project the create named. |
| `SESSION_LIMIT` | provider, create/resume | `failed` | The provider already runs as many sessions as it will run. |
| `PROVIDER_AUTH_REQUIRED` | provider, create/resume | `failed` | The agent adapter needs interactive authentication before it can serve. |
| `PROVIDER_UNAVAILABLE` | provider, create/resume | `failed` | The adapter could not be started or did not complete `session/new`. |
| `SESSION_ALREADY_ATTACHED` | provider, resume | `failed` | A resume addressed an execution that still has a live actor. |
| `SESSION_BUSY` | provider, restart | `failed` | A restart arrived while a turn was open; the provider will not kill work in flight. |
| `TREE_BUSY` | provider, rewind | `failed` | Another execution on this provider works in the same tree and has a turn open; the message names it. |
| `CHECKPOINT_UNAVAILABLE` | provider, rewind | `failed` | The named checkpoint could not be fetched, decoded or found in git, or the pre-rewind capture failed before any file was touched. |
| `CHECKPOINT_NOT_THIS_EXECUTION` | provider, rewind | `failed` | The checkpoint is another key's, another execution's, or an earlier generation's. |
| `NOT_RESTORABLE` | provider, rewind | `failed` | Not a turn checkpoint, `restorable: false`, or `files: restore` without the git trees a restore needs. |
| `REWIND_NOT_RESTARTED` | provider, rewind | `failed` (with `rewind`) | The checks passed but the new generation was not opened; `rewind.files` says what happened to the files. |
| `CONTEXT_NOT_RECOVERED` | provider, resume | `resumed_without_context` | A new generation started without the prior context. |
| `GENESIS_NOT_FOUND` | provider, create | `failed` | The create named a genesis event that could not be resolved and verified. |
| `ACTOR_ROLE_PAIR` | relay, create | ingest rejection | A `role` without an `actor`, or a null seat identity. Structural: refused before storage, so it never appears in a receipt. |
| `ACTOR_UNAVAILABLE` | provider, create | `failed` | The seated `actor`'s key material is not held on this host; custody is host-local by design. |
| `UNAUTHORIZED_OPERATOR` | provider, any | `failed` / `turn_refused` | The command signer is not authorized to operate the addressed session. |
| `OWNER_ATTESTATION_EXPIRED` | provider, turn | `turn_refused` | The author is a same-owner sibling whose NIP-OA attestation is not in force at the input's authorization time (expired, or not yet valid). Refused with a receipt, never dropped, so expiry is not mistaken for the agent being down. |
| `UNKNOWN_TARGET` | provider, any | `failed` / `turn_refused` | The addressed provider has no execution matching the target. |
| `STALE_GENERATION` | provider, any | `failed` / `turn_refused` | The command addressed a superseded generation of an existing execution. |
| `SESSION_CLOSED` | provider, any | `failed` / `turn_refused` | The addressed execution was already durably stopped. |
| `QUEUE_FULL` | provider, turn | `turn_dropped` | The execution's queue is full; terminal for this command, send it again later. |
| `NO_LIVE_EXECUTION` | provider, turn | `turn_dropped` | The turn addressed a persisted execution with no live process; it will not be retried — resume and send again. A replay of the same command is answered as already refused. |
| `STEER_UNSUPPORTED` | provider, steer | `turn_degraded` | The runtime never advertised native steering; the turn was delivered at the next boundary. |
| `STEER_TURN_ENDED` | provider, steer | `turn_degraded` | The steer arrived after the running turn ended (or before one existed); delivered at the next boundary. Includes the adapter's `promptRequired` answer. |
| `STEER_SATURATED` | provider, steer | `turn_dropped` | Native-steer admission is full; nothing was written. Terminal: send again once it drains. |
| `STEER_REJECTED` | provider, steer | `turn_degraded` | The runtime answered the steer with an explicit JSON-RPC error proving nothing was injected; delivered at the next boundary. |
| `STEER_ATTACHMENTS_UNSUPPORTED` | provider, steer | `turn_degraded` | The steer carried images, which the native path does not take; delivered at the next boundary with its images. |
| `STEER_NOT_DELIVERED` | provider, steer | `turn_dropped` | A late runtime answer proved an input reported unknown never reached the turn. Terminal: send again. |
| `STEER_WRITE_FAILED` | provider, steer | `turn_delivery_unknown` | The write to the runtime failed part way; bytes may have reached it. |
| `STEER_ACK_LOST` | provider, steer | `turn_delivery_unknown` | Written, but the prompt ended or the runtime exited before the acknowledgement arrived. |
| `STEER_ACK_TIMEOUT` | provider, steer | `turn_delivery_unknown` | Written, and the bounded wait for acknowledgement expired. |
| `STEER_ACK_UNRECOGNIZED` | provider, steer | `turn_delivery_unknown` | Acknowledged with a result naming no recognized outcome. |
| `STEER_UNRESOLVED_AT_RESTART` | provider, steer | `turn_delivery_unknown` | The provider restarted with a native-steer intent no acknowledgement ever resolved. |
| `STEER_UNOBSERVED_NEW_TURN` | provider, steer | `turn_delivery_unknown` | The runtime started a separate, unobserved turn with this input. Delivered; must not be resent; its output may be absent from the transcript. |
| `IMAGE_UNSUPPORTED` | provider, turn | `turn_degraded` | The runtime never advertised image prompts; the turn was delivered as text only. |
| `NO_TURN_IN_FLIGHT` | provider, interrupt | `turn_refused` | The interrupt addressed a live execution with nothing to cancel. |
| `DUPLICATE_OPERATION` | provider, turn | `turn_refused` | A second turn carried an identifier-only team-wake pointer byte-equal to one already custodied for the same exact target. Zero turns spent; both producers treat this as settlement, never failure. The message names the owning command. |
| `BUDGET_EXHAUSTED` | provider, turn | `turn_refused` | The umbrella has spent its turn budget; the message carries `used` of `limit`. Founder turns and interrupts are exempt. |
| `CI_CONTINUATION_EXPIRED` | provider, CI continuation | `failed` | The continuation reached its `expiresAt` with no result recorded for the exact run attempt. Says nothing about the run itself. |
| `CI_RESULT_CONFLICT` | provider, CI continuation | `failed` | Two or more different relay-signed results exist for one correlation digest; no winner is picked. |
| `CI_RESULT_UNAVAILABLE_OR_HIDDEN` | provider, CI continuation | `failed` | The window closed without the provider ever being able to see results for the project; "not finished" and "not permitted" are indistinguishable. |
| `COMMAND_ID_CONFLICT` | provider, CI continuation | `failed` | A registration reused a `commandId` already held for a different payload; the first durable record wins. |
| `CI_CONTINUATION_STORE_FULL` | provider, CI continuation | `failed` | The durable continuation store is at its bound; refused before acknowledgement, never queued. |
| `HANDOVER_FENCED` | provider, any | `failed` / `turn_refused` | The umbrella has been handed over ([NIP-HO](NIP-HO.md)) and this command is not the claimant acting on the claimed body. Answered from the folded authority chain, never by the relay. |
| `SESSION_RETIRED` | provider, any | `failed` / `turn_refused` | The umbrella was deleted (verified relay-signed deletion receipt, or founder kind 5 plus an authenticated read); terminal for every command. |
| `MODEL_SWITCH_UNSUPPORTED` | provider, `thread.model.set` | `turn_refused` | The adapter offered no model control at `session/new`, or this provider build does not switch models. |
| `MODEL_NOT_OFFERED` | provider, `thread.model.set` | `turn_refused` | The selection's base model is not in the instance's `allowedModels` or not offered by the live adapter. |
| `MODEL_SWITCH_FAILED` | provider, `thread.model.set` | `turn_refused` | The adapter answered the switch with an error, or a required mode could not be re-asserted. The previous model is kept. |
| `TURN_INTERRUPTED_BY_RESTART` | provider, turn | `failed` | The terminal receipt that closes a `turn_interrupted` turn ([NIP-CSL](NIP-CSL.md) § turn_interrupted): the provider restarted while the turn's process group was still present, and the witness has since proved the group gone. Same `commandId` as the interrupted receipt; the synthesized `result` item carries the same code. |
| `MODEL_AUTH_FAILED` | provider, turn or create | the transcript `result` item (44225), as `errorCode` | The runtime rejected the credential it was given. The item carries `credentialSource` — the descriptor's label, or `ambient-login` for a recognised Claude login failure — so the remedy names the file to refresh or the login to renew. Any runtime can declare it with a JSON-RPC error `data.reason` of `credential_expired`, `credential_invalid` or `credential_missing`; the item then also carries `credentialReason`, and omits `credentialSource` when the descriptor names none ([runtime-descriptor.md](../runtime-descriptor.md#reporting-a-credential-failure)). Not a 44224 `error.code`: a create whose initial turn failed this way keeps `INITIAL_TURN_FAILED` on its receipt (`credential_source.rs`, [NIP-CSL § MODEL_AUTH_FAILED](NIP-CSL.md#fork-amendment-model_auth_failed-and-credentialsource)). |
| `CANCEL_NOT_CONFIRMED` | provider, interrupt or shutdown | the transcript `result` item (44225), as `errorCode` | The turn was being stopped — by the operator's cancel, or by the provider shutting the execution down — and the adapter answered the cancelled `session/prompt` with a JSON-RPC error instead of `stopReason: "cancelled"` — for a runtime whose work runs elsewhere, typically that the work is still pending there. The turn is `subtype: "error"`, never `cancelled`; the adapter's message (1024 bytes at most, plus an ellipsis when cut) is `detail`. A cancel the adapter confirms, or one it ignores past the drain grace, is still `cancelled`. On a shutdown the sentence names no operator: for a local runtime it says the adapter was stopped with the execution; for `execution: "remote"` that work outside Beekeeper may still be running. Not a 44224 `error.code`. |
| `HOST_ANSWER_RECORDED` | provider, internal | never published | A `TurnDisposition` code for a host-answer command recognized and recorded in the transcript instead of delivered as a turn. Not a receipt code: nothing is published back under it; it exists so provider logs and tests can name what happened. |

## Codes outside the fence

These are real codes a consumer will see, defined outside
`coding_session_payload.rs` at the time of writing and therefore not checked
by the drift test above (the file column comes first on purpose: the test
reads rows whose first cell is a code). When they move beside the others,
move the row up.

| defined in | code | carried by | meaning |
| --- | --- | --- | --- |
| `crates/beekeeper-session-provider/src/runtime_credentials.rs` | `MODEL_AUTH_UNAVAILABLE` | `failed` / `turn_refused` | No credential source the runtime accepts could be resolved (no ambient login, no credential file, or a runtime with neither defaults nor a declared `credentialEnv`). The message names the fix. |

## Reading a receipt from outside

With only nostr-tools, a client that wants the outcome of its command reads
`kind:44224` events in the channel whose `commandId` is its own, keyed by
`(commandId, status)`, and treats a non-null `error.code` as the branch value:

```js
const rows = await http('/query', [{kinds: [44224], '#h': [channel], limit: 100}]);
for (const event of rows) {
  const receipt = JSON.parse(event.content);
  if (receipt.commandId !== myCommandId) continue;
  if (receipt.error) console.log(receipt.status, receipt.error.code, receipt.error.message);
}
```

`conformance/foreign-client/foreign-native-read.mjs` is that read with
signature verification; see [`docs/INTEGRATING.md`](../INTEGRATING.md).

## Not covered

Receipt statuses themselves (`created`, `turn_queued`, …) are NIP-CSL's. Host
step refusal codes (`ACTION_DEFINITION_DRIFT`, `ACTION_HOST_EXECUTABLE_MISSING`,
…) travel in `kind:46023` and are [NIP-HS](NIP-HS.md)'s; the host-step
`refusal.code` is an open string there, not a closed vocabulary, and this
fence does not cover it.
