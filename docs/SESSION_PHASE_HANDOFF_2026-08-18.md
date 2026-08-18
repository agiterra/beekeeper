# Sessions phase handoff — 2026-08-18 (overnight ship)

**For:** Brian and Andy, morning testing.
**From:** the Fable session that ran 2026-08-17/18 (continuity research →
implementation → two-member live proof → overnight split + ceremony).
**Supersedes:** `SESSION_PHASE_HANDOFF_2026-08-17.md` for current state; that
document's forensics and §0 epistemic standard remain in force.

---

## 1. What shipped tonight (all live-tested before the ceremony)

Seventeen commits on `feature/coding-sessions` (+ glue), the whole
session-management arc:

- **R26 durable names** (kind 44229) and rename sync across views.
- **R27 closure/stop separation** (kind 44230): close session ≠ stop
  execution, reopen, Settled = shared closure fact — never inferred from
  execution status.
- **Rehydration** (`context_projector`, private context MCP, verified
  packages): a new execution under an existing umbrella gets verified
  session history. Founder-only sessions project without a relay identity;
  authority chains still fail closed without one.
- **systemPrompt-first continuity bootstrap** (Brian's ruling): the
  Rehydrated bootstrap rides `session/new`'s systemPrompt transport
  (`_meta.systemPrompt.append` for claude-agent-acp); first-turn injection
  is fallback-only. The transport used is persisted on the SessionRecord.
- **Continuity disclosure end-to-end**: `session_fresh` published on
  no-context creates (silence is never the signal); five continuity slugs
  render as first-class "Session continuity" rows; `resumed_without_context`
  is a distinct lifecycle state with honest copy; the attach dialog states
  the joining agent does not automatically read the transcript; "joined this
  session" seam rows in the umbrella timeline.
- **Reconnect actually reconnects**: the resume path follows its receipt to
  the newly minted generation (create-path parity), surfaces refusals
  (`STALE_GENERATION` etc.), 30s stall fallback; `disconnected`/`failed`
  outrank transcript inference in the header; routing to a dead prior
  generation self-heals onto the live one.
- **Turn refusals are visible**: an unauthorized sender sees "Turn refused:
  only the session founder or a granted operator may steer this execution"
  with their draft restored — previously the message vanished silently.
- **Foreign-member authority resolution**: a member viewing a session
  founded elsewhere resolves receipts/creates/founder from the *session's*
  declared provider authority (pinned) instead of the machine-local
  allowlist. Kills the spurious "ungoverned — adopt to govern" banner and
  the fallen-open composer on other members' clients.
- **Joins inherit project/repo claims**: added providers no longer re-file
  the session as standalone (the "session moved to General" bug).
- **Operator attribution**: `user_prompt` items carry the verified
  `operatorPubkey`; the transcript renders "You" only for your own turns,
  and the sender's name for everyone else's. Queued turns are attributed to
  their own sender.

## 2. The two-member live proof (2026-08-18, local relay)

Run with a real second identity (Brian_Dev2) in a second app instance.
Every rung passed on the wire; all evidence is signed events on the relay:

1. Non-member sees nothing (fail-closed observation).
2. Member observes the full live transcript.
3. Non-granted steer refused 3× (`UNAUTHORIZED_OPERATOR`), resume refused.
4. Founder-signed 44228 grant (seq 1) → relay-signed 40099 → provider
   applied live to both executions in 14 ms, no restart (A5's machinery).
5. **Grantee turn ran** — the never-run A5 leg: a 44220 signed by the
   grantee executed on the founder's machine, answered correctly by Codex.
6. Grantee stop still refused (grants extend steering, never custody).

## 3. For Andy's morning testing

- Pull the new `integrated` / `build/2026-08-18*` tag; the relay
  autodeploys when the pipeline is green.
- The A5b observation test: open Brian's session from your client — you
  should see transcript + continuity rows + honest gating ("Only the
  session founder can prompt executions in this version"), never
  "ungoverned", never a silently swallowed refusal.
- Local relays now advertise NIP-11 `self` when `BUZZ_RELAY_PRIVATE_KEY`
  is set in `.env` — required for authority-chain verification (grants) in
  local dev. Without it, founder-only sessions still rehydrate; granted
  sessions disclose Fresh instead of failing silently.
- Grants have no UI yet (A8 deferred behind A6, unchanged): the CLI-driven
  grant used `/tmp/grant-proof` (`publish --relay ws://localhost:3000
  --key <founder> --channel <uuid> --genesis <id> --seq 1 --grantee <pk>`).
- Membership to a session's hidden transport channel is the visibility
  gate for other members; tonight it was granted via
  `buzz channels add-member` (no UI affordance yet — known A8-shaped gap).

## 4. Known gaps carried forward (ordered)

1. **Desktop grant-awareness**: a granted operator's composer is still
   gated (desktop doesn't consume 44228 chains); the provider accepts
   their turns. A6 → A8 remains the ordained path.
2. **Rehydrate-on-reattach**: `Resumed` executions have native-thread
   memory only — no context MCP on the reattach path
   (`session.rs` resume/load pass no MCP servers; `lib.rs` reattach passes
   `rehydration_mcp: None`). Demonstrated twice; ACP transport supports
   fixing it (mcpServers accepted on all three session-open methods).
3. **Stale-Reconnect echo port**: `a92fd728` (wip/session-stability) is
   genuinely absent from this line — relay-websocket reconnect churn;
   suspected cause of a double generation bump on reconnect.
4. **`rejectedAuthorCount` surfaced nowhere** (silent drop counter).
5. **P1 seed-quality gate**: still never run; harness ready
   (`scripts/p1-seed-spike/`, `docs/P1_JUDGE_SCRIPT.md`,
   `docs/P1_CONTINUITY_MATRIX.md`). One judged hour; gates B2/B3/B4.
6. **Session-continuity research report**:
   `docs/FABLE_SESSION_CONTINUITY_RESEARCH_REPORT.md` — the full
   evidence-backed investigation (Buzz map, T3/Claude-Code/Pi/Codex
   comparative findings, taxonomy, design space, checkpoint/44231 and
   native-snapshot/44232 designs, MCP 2026-07-28 audit). The
   recommendation queue in §7 is the continuity roadmap.

## 5. Ceremony provenance (this build)

- Andy's `build/2026-08-17.5` assembly was the base; his session-shelf
  optimistic rows / Settled-bucket / sidebar-capping glue and the
  `fix/relay-multichannel-subscriptions` branch are preserved and unioned
  with tonight's closure/name/authority work (notable unions: shelf
  pending-overlay + closure facts; wire-freshness status model + terminal
  disconnected/failed precedence — his `codingSessionWireWorkspaceStatus`
  now maps those honestly instead of Idle).
- `feature/coding-sessions` carries the 16 feature commits and compiles and
  tests in isolation (R1); the known `mesh_demo` loopback-QUIC flake is
  pre-existing and documented.
- Cross-feature residue (projects-container coupling, transport-channel
  options, closure reopen's project-ACL gate wiring, fork-only docs) lives
  in `integration/glue` per the branch model.
