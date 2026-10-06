# kind:44224 — coding-session lifecycle and turn receipt

`schema` exactly `buzz-coding-session-lifecycle-receipt/v1`. Two vocabularies
share the record.

## Closed key set

| Shape | Keys | Statuses |
| --- | --- | --- |
| Five-key | `schema, commandId, status, session, error` | every lifecycle status, and `turn_queued`, `turn_dropped`, `turn_refused`, `turn_degraded`, `turn_delivery_unknown`, `interrupt_delivered`, `continuation_registered` |
| Six-key | those five plus `turnId` | `turn_started` and `turn_injected`, and nothing else |

`turnId` is present **exactly** when the status is one of those two — a started
receipt without one names no turn, and any other status carrying one claims a
turn that has not begun. An explicit `"turnId": null` is a six-key object making
a third claim and is refused.

## The status/session/error coupling

- `created`, `resumed`, `stopped`, `turn_queued`, `turn_started`,
  `turn_injected`, `interrupt_delivered`, `continuation_registered`: a session,
  no error.
- `created_with_failed_initial_turn`: a session and `error.code` exactly
  `INITIAL_TURN_FAILED`. `resumed_without_context`: exactly
  `CONTEXT_NOT_RECOVERED`.
- `failed`: **no** session, an error. The one status whose `session` is null.
- `turn_dropped`, `turn_refused`, `turn_degraded`, `turn_delivery_unknown`: a
  session and an **open** error code — a provider that learns a new way to lose
  a turn must be able to say so. What stays enforced is that the code is a code.

## Scope, not defect

The desktop's coordination reader answers `false` to every turn-stage status on
purpose: it is the lifecycle vocabulary the coordination fold cares about, and
the turn stages are read by `parseCodingSessionLifecycleReceipt` instead. Those
vectors are pinned as divergences so the scope stays a decision somebody made.

The two byte-bound divergences are not scope: `beekeeper-core` bounds a turn-stage
error code at 64 bytes and a message at 1027, and the desktop ingress decoder
and mobile bound them at 256 and 2048.
