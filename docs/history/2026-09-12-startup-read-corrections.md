# Startup reads and the history/live goal gap — 2026-09-12

Candidate: 0e4c2cd84. Fresh E2E build via build.sh; build-e2e.log,
build-head.txt, build-status.txt, build-index.sha256 and manifest.json retain
source/build identities. Four copied specs retain every original assertion and
timeout; changes are only absolute imports, external screenshots and the
probe fixture import. The first invocation failed during external module
loading (unchanged-before.log); package.json type=module corrected the harness.

The unchanged-baseline run and probe-before run each fail all six selected
cases. Per-case traces, screenshots, JSON report, and extracted
read-timestamps.json live under artifacts/<run>/results. The probe waits after
the failed assertion and cannot turn it into a pass. Times below are browser
performance.now seconds. Raw callback frame capture produced no rows; direct
IPC calls/results and DOM snapshots provide the evidence claimed here.

| Case | Failure snapshot | Read/result later |
| --- | --- | --- |
| Founder L20 | 11.17: mission-land-control absent | Scoped repository REQ 15.581; protection REQ and coding_session_land 15.582; correct ungoverned/founder UI at17.20 |
| Audit observations | 11.30: Reading this session's signed observations | 44246 REQ15.597, fold receives five signed events15.598; expected observations and gates17.33 |
| Inspector Structured tests | 11.27: No gate row yet / None has been published | 44246 REQ15.589; fold receives five events15.591; two correct gate rows17.30 |
| Route rail | 11.16: gate signs absent | 44246 REQ and five-event fold15.573; correct gate signs in later body/rail snapshot17.19 |
| Seeded catalog goal | 6.28: goal absent, still absent18.34 | Dedicated goal coalesced history query empty1.041; seed follows in test.trace; dedicated limit0 live REQ only10.570; no history catch-up |
| Pack source | 7.37: Reading source, shipped selector absent | 30624 REQ10.580; correct shipped null-source UI13.40 |

No assertions were obsolete. The bounded observation/repository/pack one-shot
reads still use fetchEvents WebSocket history, whose sendRaw admission awaits
startup send budget; they only reach IPC after the assertion. Coalesced reads
already exist and preserve the same supported filters and result semantics.
The observation fold receives the original five signed rows unchanged after
that read resolves.

Independent honesty defect: Inspector accepts gateRows but not observation
loading/error and claims no gate has been published while the read is pending.
Audit correctly distinguishes unread/error states.

Seeded goal is different: useCodingSessionGoals starts coalesced history and a
limit0 live subscription concurrently. A publication between the completed
history and actual live REQ is missed indefinitely. The seeding helper records
the real signed event and broadcasts it to currently active subscriptions;
no replay is expected for limit0. The first proposed correction was a readiness catch-up. Further API inspection
showed why that alone is insufficient: `relayClientSession.ts` starts its
readiness timer before waiting for the REQ send budget, and
`relaySubscriptionRegistry.ts` retains the first readiness answer. A timeout
can therefore suppress the later EOSE callback. Resolving the subscription
handle proves a local send, not server registration. The scoped correction
uses the same bounded goal history limit on the live subscription, allowing
the relay history/live stream to cover the gap and the existing signed fold
to deduplicate results. Opening the catalog also requests current history.
This incurs bounded duplicate history rather than widening channel or kind
access. No generic transport readiness contract changes.

The baseline and timestamp probes preceded product edits. Correction and
validation results will be recorded below; this diagnostic evidence alone
does not prove a fix or replace full smoke acceptance.

## Corrective scope and focused checks

Seven production files and five focused test files implement the correction.
Repository-owner/repository-roster reads, session observations and the project
pack source use the existing coalesced one-shot path with unchanged filters.
The strict v2 source decoder, signature checks and founder goal fold are
unchanged. The goal live subscription uses the same 1,000-record bound as
history; signed records from both paths are deduplicated by ID and folded.

History-result sequencing cannot let a stale failure replace a newer result;
changing scope disposes pending readers. A failed refresh retains valid goals,
and successful history does not erase a failed live watch. A saturated
history response discloses that older goals may be missing; it does not prove
absence. Explicit catalog opening refreshes history without reopening the
watch. Inspector renders unread/error/empty states distinctly, and an
observation-only error exposes Retry.

Independent review found the watch-error clearing and observation-only Retry
issues; both have regression coverage. Final focused JavaScript run passes
99/99, TypeScript checking exits 0 and Biome passes all 12 owned files.
The pre-fix scoped run records five failing new assertions; the separate
`live-gap-red.log` / `live-gap-green.log` records the bounded subscription
change (11 goal tests pass in that intermediate green run). Two literal NUL bytes
already existed in the observations source at HEAD; replacing them with the
source escape `\u0000` preserves runtime keys. All 12 edited source/test files
now scan without literal NUL bytes and `git diff --check` passes.

Raw focused evidence is machine-local under
`../review-2026-09-12-smoke-read-triage/`: `scoped-final-green.log`,
`scoped-final-tsc.log`, `final-biome.log`, `final-source-scan.json`, and
`final-frontend.diff`. Fresh browser results remain pending.

## Unchanged browser verification

A fresh E2E bundle from the frozen corrective source passes all six unchanged
cases (38.2 seconds). The four original spec hashes still match the baseline
manifest; assertions and timeouts did not change. Log: `corrected-six.log`;
JSON report/traces: `artifacts/corrected-six/`. The pre-correction build record
is retained under `baseline-build-record/`, and the new `build-head.txt`,
`build-status.txt` and `build-index.sha256` identify the corrected bundle.

A final Inspector refinement keeps an already-known valid signed goal visible
beside refresh/partial-read status. It does not enable editing while the
reader is unresolved/errored or present rejected/conflicting goals as valid.
That refinement follows the six-case run and requires its own focused checks
and the final combined full gate.

The final known-goal display change passes all 44 Inspector tests (including
four new cases; three failed against the preceding renderer), TypeScript and
Biome. Editing remains disabled for unresolved/errored reads. Logs:
`known-goal-red.log`, `known-goal-green.log`, `known-goal-tsc.log`,
`known-goal-biome.log`. Only the Inspector component/test changed after the
six-case browser run.
