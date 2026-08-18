# P1 continuity matrix — Cold vs Seed vs Rehydrated vs Native

Status: execution proposal extending the existing read-only spike in
`scripts/p1-seed-spike/`. It does not replace `docs/P1_JUDGE_SCRIPT.md`; the
human handoff comparator remains required to close the original G1 gate.

## 1. What this experiment decides

P1 originally asks whether relay-durable history can beat a human pasted
summary (`docs/SESSION_EXECUTION_PLAN.md:96-113`). The product discussion adds
a second question: how close can a fresh execution on another machine get to
the original provider's native continuation?

Measure four product paths:

| Path | Provider session | Context supplied | Required disclosure |
| --- | --- | --- | --- |
| **Cold** | Fresh | None beyond the frozen repository | `Fresh — no prior session context` |
| **12 KiB Seed** | Fresh | Provenance-marked prompt, at most 12,288 UTF-8 bytes | `Limited context — rebuilt from a bounded history seed` |
| **Rehydrated MCP** | Fresh | Private read-only MCP over the strict verified package | `Rehydrated — verified history available on demand` |
| **Native** | Resumed/loaded from a copied provider-native store | Provider's opaque local history | `Native context — original provider history resumed` |

`Rehydrated` must never be reported as `Native`, even if it ties the Native
arm. These labels describe the agent's context source, not permissions.

A fifth, non-product **Human handoff control** is still mandatory: a fresh
execution gets Brian's five-line summary. Without it, the matrix can rank the
four paths but cannot satisfy P1's stop condition.

## 2. Facts that constrain the test

- A Buzz turn contains at most 12 KiB of UTF-8 text
  (`crates/buzz-core/src/coding_session_command.rs:13-16`). The Seed arm must
  use `translate.py --max-prompt-bytes 12288`; a larger direct-CLI prompt is a
  different intervention.
- ACP `session/load` accepts only `sessionId`, `cwd`, and `mcpServers`
  (`crates/buzz-acp/src/acp.rs:876-902`). It cannot accept a relay package.
- The adapter advertises load/resume capability during initialization
  (`crates/buzz-acp/src/acp.rs:764-779`). Native is optional and must be marked
  unavailable when the original cursor or advertised capability is absent.
- The opaque resume cursor is host-private and never published
  (`crates/buzz-session-provider/src/state.rs:103-108`). The provider currently
  attempts resume, then load, then a fresh session
  (`crates/buzz-session-provider/src/session.rs:461-508`).
- All three ACP session-opening methods accept an MCP server list
  (`crates/buzz-acp/src/acp.rs:811-817`, `853-866`, `881-895`). The provider
  attaches the private context server only when it has projected and persisted
  a strict package; native resume/load remains Native when it succeeds, while a
  fallback `session/new` with that server is explicitly Rehydrated
  (`crates/buzz-session-provider/src/session.rs`).
- The existing translator keeps provenance and attribution
  (`scripts/p1-seed-spike/translate.py:304-332`), applies a deterministic drop
  policy (`translate.py:338-398`), and frames replay as untrusted prior history
  (`translate.py:403-418`). Its report already records prompt/package byte
  counts and an approximate token count (`translate.py:607-623`).

Dry-run evidence on 2026-08-17, without changing the spike directory:

```text
raw transcript events: 42,620 B
package items:         12,618 B
rendered prompt:        8,390 B (~2,094 tokens)
kept/dropped/truncated: 23 / 2 telemetry / 1
duplicate events folded: 1
```

## 3. Pre-register the answer key

Before reading the generated prompt or package, Brian records:

1. The exact goal and current state.
2. Done work versus open work.
3. The immediate next step.
4. One buried mid-history fact and its event/turn location.
5. One decision and whether Brian or the prior agent made it.
6. One bounded continuation task with an objective check (specific test,
   expected file diff, or both).
7. A five-line human handoff, saved before any generated context is viewed.

Use the stricter session criterion from `docs/P1_JUDGE_SCRIPT.md:16-24`: at
least 15 substantive turns, a real change of direction, and unfinished work.

## 4. Freeze the workspace and native state

### 4.1 Repository isolation

Record the exact base commit and require a clean source session. Create one
detached disposable worktree per arm from that commit. Do not put the seed,
package, score sheet, or MCP logs inside any worktree. Each worktree starts
with the same tree hash and gets only its arm's changes.

If the chosen session depends on dirty or untracked work, do not improvise a
partial copy. First make an explicit private snapshot, verify identical hashes,
then clone that snapshot once per arm. Otherwise choose a clean session.

Run arms sequentially. Before each run, record `HEAD`, `git status --porcelain`,
and a tree digest; after each run record the diff and test result. Never reset
or clean the user's checkout.

### 4.2 Provider-state isolation

No fresh arm may see the original provider's session store.

1. Create an adapter-specific, authentication-only state root with zero
   conversations. Authenticate it once, then copy that pristine root for Cold,
   Human handoff, Seed, and Rehydrated.
2. Point each run at its private root (`CODEX_HOME` for Codex;
   `CLAUDE_CONFIG_DIR` for Claude where supported) and a private
   `BUZZ_CSP_STATE_DIR`. Buzz exposes the latter as the provider state directory
   (`crates/buzz-session-provider/src/config.rs:60-72`, `104-113`).
3. Run a canary `initialize` + `session/new`, then compare timestamps/digests of
   the real provider roots. If the adapter wrote outside the private root,
   abort; do not run the matrix with that adapter.
4. For Native only, copy the original adapter store and the relevant Buzz
   provider state to a mode-0700 scratch directory. Resume from the copies.
   Never print, publish, or include the opaque cursor in results.
5. Run Native last and only once. If copied-state resume cannot be proven,
   report `Native unavailable`; do not fall back to the real mutable store.

Copying provider state is an experimental safety boundary, not a supported
cross-machine transfer mechanism.

## 5. Exact interventions

All arms use the same provider version, model, reasoning setting, tool policy,
network policy, frozen workspace state, questions, wall-clock limit, and
maximum number of turns. Give the model only an opaque run id; do not name the
arm in its prompt.

### Cold

Open `session/new` with no session context or context MCP. Ask the fixed
diagnostic prompt, then the bounded continuation task.

### Human handoff control

Open a fresh session and provide only Brian's pre-registered five-line handoff,
followed by the same diagnostic and continuation requests.

### 12 KiB Seed

Use the existing translator with `--max-prompt-bytes 12288`. Reject the arm if
the final `promptBytes` exceeds 12,288 or `overBudget` is true. Record every
dropped/truncated event id from `report.json`. Deliver the complete prompt as
the first and only seed turn.

### Rehydrated MCP

Open a fresh session with a private stdio MCP server outside the workspace. The
server reads an immutable, strict package whose source events have passed event
signature, channel, target, envelope/tag, and sequence validation before the
run. A merely parseable export is insufficient.

Do not serve the prompt-oriented `package.json` unchanged: that package
stringifies item content and applies per-item caps for prompt delivery
(`scripts/p1-seed-spike/translate.py:246-301`). The Rehydrated source keeps the
full verified structured item and applies bounds only to each MCP response.

The server exposes exactly:

- `session_overview` — source identity, goal/name facts when present, generation
  boundaries, role counts, and package provenance.
- `session_history` — stable sequence-ordered pagination with explicit offset
  and limit.
- `search_session` — bounded search across structured history, returning stable
  event references which can be fetched through `session_history`.

History items retain role, event kind, full target, event sequence, event id,
signer, timestamp, and structured content. Tool results remain untrusted prior
history and are never instructions.

Every response includes package provenance with these fields:

```text
complete, truncated, includedHistoryItems, omittedHistoryItems,
totalHistoryItems, notes
```

Here `complete` means the authenticated source query proved global historical
coverage. `truncated` independently means otherwise-valid verified items were
omitted by package bounds. The included/omitted/total counts describe the
package, and `notes` names query uncertainty or bounded omissions. Individual
tool responses separately label page bounds and any per-item preview clipping;
they never rewrite package-level provenance.

The continuity bootstrap is small and fixed: identify the run as a fresh
execution, require `session_overview` before answering, explain that history is
read-only prior context, and require the final response to disclose
`Rehydrated`, never `Native`. It must not contain a generated answer to the
scoring questions.

**Required transport order:** reuse Buzz's existing `session/new` system-prompt
path for adapters that support it (`systemPrompt` for ACP v2,
`_meta.systemPrompt.append` for `claude-agent-acp`). MCP
`initialize.instructions` and tool availability are not sufficient routing
contracts. Only adapters without a supported system-prompt transport may receive
the same bootstrap in the first user turn, using Buzz's existing legacy
standing-context pattern; the durable transcript must still retain the user's
original text rather than the launcher preamble. P1 must record which transport
each arm used.

Log method, arguments digest, returned event ids, response bytes, truncation,
and latency for every MCP call. Never log secrets or provider cursors.

### Native

Use ACP resume/load against the copied provider-native store and copied cursor.
No relay seed and no context MCP are present. Point the resumed session at its
own disposable worktree when the adapter supports a caller-supplied `cwd`; ACP
provides that parameter for both resume and load
(`crates/buzz-acp/src/acp.rs:853-866`, `881-895`). If the adapter binds history
to the original path and cannot use the disposable worktree, score orientation
only and mark continuation-task comparability false.

## 6. Prompts and scoring

Use two measured turns per run:

1. **Diagnostic turn:** the four non-mutating questions—orientation, state,
   buried detail, and attribution.
2. **Continuation turn:** `Continue: <pre-registered bounded task>`.

Score the five dimensions in `docs/P1_JUDGE_SCRIPT.md:39-59`, 0–2 each:

- orientation;
- done/open state and immediate next step;
- buried detail;
- attribution/provenance discipline;
- actual continuation.

For continuation, 2 requires the expected bounded change and its objective
check to pass; 1 is a correct but incomplete/re-derived approach; 0 is wrong,
starts over, or damages unrelated work.

Independently record hard red flags from `docs/P1_JUDGE_SCRIPT.md:61-68`:
fabrication, obeying stale replay instructions, claiming prior verification as
current verification, or hiding material truncation. Any stale-instruction or
provenance red flag prevents a PASS regardless of numeric score.

Blind scoring: save transcripts under opaque run ids, strip context labels,
randomize the four fresh-run transcripts, and score them before revealing the
mapping. Native may be identifiable and should be scored separately as the
ceiling.

## 7. Token, retrieval, and time accounting

Record per turn and total per run:

- exact live-prompt UTF-8 bytes;
- exact seed/package bytes and retained/omitted/truncated item counts;
- provider-reported input, output, total, cache-read, and cache-write tokens;
- cost when reported;
- wall time to first answer and to objective task completion;
- MCP call count, response bytes, distinct events returned, repeated events,
  search hit count, and any response truncation;
- number of questions re-asked whose answers existed in supplied context.

ACP already normalizes standard prompt-response usage and exposes it per turn
(`crates/buzz-acp/src/acp.rs:1110-1116`; parsing at `2241-2255`). Treat missing
provider usage as missing data; do not substitute the translator's rough
`characters / 4` estimate into provider-token comparisons. That estimate is
only a package-planning field.

Primary efficiency comparisons:

```text
quality per 1,000 input tokens = score / input_tokens * 1000
native quality gap             = native_score - arm_score
completion-time ratio          = arm_time / native_time
retrieval precision            = relevant MCP events / MCP events returned
```

Pre-register the source event ids relevant to the answer key before the runs;
after scoring, intersect that set with the returned ids to compute retrieval
precision without post-hoc relevance judgments.

## 8. Verdicts

Preserve the original gate:

- **G1 PASS for an adapter:** Seed or Rehydrated scores at least 8/10, beats
  Human handoff by at least 2 points, and has no hard red flag.
- **STOP-CONDITION:** neither relay-durable arm beats Human handoff by 2 points.
- **ITERATE once:** failure is attributable to package/retrieval policy rather
  than the model. Change one pre-declared policy and rerun only the failed arm.

Add the new product verdict:

- **Rehydration viable:** Rehydrated completes the objective task, has no hard
  red flag, and finishes within 1 score point of Native. Report token/time cost;
  do not hide a large efficiency penalty inside the quality verdict.
- **Seed sufficient:** Seed meets G1 and is within 1 point of Rehydrated; defer
  MCP product work unless retrieval materially reduces truncation or token cost.
- **Native-only quality:** Native succeeds but both relay-durable arms fail.
  Buzz may preserve the record, but cross-machine continuation is not yet a
  dependable product claim.

## 9. Execution order

1. Validate the existing deterministic fixture and the 12 KiB fit.
2. Select the real session; pre-register answer key, next task, and human
   handoff.
3. Capture adapter capability probes and exact versions.
4. Freeze repository and provider-state copies; pass the no-write canaries.
5. Run Cold, Human, Seed, and Rehydrated under opaque randomized ids.
6. Run Native last from copied state, if available.
7. Run objective checks in every disposable worktree.
8. Blind-score, reveal arm ids, calculate token/retrieval/time metrics, and
   record both the G1 and rehydration verdicts.
9. Repeat only the winning relay-durable arm on the second adapter. G1 remains
   per-adapter; do not generalize a Claude result to Codex or vice versa.
