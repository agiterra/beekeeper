# Dual-stream thesis — research capture, 2026-08-19

**Status:** working research document on `wip/dual-stream-thesis`. Not an
authority. Written to preserve one session's output so work can continue
later; when its findings graduate, they land in `SESSION_STATE.md` or the
research report's addenda per the ledger rule.

**Contents:**
- **Part I** — Addendum D draft: the commit-language thesis contribution
  (custody-time memory, the Git–relay join, the citator). Candidate addendum
  to `FABLE_SESSION_CONTINUITY_RESEARCH_REPORT.md`.
- **Part II** — Four research reports (Sonnet agents, 2026-08-19):
  context-as-durable-memory; tool-call augmentation middleware; Entire.io
  plans and the session-leverage landscape; the `amas` predecessor project.
- **Part III** — Consolidated synthesis and recommended next actions.

**Provenance:** Part I was written by Fable after reading `AGENTS.md`,
`docs/SESSION_STATE.md`, `docs/SESSION_VISION.md`, the full
`FABLE_SESSION_CONTINUITY_RESEARCH_REPORT.md` (incl. Addenda A–C), and the
live commit history of this branch. Part II reports are near-verbatim from
four Sonnet research agents; their claims cite their sources and were not
independently re-verified except where noted. Brian's five-message and
fifty-message native-agent experiments are treated throughout as reported
observations (not re-run).

---

# Part I — Addendum D (draft): The commit-language layer

*Custody-time memory, the Git–relay join, and a citator for claims. A thesis
contribution building on Brian's two-stream observation and Sol's continuity
work (report §7, Addenda A–B).*

## D.1 The thesis, restated

Buzz's continuity program has so far been built on a single hard-won
principle, stated in the report's first line: *the observed failure is not a
missing mechanism, it is a missing truth.* Every design that followed — the
disclosure wiring, the deterministic brief, the checkpoint, the snapshot
lane — is a way of making what the system knows into something it says,
without ever saying more than it knows.

The commit-language observation extends that program in a direction none of
the existing layers anticipated, because it is not something we built. It is
something the agents built, inside a channel that was lying around.

Restated:

> **Git's commit-subject field is the only place in the entire system where
> an agent writes durable, addressable prose while still holding full
> working context — and it does so at zero marginal cost, as a side effect
> of work it was doing anyway.** Agents, under no instruction to do so, have
> progressively densified this channel until it functions as a compressed
> project language: subjects that are simultaneously patch labels, decision
> records, supersession notices, ownership claims, and addresses into a
> shared body of doctrine.

Addendum A engineered exactly this property deliberately — its deepest line
is *"checkpoint authorship follows context custody."* The commit-language
observation is that custody-time authorship did not wait to be engineered.
Git demanded a message at every commit; the party holding context supplied
one; and because the writers were language models writing for future
language models, the messages evolved toward the optimum for that reader.
The 44231 checkpoint is the *deliberate* custody-time memory act. The
commit subject is the *incidental* one — older, cheaper, narrower, and
already deployed at scale in this repository.

The design consequence: treat agent commit language as a third memory
channel alongside the signed transcript and the model-written checkpoint —
but only after doing to it what Buzz did to continuity: refusing to let an
attributable claim masquerade as a verified truth. Git supplies the material
grounding; the relay supplies causality and attribution; and a claim-status
model — best understood as a **citator** — supplies the honesty.

## D.2 What kind of language this is

### D.2.1 The reader it is optimized for

Conventional commit style was shaped for a human reviewer scanning `git
log`. The register visible in this repository's newest commits is shaped for
a reader that has never existed before:

- **perfect recall of general software knowledge** (the pretrained prior),
- **zero episodic memory of this project** (the fresh context window),
- **cheap, fast pull access to the corpus** (tools, MCP, `git show`).

For that reader, the optimal message is exactly what we observe: heavy on
*addresses* (cheap to dereference), light on *explanation* (already in the
prior), dense in *project-specific coinages* (the only bits the reader
genuinely lacks). `glue: Settled means user-ended in the project shelf`
spends none of its sixty characters explaining what a shelf is — the writer
correctly models a reader who can infer or pull those. The subject carries
only the *delta against the prior*: the one binding fact a generic strong
software model would not predict.

This is compression against shared side information, and it explains the
fifty-message experiment's arithmetic. ~1,300 tokens of subjects decoded
into ~5,100 tokens of reconstruction because the subjects were never
self-contained messages; they were **keys into the decoder's prior**. The
expansion is the decoder regenerating, from its prior, everything the
encoder correctly declined to transmit. That is also why the reconstruction
was *plausible but unverified*: the prior supplies generic structure; only
the corpus supplies project truth. The language is a lossy codec whose
decoder hallucinates gracefully — its greatest efficiency and its central
hazard are the same property.

### D.2.2 The nearest analogue is common-law citation practice

Not telegraphese (which explains brevity but not structure). The structure
is that of a **common-law reporter**:

- Each commit subject is a **headnote**; the diff is the full opinion.
- Ancestry is **precedential order**.
- `docs(sessions): P1 judge script; ruling R25 — custody and co-input both
  ship` is a **published holding**, citable ever after by docket number.
- `glue: Settled means user-ended in the project shelf` is a **narrowing
  construction** — restricting an earlier term without overruling it.
- Coined terms — *ceremony, shelf, umbrella, glue, assembly* — are **terms
  of art**: meaning fixed by the ruling that defined them and refined by the
  chain of subsequent use.
- The speech-act prefixes (`glue:`, `fix(integration):`, `docs(sessions):`)
  are **jurisdictional claims**. Under Andy's branch model they assert which
  patch series owns the change — an ownership claim the integration ceremony
  mechanically enforces. One of the few parts of the language that is
  *already verifiable* (observed: prefixes in the live log align with the
  branch model).

The frame is productive because it predicts phenomena: terms of art
(observed); the distinction between *distinguishing* and *overruling* (maps
onto `superseded` vs `reverted`); vocabulary drift and circuit splits; and
the need for the one institution the commit stream lacks — a **citator**,
the apparatus that answers "is this holding still good law?" One limit: case
law is written under adversarial scrutiny; commit subjects under none. The
register has the *form* of a reporter with none of its *verification
institutions*. That gap is what the Git–relay join fills.

### D.2.3 Why it became compact — three pressures, one new

1. **The field's shape** — ~50–70 chars of subject by convention. (Observed:
   average length essentially unchanged, 63.0 → 62.8 chars across the
   oldest/newest-100 study.)
2. **The reader's prior** — everything the reader can regenerate is wasted
   transmission. Weak for human readers; overwhelming once the reader has a
   pretrained prior.
3. **The writer's custody** — the writer holds full context *now* and knows
   it is about to lose it; a rational incentive to externalize the
   load-bearing bits into the one durable field available at the moment of
   the act. This pressure is new, and it is the same pressure Addendum A's
   closing checkpoint formalizes.

The measured signature — constant length, rising symbolic density
(multiclause 3%→18%, local grammar 0%→59%, symbolic vocabulary 1%→8%) — is
what you would expect if the channel's *capacity* is fixed while its *code*
improves. The language is not getting longer; it is getting a better
codebook. (Hypothesis; the oldest/newest comparison is confounded by the
upstream-PR vs local-work population split. See D.10.2.)

## D.3 Terms as addresses: baptism, chain of use, dereference

A coined term behaves like a pointer with three possible backing stores:

1. **The doctrine corpus** — `SESSION_STATE.md`, the rulings log,
   `INTEGRATION.md`, the vision documents. (*Ceremony* is operationally
   defined in the ledger §3a; *R25* dereferences to a specific ruling.)
2. **The commit chain itself** — meaning fixed by history of use (*shelf*
   baptized around `glue: session-shelf optimistic rows...`, refined through
   `one shelf row per umbrella`, `Settled means user-ended`, `prefer active
   umbrella executions`). The chain is a distributed definition.
3. **The author's in-context memory at authorship time** — the richest
   store, gone the moment the execution ends.

Dereferencing succeeds when the reader can reach store 1 or 2. When it can
reach neither — the five-message experiment's condition — the decoder
silently substitutes **the pretrained prior**, returning a plausible generic
referent instead of the project's actual one. The experiment observed
exactly this: fluent in the genre, wrong about the holdings.

First crisp requirement: **make dereference checkable.** A maintained
**lexicon** — term, one-line definition, baptism pointer (commit OID or
ruling event id), superseded-by — delivered in bounded form with the
first-turn brief and dereferenceable in full through the context MCP. The
lexicon does for vocabulary what the evidence brief does for history:
converts "the model must guess" into "the model can cite."

Two properties of these addresses:

- **Rigid within the jurisdiction, meaningless outside it.** An asset for
  continuity and a wall for outsiders — including upstream `block/buzz`
  reviewers. Feature-branch commits destined for upstream should stay closer
  to the general register; the deep dialect belongs on glue and docs
  commits. (Observed: this stratification already roughly holds.)
- **Subject identity is not commit identity.** Cherry-pick/rebase duplicate
  the sentence under a new OID; squash erases many sentences into one. The
  *address* (term) survives these operations; the *anchor* (OID) does not.
  Evidence must bind to trees and OIDs, never to subject text.

## D.4 Composition: how sequences of subjects carry bodies of work

Three compositional mechanisms, all visible in this log and all recovered by
the fifty-message experiment's reader:

1. **Narrative threading by shared vocabulary.** The shelf arc reads as one
   storyline because *shelf*/*umbrella* thread it; each new subject assumes
   and extends the established senses.
2. **Supersession as first-class narrative structure.** The `Settled
   means...` construction; the base-ref sequence `cc8a8b0dc → 068a83b0 →
   cc8a8b0dc`; `park portable handoff exploration` as a boundary marker.
   Chronological + ancestral order is load-bearing.
3. **Register shifts as work-type markers.** The prefix grammar partitions
   the stream into lanes; a reader reconstructs the *shape* of a work period
   from prefixes alone.

Stated carefully: a subject sequence does not compress a session the way a
summary does (selecting and restating). It compresses the way an **index**
does: giving a prepared reader enough addresses, in the right order, to
regenerate the account from prior plus corpus. That is why it can be so
small, and why it can never be trusted alone.

## D.5 What the language preserves and destroys

**Preserved, often uniquely well:** decisions and their supersession order;
boundaries between bodies of work (`park`, ruling numbers); ownership and
routing claims (checkable); warnings addressed to successors (`the ceremony
gate is narrower than CI`); custody-time perspective — what the change
*meant to the party making it, at the moment of making it* — which no
retrospective summarizer can recover.

**Destroyed, structurally:** everything uncommitted (abandoned approaches,
failed experiments, resets, non-code work — the channel is silent exactly
where the ledger's hardest findings live); the losing side of decisions
(holdings, not arguments); uncertainty grading (subjects assert flatly —
"close the two gaps" reads identically whether verified or hoped, which is
exactly the gap the citator must fill from outside); accessibility
(compression against a shared codebook is exclusionary by construction).

The design must never treat commit language as *the* memory. It is the
zero-cost memory of the committed subset, and the committed subset is a
biased sample of the work. The layers that cover its silences — checkpoint
for uncommitted/non-code work, transcript for process, ledger for negative
results — are its complement, not its redundancy.

## D.6 The join: witnessed transitions and a citator for claims

The proposed join (fingerprint around mutating tools; observation hooks;
turn-boundary reconciliation; commit OID as join key) is correct in shape —
it observes rather than intervenes, parses Git's object graph rather than
command text, and refuses to guess authorship. Refinements at the joints:

### D.6.1 Name the object: the witnessed transition

Two new kinds (numbers to confirm against `buzz-core/src/kind.rs`; next free
after Addendum B's 44232 would be 44233/44234):

- **44233 `KIND_CODING_SESSION_GIT_TRANSITION`** — the witness record:
  before/after fingerprints, per-ref old/new OIDs, newly reachable commits
  with parents/subjects/changed-paths/diff-digests, rewrite mappings, and an
  **attribution class** (D.6.3).
- **44234 `KIND_CODING_SESSION_CHECK`** — the verification record: named
  check, exact tree it ran against, tool-call/result event ids proving the
  run, exit status, claim dimension it bears on.

Both follow the 44223/44225 envelope discipline: `h`-tagged, target-tagged,
size-bounded, agent-fence signer trust, provenance-not-truth. Split because
they have different writers, cardinalities, and consumers.

### D.6.2 The workspace fingerprint should be a Git tree OID

Instead of a bespoke `dirtyDigest`, compute a **synthetic tree OID**: stage
the working tree (tracked + untracked non-ignored) into a *temporary index*
(`GIT_INDEX_FILE=<tmp> git add -A && git write-tree`); record the resulting
tree OID. Real index/HEAD/refs untouched.

Buys: (1) checks against dirty trees get the same verification semantics as
checks against commits — a 44234 check binds to a tree OID, full stop;
(2) **retroactive upgrade for free** — the common run-tests-then-commit
workflow: when a later commit's tree OID equals the checked tree OID, the
check binds to the commit by identity, mechanically; (3) `dirty` becomes
derived: `syntheticTree != HEAD^{tree}`.

Residue: ignored files, modes beyond Git's model, environment-dependent
checks. A check verifies at most "this command exited this way at this tree,
on this host, at this time" — host and time stay in the record.

### D.6.3 Attribution is a witness class, never an authorship claim

Closed vocabulary:

- **`tool_correlated`** — bracketed by a specific tool call, or hook record
  carrying this turn's `BUZZ_TOOL_CALL_ID`.
- **`turn_correlated`** — hook records carried this execution's
  `BUZZ_TURN_ID`/`BUZZ_EXECUTION_ID`, no single tool call brackets it.
- **`sibling_correlated`** — hook records carried a *different* live
  execution's identifiers (shared-worktree/shared-repo cases).
- **`reconciled`** — found only by turn/session-boundary comparison; the
  honest label for hook-less writers (libgit2 runs no hooks) and spool loss.
- **`external`** — affirmative evidence of outside origin (committer not in
  roster, arrival via fetch/pull, upstream ancestry).

Rules: classes state **evidence strength, not authorship** — render
"observed during turn N," never "created by the agent"; Git author/committer
fields are themselves claims. **Environment correlation beats time-window
correlation** — anything classified by window alone is `reconciled`, not
`turn_correlated`.

Repository fact making this non-optional: **worktrees isolate trees, not
refs.** The ref namespace is repo-global, so concurrent-worktree executions
share one ref stream; per-execution attribution can only come from
hook-carried correlation IDs. Fingerprint is two-scoped: per-worktree (HEAD,
synthetic tree) and per-repository (refs).

Hook precision: `--no-verify` bypasses `pre-commit`/`commit-msg`, **not**
`post-commit`; nothing bypasses `reference-transaction` for Git-binary ref
updates. Roles: *tool boundary* for turn association, *hooks* for
fine-grained attribution + rewrite mappings, *reconciliation* for
completeness (non-Git writers, hook failures).

### D.6.4 Record rewrites as witnessed supersession edges

`post-rewrite` yields exact old→new OID mappings — record as first-class
`rewrites: [{old, new}]` edges. Upgrades *rewrite-supersession* from
inference to witnessed fact; *semantic supersession* (a later commit
changing the same value back, e.g. the base-ref sequence) remains a derived
judgment. Never conflate the two.

### D.6.5 The citator: currency is computed, not stored

Sort truth-model dimensions by where their truth lives:

- **Witnessed facts** (store forever, relay events): assertions,
  transitions, checks-at-tree, rewrite edges, deploy observations.
- **Derived-but-stable** (compute once, cache): materialization; structural
  claim decisions mechanically decidable from a tree.
- **Relative judgments** (compute at read time, always stamped):
  *contained / superseded / reverted / current* are properties of a
  **(commit, ref, time)** triple, not of a commit. On this fork `integrated`
  is rebuilt and force-pushed — containment flips without local action.
  Storing `containedInCurrentHead: true` in a durable event would repeat the
  frozen-but-current-looking failure of ledger §2.4.

**The relay stores witness records; the citator is a projection**, stamped
`asOf` (the `completeAsOf` discipline). A claim's full status reads as a
citation history, e.g.:

```
"close the two gaps that let a green gate ship a broken deploy" (718371ca)
  asserted    — commit subject, recorded author Brian, observed during turn …
  materialized— tree diff touches .woodpecker/*, scripts/integrate.sh
  checked     — "woodpecker gate run 1123: passed" at tree 3f2c… (event …)
  current     — contained in integrated as of 2026-08-19T…Z
  unproven    — the English claim names a universal; no probe decides it.
```

The last line is the one the system must never drop: structural sub-claims
are decidable; the universal in the prose is not. The citator's honesty is
showing checked sub-claims *and* the unchecked remainder side by side.

## D.7 Where this sits in the layer stack

| Lane | Authored when | Inference cost | Anchor | Covers | Fidelity |
|---|---|---|---|---|---|
| Signed transcript (44225) | continuously | none | relay event ids | the *process* | exact, verified |
| Native snapshot (44232, L4t) | at capture points | none | encrypted blob + pointer | model's own context | full, opaque, same-adapter |
| Model-written checkpoint (44231) | deliberately, at custody | one turn | relay event + coverage seq | the *session*, incl. uncommitted/non-code | distilled, provenance-marked |
| **Commit language (44233/44234 join)** | **incidentally, at custody** | **zero marginal** | **content-addressed Git object** | **committed work only** | **compressed claim + material proof** |
| First-turn brief | at projection | none | cites all of the above | delivery, not storage | proven facts only |

Identity: **the commit subject is a micro-checkpoint the system gets for
free** — custody-time authorship like 44231, but per-change, Git-anchored,
zero-cost, and surviving even when the execution dies before any checkpoint
turn (Addendum A's dead-execution bootstrap problem does not exist for this
lane).

Relationships:

- **Transcript:** orthogonal axes of the same events — process vs product;
  the 44233 witness record is the edge between them.
- **First-turn brief:** admissible under the brief's founding principle
  (ledger §2a: emit only what the record proves; never a model-written
  summary). Subjects are pre-existing, Git-anchored, witnessed claims,
  citable by OID like turn outcomes by event id. What would violate the
  principle is summarizing commits at projection time — never do that.
- **Checkpoint (Addendum A):** complementary and mutually strengthening. The
  checkpoint narrates what the commit stream destroys; the commit stream
  anchors the checkpoint. Extend A.1's template: *cite commits by OID when
  describing committed work* — a checkpoint's load-bearing claims become
  independently checkable through the citator. Not a renaming of A: it lets
  the checkpoint spend inference on what only inference can do.
- **Native snapshots:** disjoint lanes.
- **Git history:** the ground; this layer is an interpretation discipline
  over it — no trailers, notes, or shadow branches added, deliberately;
  degrades gracefully to plain `git log`.
- **Workspace:** the only layer true *now*; the synthetic-tree fingerprint
  is how dirty state enters the evidence system without being committed.

## D.8 Delivery to a brand-new execution

The brief gains a bounded **Changes** chapter and a bounded **Lexicon**
block, systemPrompt transport per the report's mandated order, both
deterministic projections with zero model authorship.

**Changes** — most recent K witnessed transitions relevant to the session
(introduced by its executions + external transitions on its working refs),
ancestrally ordered:

```
79861286 chore(integration): stamp CI base ref cc8a8b0dc
  claim (asserted) · observed during turn 41 of execution … · in integrated as of <t>
  check: integration base-ref guard passed at this tree (event …)
supersedes 8e55011b ("…068a83b0") — value-level, derived
```

Rendering rules: subjects verbatim but bounded and control-stripped; every
subject visibly a claim; attribution class in human words; currency always
`asOf`-stamped; unverified claims rendered with nakedness showing
(`verification: none recorded`), never omitted. Section ends with the
coverage disclaimer: *"Commits describe only committed work. Uncommitted,
abandoned, and non-code work is not represented here; see the checkpoint and
history tools."*

**Lexicon** — top-N active terms (recent-use frequency), one line each:
term, gloss, baptism pointer. ~10 lines stops the graceful-hallucination
failure mode for load-bearing vocabulary; the rest stays pull-based.

**Pull side** — context MCP grows three tools: `git_transition(oid)`,
`claim_status(oid)` (citator judgment, stamped), `term(term)`. Same
discipline as `session_history`: read-only, paginated,
evidence-not-instructions framing.

## D.9 Hard cases and trust risks

**Coverage gaps** (uncommitted, abandoned, no-commit turns, non-code): the
layer is silent — *say so* (disclosure sentence), pair with checkpoint layer
and dirty-state witnessing. A turn ending with a changed synthetic tree and
no commit is itself a witnessable fact.

**Cardinality** (many commits/turn; commit spanning turns): witness records
are per-transition. The spanning case: attribute the commit event to its
committing transition; content lineage is unknowable from outside and must
not be claimed (the checkpoint, which watched the turns, may assert it — as
a claim).

**Concurrency** (shared refs, shared worktrees): two-scope fingerprint +
env-beats-time-window. Residual: interleaved uncommitted edits in one shared
worktree are unattributable at file level — classify `turn_correlated` at
best and surface the shared-worktree condition itself (detectable) as a
warning fact.

**History mutation** (amend, rebase, squash, reset, unreachable,
never-pushed): witnessed rewrite edges + computed currency cover all without
special cases. Nasty case: upstream squash-merge of a fork PR — local
subjects vanish from surviving ancestry while claims live on in the relay;
citator handles it, but the *language* is damaged (see merge-policy
prediction, D.10.2).

**Imports/foreign authorship:** class `external`, plus **population
labeling** — this repo's history is two populations (upstream PR history;
fork-local agent work) that must be neither conflated statistically nor
trusted equally.

**D.9.1 Adversarial and injected subjects.** Three surfaces: (1) injection
into fresh contexts — same replayed-history surface as report §5.2;
evidence-not-instructions framing, bounding, control-stripping; upstream
subjects prominently labeled `external`. (2) Secrets — a secret in a commit
message today reaches Git; under this design it also reaches a
channel-readable relay event. Subject text must pass the same publication
policy that resolves open ruling §9.1; **44233 must not ship ahead of that
ruling with verbatim subjects.** (3) False claims — handled by the citator's
refusal to upgrade any claim on subject text alone.

**D.9.2 Goodhart pressure.** Once subjects seed continuity, they drift from
describing work toward managing successors' beliefs. Defenses: (a) **the
subject's only privileged role is as an address** — no truth dimension ever
advances on subject text; checks bind to trees; a magnificent subject over
an empty diff renders as `asserted, materialization: trivial`; (b) visible
nakedness — inflation buys prominence and scrutiny simultaneously;
(c) pre-registered drift measurement — run the subject–diff entailment audit
(X3) *before* shipping, re-run after; the delta is the Goodhart measurement;
(d) **no subject-quality gates** — a CI check that scores subjects would be
the strongest invitation to Goodharting.

**D.9.3 Vocabulary drift/collision.** Failure mode is *silent* drift. The
lexicon converts drift into supersession records (append-only latest-wins,
like goal/name revisions); a term, like a commit, has a currency. Mitigate
lexicon rot by giving the checkpoint turn a one-line duty: "define any term
you coined this session."

**D.9.4 The temptation to formalize the language.** Resist in-band
formalization (mandated trailers, structured schemas, hook-injected
metadata): observation-only stance; Goodhart (a schema is a target); and
**this language is an instrument reading** — its uninstructed evolution is
evidence about how agents externalize memory under pressure. Formalize the
*join* (out-of-band relay events) and the *codebook* (lexicon); leave the
grammar wild. If agents converge on trailer conventions spontaneously,
parse it, don't mandate it.

## D.10 Falsifiable experiments

### D.10.1 Product-side (buy into the P1 apparatus, don't build parallel)

- **X1 — Orientation delta:** brief-variant arm — (a) current brief,
  (b) brief + Changes/Lexicon, (c) subjects-only without citator status
  (ablation). Score on G1 dimensions + new red flag: *treating an `asserted`
  claim as established*. Prediction: (b) beats (a) at <2 KiB added seed; if
  (c) matches (b), the citator apparatus is decoration.
- **X2 — Attribution gauntlet (fixture-level):** scripted session performs
  the full edge-case list against known ground truth. Acceptance: zero false
  `tool_correlated`/`turn_correlated`; every ground-truth transition appears
  somewhere; rewrite edges exact.
- **X3 — Subject–diff entailment base rate:** ~100 fork-local commits,
  blind-judged (entailed / partially / aspirational / wrong). Run **before**
  shipping, pre-registered; re-run quarterly. The delta is the Goodhart
  measurement; the baseline measures today's reliability of the channel the
  thesis is built on.
- **X4 — Dereference test:** subjects with symbolic terms, with/without
  lexicon pointers; measure prior-hallucinated vs citation-grounded
  expansion. Direct test of "terms are addresses."
- **X5 — Economics:** token cost of the Changes chapter vs observed cost of
  unaided reconstruction (~5,100 tokens in the fifty-message datum).
  Falsified if oriented agents re-derive the relationships anyway.

### D.10.2 The language-evolution hypothesis, tested seriously

**Corpus:** public repos stratified by authorship regime (human-only
pre-2023 baselines / agent-assisted / agent-dominant), identified via
Co-Authored-By trailers, committer patterns, tool signatures — with the
label's precision validated on a hand-audited subsample. Stratify by
**merge policy** (squash destroys the phenomenon — a different medium, not a
null result), repo age, team size.

**Core metric — compression against a shared prior:** fixed reference LM;
per-subject perplexity (i) unconditioned and (ii) conditioned on the repo's
earlier subjects/docs. **Local-language density** = gap (i)−(ii).
Predictions: (P1) density rises faster over repo lifetime in agent-dominant
repos than matched human repos; (P2) raw length stays flat while density
rises; (P3) coined-term half-life is longer in agent-dominant repos. Nulls
that kill/diminish: density rise fully explained by conventional-commits
adoption or age; or human expert teams show equal growth (demotes to
"context pressure creates this register in any high-context author").

**Secondary metrics:** coinage rate per 100 commits; cross-reference density
(citing rulings/ids/prior subjects); multiclause rate; supersession
constructions.

**Cross-model design:** within multi-agent repos, mask authorship and train
a classifier. Success ⇒ partly model idiolect; failure with high density ⇒
genuinely a **project language** shared across models — the stronger result,
and the one Buzz needs (cross-provider seeds).

**Causal probe:** within this fork, vary the brief (with/without Changes)
across work periods; if agents write denser subjects *when they know
successors read them*, the language is communicative adaptation. (Doubles as
the first controlled observation of Goodhart onset — instrument X3 around
it.)

**Free sociological prediction:** agent-dominant repos will measurably drift
away from squash-merge. If in five years they squash as much as ever, the
language is not load-bearing and this addendum overclaimed.

## D.11 Ledger

**Observed (verified in this repo, this pass):** the register is real and
lane-stratified; the prefix grammar aligns with branch ownership (checkable
claim class); supersession constructions and value-flip sequences exist; the
design DNA extended here (facts-not-aspirations, disclosure of the negative
case, provenance-not-truth, custody-time authorship, `completeAsOf`) is
consistently present in shipped/specified work.

**Reported (Brian's experiments, not re-run):** the five- and fifty-message
reconstructions and measurements; the oldest/newest-100 register comparison
with its acknowledged confound.

**Hypotheses:** the evolution claim (P1–P3); orientation value of
subjects-as-seed (X1); economics (X5); cross-model sharedness; squash
prediction.

**Recommendations (dependency order):**
1. Witnessed transition / check split (44233/44234), closed attribution
   classes; observation-only, no Git mutation, ever.
2. Synthetic tree OIDs as workspace fingerprint.
3. Truth model as a **citator**: witnessed facts stored, currency computed
   at read time, all relative judgments `asOf`-stamped. Never store
   containment.
4. Brief gains bounded Changes + Lexicon chapters; three pull tools on the
   existing sidecar.
5. Extend the A.1 checkpoint template: cite commits by OID; define terms
   coined this session.
6. Gate 44233 subject publication on the §9.1 publication ruling.
7. Run X3 before shipping anything (pre-registered Goodhart baseline); fold
   X1/X4 into P1. P1 remains the gate on the whole track.
8. Do not formalize the language in-band.

**Open questions:** the §9.1 ruling (now covering subjects); lexicon home
(provider-maintained / doc / relay-native — leaning checkpoint-fed
relay-native); whether external transitions on shared refs belong per-session
or in a repository-scoped stream; whether upstream-destined commits should be
dialect-free by policy or continue self-organizing.

**Shortest version:**

> Agents given a durable sixty-character field at the moment of fullest
> context invented a citation language for their successors. Buzz's job is
> not to invent that language, improve it, or trust it — it is to witness
> it, attribute it, join it to the material record, and deliver it to the
> next reader labeled exactly as strong as the evidence behind it. Git
> proves what existed; the relay proves who said so and when; the citator
> says what is still good law; and the language — left wild — keeps telling
> us how agents remember.

---

# Part II — Research reports (Sonnet agents, 2026-08-19)

Four parallel read/research agents. Reports below are near-verbatim; URLs
are as the agents reported them and were not independently re-fetched.

## II.1 Stored context as durable memory (state of the art, 2025–2026)

### Agent memory architectures and products

**Letta / MemGPT** — [Agent Memory](https://www.letta.com/blog/agent-memory/),
[Sleep-time compute](https://www.letta.com/blog/sleep-time-compute/).
Three-tier model: **core memory** (small always-in-context block, agent
read/writes directly), **recall memory** (searchable history outside
context), **archival memory** (long-term store via tool calls,
Postgres+pgvector). Writes are **agent-driven and in-flow** — if the model
doesn't judge something worth saving, it's lost; every memory op costs
inference. **Sleep-time compute** (Letta 2.0): a second dedicated agent runs
during idle periods to rewrite/reorganize core memory — "raw context" →
"learned context." Relevance: direct precedent for "relay records raw events
in-flow; a separate background process distills later" — capture and
distillation decoupled.

**Zep / Graphiti** — [arXiv:2501.13956](https://arxiv.org/abs/2501.13956),
[Neo4j blog](https://neo4j.com/blog/developer/graphiti-knowledge-graph-memory/).
**Bi-temporal knowledge graph**: every edge carries event-time and
ingestion-time plus source provenance. On conflict, **invalidate, not
delete** — old edges get an end-validity timestamp. Three tiers: episodic
nodes → semantic entities/facts → community summaries. Benchmarks: DMR 94.8%
(vs MemGPT 93.4%); LongMemEval +18.5% accuracy, −90% latency. Relevance:
bi-temporal invalidate-don't-delete is close to what a signed relay wants
for memory supersession — supersede without destroying the audit trail of
what was believed when.

**Mem0** — [arXiv:2504.19413](https://arxiv.org/abs/2504.19413). Extracts
salient facts to a vector store; an LLM router classifies each candidate
against top-k existing memories as **ADD/UPDATE/DELETE/NOOP** (conflict
resolution at write time). 26% relative improvement over OpenAI memory on
LLM-judge metrics; 91% lower p95 latency; >90% token savings vs full-history
replay. Relevance: clean model for reconciling new signed events with
existing memory entries rather than just accumulating.

**LangMem (LangChain)** —
[conceptual guide](https://langchain-ai.github.io/langmem/concepts/conceptual_guide/),
[GitHub](https://github.com/langchain-ai/langmem). Splits (a) in-conversation
memory tool the agent calls live, and (b) background **Memory Manager**
running post-hoc over transcripts (extract/update/consolidate/delete via
`trustcall`). Explicit semantic/episodic/procedural taxonomy; tunable
creation-vs-consolidation balance. Relevance: the live-tool vs
background-consolidation split maps onto "relay captures the raw stream; a
separate job distills"; the three-type taxonomy is a candidate schema for
memory event kinds.

**Anthropic — memory tool + context editing** —
[context-editing docs](https://platform.claude.com/docs/en/build-with-claude/context-editing),
[blog](https://claude.com/blog/context-management). Two orthogonal
mechanisms: **context editing** (`clear_tool_uses_20250919`,
`clear_thinking_20251015`) is server-side *lossy pruning* of old tool
results/thinking — window management, not memory. **Memory tool**
(`memory_20250818`, Sept 2025): file-system-like CRUD against a `/memory`
directory that the *client application* persists — writes in-flow,
self-directed. Combined benchmark: context editing alone −84% tokens;
combined with memory +39% task performance. Relevance: functionally closest
existing pattern, but it punts persistence/provenance/trust to the caller —
exactly the gap a signed-event relay fills (provenance, ordering, tamper
evidence) that a bare file directory cannot.

**OpenAI ChatGPT memory** —
[memory FAQ](https://help.openai.com/en/articles/8590148-memory-faq),
"Dreaming" blog (fetch blocked; snippets only). **Saved memories** (explicit,
editable) vs **reference chat history** (implicit mining, opaque).
"Dreaming" = offline/background consolidation, parallel to sleep-time
compute. Relevance: saved-vs-inferred maps to "verified/attributable claim"
vs "inferred pattern" — worth carrying into event kinds so provenance
strength is visible.

**Gemini** (snippets only): explicitly separates **context caching**
(KV-cache cost optimization) from **memory** (durable cross-session recall).
Terminology hygiene: caching and memory are different axes; a relay design
is squarely "memory."

### Research literature

Surveys:
[Agent-Memory paper list](https://github.com/Shichun-Liu/Agent-Memory-Paper-List);
["Always-On Agents"](https://arxiv.org/pdf/2606.30306);
["From Storage to Experience"](https://arxiv.org/pdf/2605.06716);
["Memory for Autonomous LLM Agents"](https://arxiv.org/html/2603.07670v1).
Converged taxonomy: **episodic / semantic / procedural**. Central design
axis across the field: **in-flow** capture vs **post-hoc** consolidation.
Open problems: scalability, accuracy-vs-abstraction, eval standardization,
continual learning.

Sleep/offline consolidation: Letta sleep-time; CMU/UMD offline-recurrence
line (["Do Language Models Need Sleep?"](https://toknow.ai/posts/do-language-models-need-sleep-offline-recurrence-long-context-reasoning/)).
Caveat: offline compute pays off most when future queries are *predictable*
— a real risk for open-ended coding-agent memory.

**Distillation vs raw fidelity — genuinely unsettled (most load-bearing
finding):**
- ["What Deserves Memory: Adaptive Memory Distillation"](https://arxiv.org/pdf/2508.03341)
  (Aug 2025): distilled/selective retention substantially beats raw replay.
- ["Verbatim Chunks Beat Extracted Artifacts"](https://arxiv.org/pdf/2601.00821)
  (Jan 2026): the opposite — verbatim chunks beat LLM-extracted summaries
  when tasks need precise factual grounding or multi-hop reasoning;
  extraction wins only under binding token budgets or genuine high-level
  synthesis.
- ["Memory is Reconstructed, Not Retrieved" (MRAgent)](https://arxiv.org/abs/2606.06036)
  (Jun 2026): against static retrieve-then-reason entirely — Cue-Tag-Content
  associative graph + active reconstruction at query time; +23% on
  LoCoMo/LongMemEval at lower cost.

Net: the field has **not** converged on "always distill." The defensible
design keeps the **raw signed event stream as permanent ground truth** and
treats **distilled memory as derived, regenerable, time-stamped/superseded**
— never destroying the raw layer. (Effectively Buzz's existing position;
supported by Mem0's write-time reconciliation and Graphiti's
invalidate-don't-delete.)

### Coding-agent specifics

- **Claude Code** ([sessions docs](https://code.claude.com/docs/en/sessions)):
  `/compact` = destructive in-place summarization of live context; `/resume`
  reloads by ID; separate agent-written auto-memory directory layered on
  human-authored CLAUDE.md/AGENTS.md.
- **Codex CLI** ([mem0 writeup](https://mem0.ai/blog/how-memory-works-in-codex-cli),
  [codex.danielvaughan.com](https://codex.danielvaughan.com/2026/05/01/codex-cli-memories-persistent-context-session-memory-ecosystem/)):
  no cross-session memory by default; `~/.codex/sessions/` transcripts are
  replay, not memory; AGENTS.md is the static layer; third-party MCP memory
  servers being bolted on — evidence of unmet demand.
- **Cursor** ([context management](https://datalakehousehub.com/blog/2026-03-context-management-cursor/),
  [indexing](https://towardsdatascience.com/how-cursor-actually-indexes-your-codebase/)):
  decouples "memory of the codebase" (continuously re-derived index, always
  fresh) from "memory of decisions/conversations" (accumulated, can go
  stale) — mirrors Buzz's Git-vs-relay two-stream split.
- **Devin** ([docs](https://docs.devin.ai/work-with-devin/advanced-capabilities)):
  **Knowledge** (facts/tips, auto-recalled) vs **Playbooks** (procedural
  templates), deliberately separate; sessions are disposable — everything
  durable must be **explicitly promoted**. Strong argument for explicit
  promotion over persist-by-default (cuts poisoning/staleness risk).
- **OpenHands** ([condensation](https://www.openhands.dev/blog/openhands-context-condensensation-for-more-efficient-ai-agents),
  [persistence](https://docs.openhands.dev/sdk/guides/convo-persistence)):
  domain-aware condenser preserving goal/progress/remaining-work/technical
  anchors; resumed conversations prefixed with a `<<RESUMED CONVERSATION>>`
  marker — explicit provenance boundary between live and reconstructed
  content.
- **Factory.ai** ([memory docs](https://docs.factory.ai/guides/power-user/memory-management)):
  org/user-level shared Memory + a Knowledge droid so one droid inherits a
  prior droid's context — multi-agent-shared memory, the shape Buzz has by
  construction.

### Design tensions

- **Staleness/supersession**
  ([design guide](https://hidekazu-konishi.com/entry/ai_agent_memory_design_guide.html)):
  TTL + retrieval decay + **active supersession at write time**; Graphiti's
  bi-temporal edges as the cited mechanism.
- **Provenance / verified fact vs attributable claim**
  (["From Agent Traces to Trust"](https://arxiv.org/html/2606.04990v1),
  ["When Does Belief-Based Agent Memory Help?"](https://arxiv.org/html/2606.22030)):
  provenance must capture semantic support/contradiction relations, not just
  who-wrote-when; pattern of surfacing `[unverified]` markers *into the
  prompt*; retrieved memory as hypothesis to re-check, staleness-weighted.
  Key: cryptographically "verified" (signed, attributable) is a different
  axis from "true/current" — expose both, never collapse into one score.
- **Memory poisoning** ([WorkOS](https://workos.com/blog/ai-agent-memory-poisoning);
  MINJA, NeurIPS 2025, 76.8% ASR evading sanitization; MemoryGraft, Dec
  2025; ["From Untrusted Input to Trusted Memory"](https://arxiv.org/pdf/2606.04329);
  [MemAudit](https://arxiv.org/pdf/2605.23723)): prompt injection is a
  session problem; memory poisoning is a *persistence* problem — "an
  instruction planted today executes weeks later." The strongest external
  argument for signed, attributable, policy-gated memory writes — a
  relay-native design is structurally positioned against this class in a way
  flat memory files are not (no per-write actor binding there).
- **Goodhart/self-reinforcing drift:** implied by MemoryGraft (agent
  canonizes a "successful" pattern that was the attack payload) and the
  predictability caveat; mitigations in the wild: Devin's explicit
  promotion, OpenHands' goal-preserving condenser, LangMem's tunable
  consolidation.

## II.2 Tool-call interception/augmentation at a middleware layer

### MCP-layer middleware

- **SEP-1763 "Interceptors for MCP"** —
  [issue](https://github.com/modelcontextprotocol/modelcontextprotocol/issues/1763)
  (Draft, Nov 2025), [reference impl](https://github.com/modelcontextprotocol/experimental-ext-interceptors),
  [WG charter](https://modelcontextprotocol.io/community/working-groups/interceptors).
  Two interceptor types: **validators** (pass/fail, parallel, never mutate)
  and **mutators** (sequential by priority, **replace the entire payload** —
  no patching). Trust-boundary ordering: outbound mutate→validate→send;
  inbound validate→mutate. Hooks cover `tools/call`, `resources/read`,
  `prompts/get`, `sampling/createMessage`, `elicitation/create`,
  `roots/list`, `llm/completion`, request/response phases, wildcards.
  JSON-RPC: `interceptors/list`, `interceptor/invoke`,
  `interceptor/executeChain`. Deployment: in-process, sidecar/gateway,
  hybrid. **Critical gap for Buzz's pattern:** no schema for a mutator to
  *append* supplementary content alongside the original result with
  provenance/attribution — mutators replace; `MutationResult.info` describes
  the transformation, not a labeled channel to the model. A
  companion-call/fan-out-with-attribution feature is a genuine extension
  beyond current scope — workaroundable as a relay-level convention or
  proposable upstream.
- **SEP-2133** (extensions framework) and **SEP-1766** (digest-pinned tool
  versioning + interceptor validation) —
  [PR 2133](https://github.com/modelcontextprotocol/modelcontextprotocol/pull/2133),
  issue 1766.
- **SEP-2567 "Sessionless MCP via Explicit State Handles"** —
  [spec](https://modelcontextprotocol.io/seps/2567-sessionless-mcp).
  Server-minted opaque handles instead of large inline content; documented
  lifetime; reauthorize on every use (possession ≠ access). Relevant if
  companion-search results are large: return a handle + preview, expand on
  demand.
- **Gateway ecosystem 2026** ([MintMCP](https://www.mintmcp.com/blog/enterprise-ai-infrastructure-mcp),
  [Cloudflare](https://blog.cloudflare.com/enterprise-mcp/),
  [MCP portals](https://developers.cloudflare.com/cloudflare-one/access-controls/ai-controls/mcp-portals/)):
  OAuth 2.1, per-key tool filtering, policy transforms (PII anonymization),
  audit logs with per-call metadata. `tools/list` merged across upstream
  servers — fan-out/merge precedent at catalog level, not result level. No
  standard model-visible provenance field found; provenance lives in logs.
- Traefik Hub / Moesif: identity/session enrichment for observability, not
  model-visible augmentation.

### Agent-harness hooks

- **Claude Code hooks** ([guide](https://code.claude.com/docs/en/hooks-guide)):
  `PreToolUse` can block/allow/ask/defer; `PostToolUse` injects
  `hookSpecificOutput.additionalContext` "as a system reminder read as plain
  text" — **unattributed as to source**. Multiple matching hooks: all run,
  most-restrictive decision wins, and "text from `additionalContext` is kept
  from every hook and passed together" — i.e. **Claude Code already merges
  companion results, but flatly concatenates without per-source labels**.
  `PostToolUse` cannot replace the original result, only append.
  `PostToolBatch` fires once after a parallel batch before the next model
  call — natural interception point for a companion result set. New hook
  types: `mcp_tool` (hook action is another MCP tool call) and `http` (POST
  event JSON to an external service, read back decision JSON) — plausible
  substrates for a relay-side companion-search hook.
- **Codex CLI hooks** ([usage](https://knightli.com/en/2026/06/11/codex-hooks-advanced-usage/),
  [reference](https://agenticcontrolplane.com/blog/codex-cli-hooks-reference)):
  shipped v0.114 (Mar 2026), `Stage::UnderDevelopment`, off by default;
  `PreToolUse` intercepts **Bash only** — no apply_patch/edit/MCP coverage.
- **LangChain/LangGraph middleware**
  ([custom middleware](https://docs.langchain.com/oss/python/langchain/middleware/custom),
  [blog](https://www.langchain.com/blog/how-middleware-lets-you-customize-your-agent-harness)):
  `wrap_tool_call(request, handler)` — call handler zero/one/many times;
  modify request or response. No multi-source merge/compose primitive; the
  wrapper owns merging.
- **NeMo Guardrails** ([repo](https://github.com/NVIDIA-NeMo/Guardrails)):
  execution rails check/filter tool IO; no attributed parallel augmentation.

### Augmented retrieval alongside native tools

- **Sourcegraph Cody**
  ([how Cody understands](https://sourcegraph.com/blog/how-cody-understands-your-codebase),
  [semantic search](https://sourcegraph.com/blog/semantic-code-search-what-it-is-and-how-it-works)):
  originally embeddings + code graph + rerank; **moved away from embeddings
  toward native keyword/structural search** as primary at production scale.
- **GitHub Copilot coding agent**
  ([changelog](https://github.blog/changelog/2026-03-17-copilot-coding-agent-works-faster-with-semantic-code-search/),
  [indexing docs](https://docs.github.com/copilot/concepts/indexing-repositories-for-copilot-chat)):
  hybrid merge: TF-IDF narrows to ~128 chunks, embeddings re-rank; remote
  pre-built semantic index + local targeted search of modified files, merged
  server-side **before the tool returns**. Measured: "2% less time, no
  change in quality" — augmentation helped marginally and safely, with the
  model never having to reconcile two raw sets.
- **"Why Grep Beat Embeddings in Our SWE-Bench Agent"** (Augment, via Jason
  Liu) — [post](https://jxnl.co/writing/2025/09/11/why-grep-beat-embeddings-in-our-swe-bench-agent-lessons-from-augment/):
  agent **persistence** (iterated grep) substituted for retrieval
  sophistication. Recommendation: "don't throw away your retrieval systems —
  **expose them as tools to agents**" (vs force-injection).
- **"Is Grep All You Need?"** (2026) —
  [arXiv:2605.15184](https://arxiv.org/html/2605.15184v1): across four
  harnesses on 116-question LongMemEval, **inline grep exceeds inline vector
  for every harness–model pair**, but file-based/handle delivery reshuffles
  the comparison — *delivery architecture matters as much as retrieval
  method*. Explicitly did **not** test combining both; no attribution or
  latency measurements. **The "shadow companion call merged with
  attribution" configuration has no published eval — white space.**
- Cursor/Windsurf: context engines exist; no documented per-item source
  provenance to the model.

### Risks and lessons

- **Unattributed merging is normalized, not solved.** Claude Code's own
  hooks concatenate multiple sources unlabeled. A relay that attributes
  (`source: relay_search`) would be ahead of practice.
- **Injection/tool poisoning:** OWASP LLM Top 10 #1; MCP tool poisoning
  (CVE-2025-54136; [OWASP](https://owasp.org/www-community/attacks/MCP_Tool_Poisoning),
  [TrueFoundry](https://www.truefoundry.com/blog/blog-mcp-tool-poisoning-gateway-defense),
  [MCP-ITP study](https://arxiv.org/pdf/2601.07395) — up to 72.8% ASR).
  Auto-injecting relay-history content the model didn't request widens the
  injection surface — the model has less reason to scrutinize content it
  didn't ask for.
- **Willison's lethal trifecta**
  ([post](https://simonwillison.net/2025/Jun/16/the-lethal-trifecta/)):
  private data + untrusted content + exfiltration path. A relay
  auto-appending from its private event store into a model that can publish
  externally reconstructs the trifecta at the middleware layer. Design
  check: the companion path must never pull content the current actor isn't
  authorized to read.
- **Cost/size:** community practice offloads outputs >~8k chars to files
  with preview + reference; pair companion results with SEP-2567 handles.
- **No published eval validates the exact pattern.** Closest evidence
  supports: (a) hybrid merge helps modestly when server-ranked before
  return; (b) delivery mechanism matters as much as method; (c) for
  small/structured corpora augmentation adds cost without benefit because
  persistent agents converge anyway. Two-attributed-result-sets-to-the-model
  is the paper this project would be writing.

## II.3 Entire.io plans and the session-leverage landscape

### Entire.io

- **Founder correction:** founded by **Thomas Dohmke** (ex-GitHub CEO,
  departed Aug 2025) — *not* Nat Friedman. Confirmed across
  [TechCrunch](https://techcrunch.com/2026/02/10/former-github-ceo-raises-record-60m-dev-tool-seed-round-at-300m-valuation/),
  [Madrona](https://www.madrona.com/the-ai-eras-developer-platform-why-we-invested-in-entire/),
  [Wikipedia](https://en.wikipedia.org/wiki/Thomas_Dohmke).
- **Funding/thesis:** $60M seed at $300M valuation (largest dev-tools seed),
  Felicis-led. Three pillars (stated, largely unshipped): Git-compatible
  database unifying code + intent + reasoning; "semantic reasoning layer"
  for multi-agent coordination/handoff; "AI-native SDLC."
  ([hello-entire-world](https://entire.io/blog/hello-entire-world))
- **Shipped beyond capture+resume:**
  - **Search** across sessions/prompts/transcripts/commits/source — web UI,
    CLI, and a Skills-based guide so **agents query past sessions while
    working** ([overview](https://docs.entire.io/guides/search/overview.md),
    [from-your-agent](https://docs.entire.io/guides/search/search-from-your-agent.md)).
  - **`entire review`** — multi-agent parallel review with a judge agent;
    configurable reviewer profiles
    ([CLI ref](https://docs.entire.io/cli-reference/review.md)).
  - **"Intent review" positioning** — pivot from diff-based to intent-based
    review: reviewers start with prompt/transcript/reasoning
    ([blog](https://entire.io/blog/the-entire-cli-how-it-works-and-where-its-headed)).
  - **Dispatches** — summarize recent agent work into shareable markdown
    (weekly updates, stakeholder briefs, re-orientation)
    ([docs](https://docs.entire.io/guides/dispatches/overview.md)).
  - **Attach Sessions** — retroactively link a session to a commit/checkpoint
    ([docs](https://docs.entire.io/guides/sessions/attach-sessions.md)).
  - **Cross-agent Skills** ([entireio/skills](https://github.com/entireio/skills))
    — session corpus feeding back into agent behavior.
  - **Audit/attribution for regulated industries** — "mapping every step
    from human prompt to agent response to final human refinement."
  - **Agent Hooks** ([blog](https://entire.io/blog/agent-hooks-the-integration-layer-between-entire-cli-and-your-agent))
    — lifecycle + git hooks binding session metadata to commits via
    `Entire-Checkpoint` trailer; "sessions versionable, traceable,
    reviewable like source code."
  - **Distributed Git hosting network**
    ([blog](https://entire.io/blog/an-entirely-new-git-hosting-network)) —
    ambition to disintermediate GitHub itself.
  - **Checkpoint remote / separate checkpoint repo** — org-wide session data
    as a distinct governed asset ([cli](https://github.com/entireio/cli)).
  - No public statements on training-data use, eval mining, or pricing.
- **Security posture:** best-effort redaction on write; shadow branches may
  hold unredacted data; public repo ⇒ public checkpoints unless routed to a
  separate remote.
- **Friction/skepticism:** HN launch (~611 pts/577 comments) core objection:
  "zero reason for a person to care about the checkpoints"
  ([thread](https://news.ycombinator.com/item?id=46977209)). Analyst
  ([Ry Walker](https://rywalker.com/research/entire)): valuation bridged
  entirely by roadmap; **capture layer commoditized** (replicated with
  `git notes` in 24h) — moat is the leverage layer; lock-in/orphaned-format
  risk; competitive collision with GitHub, CLI vendors, orchestration
  platforms. Mechanics criticism
  ([julien.danjou.info](https://julien.danjou.info/blog/how-entire-works-under-the-hood/)):
  shadow branch has **no retention policy** — every throwaway session
  accumulates forever.

### Commit ↔ session link elsewhere

- **GitHub Copilot coding agent** — every agent commit carries an
  **`Agent-Logs-Url` trailer** linking to the full session log; framed for
  review context and audit; plus org/enterprise structured audit-log events
  ([changelog](https://github.blog/changelog/2026-03-20-trace-any-copilot-coding-agent-commit-to-its-session-logs/),
  [docs](https://docs.github.com/en/copilot/how-tos/copilot-on-github/use-copilot-agents/manage-and-track-agents),
  [audit events](https://docs.github.com/en/copilot/reference/agentic-audit-log-events)).
  Same primitive as Entire's trailer, shipped by the platform that owns the
  repo host.
- **Sourcegraph Amp** ([manual](https://ampcode.com/manual)) — threads sync
  to ampcode.com; four visibility tiers; `amp threads share`; team `/feed`
  searchable; threads referenced as `@T-<id>` inside new prompts — **threads
  explicitly reusable as injectable context**, not just archival.
- **Devin** ([blog](https://cognition.com/blog/how-cognition-uses-devin-to-build-devin))
  — shareable session replays with rollback of files *and memory state*;
  **Session Insights** mines completed sessions into improved-prompt
  suggestions — clearest "mine sessions to improve future sessions" loop.
- **Factory.ai** — Knowledge droid as shared context store across a fleet
  ([guide](https://sidbharath.com/blog/factory-ai-guide/));
  community [droid-mem](https://github.com/vimal7370/droid-mem) compresses
  session observations and re-injects at next session start.

### Observability platforms (generic trace-corpus leverage)

Langfuse (raw traces) / LangSmith (clustered "Insights") / Braintrust (eval
datasets from traces) / W&B Weave (tracing + evals, coding-agent framing) /
Helicone (proxy tracing). [Kitaru](https://www.zenml.io/product/kitaru)
imports traces from all of them for replay-based evals — the "capture once,
build derivative products on top" shape Entire is attempting.
Academic corpus: **SWE-chat**, ~6,000 real coding-agent sessions across
200+ repos ([arXiv:2604.20779](https://arxiv.org/pdf/2604.20779)) — session
corpora already treated as research/eval assets.

### Agent's relevance synthesis

Buzz's signed-relay model already provides what Entire builds bespoke
plumbing for: durable addressable queryable log (vs shadow-branch
no-retention bloat), real-time fan-out, NIP-29 scoping. Validated next
builds, ordered by what actually shipped elsewhere: (1) commit↔session
trailer/link — cheap, table stakes now; (2) intent-based review tooling on
signed events; (3) session→context reinjection — **the one pattern every
player converges on**; (4) explicit retention/redaction discipline — a live,
named failure mode.

## II.4 The `amas` predecessor project (/Users/brian/Desktop/migration-src/amas)

### What amas is

`amas` ("Agiterra Mas") is a **production multi-agent orchestration
toolkit** built by Brian (lead agent "keystone") plus a crew of AI actors
(levain, kiln, loom, maat, fondant, and many retired seats), coordinating
long-lived AI engineering teams across Claude Code and Codex. Source:
`agiterra/amas-redux` (git bundle in the kit: 3,277 refs, 1,622 commits on
`main`, 2026-05-20 → 2026-08-07). The Desktop directory is a
**decommission/handoff kit** (built 2026-08-14 as the Mac was being wiped):
git bundle, memory store (`state/amas-memory.tgz`, `state/crews.db` incl.
Ed25519 private keys), **1,004 memory claims exported as flat files**
(`no-amas/`), personas, configs. Orientation docs:
`NEW_MAC_AGENT_HANDOFF.md`, `README_FIRST.md`.

Maturity: "Alpha, in daily use" (`README.md:296`) — ~3 months of real use,
real merges (10 PRs in one evening per `config/roles.yaml:7-8`), own
benchmark harness, integrity tests, postmortems. Stack: Bun/TypeScript
monorepo, SQLite, git-as-database memory store, Ed25519 signing throughout,
cmux/tmux sessions, MCP delivery; Entire used externally then made opt-in in
favor of amas's own `packages/capture`.

**Not "similar prior work" — the direct ancestor with a ratified absorption
plan.** `docs/planning/buzz-one-way-absorption-2026-08-01.md` (duplicated in
the kit) is a Brian+Andy-ratified plan making Buzz the sole canonical
source, absorbing amas-redux, Portage, Hive, and Cairn as `to_import/`
submodules with `bring-in` default disposition. It states the authoritative
copy lives at `agiterra/buzz/docs/absorption/PLAN.md` with companions
`ABSORPTION-LEDGER.md`, `IMPORT-MAP.md`, `UPSTREAM-MAP.md`, `SYSTEMS.md`,
`OWNERSHIP.md`, `CAPABILITY-CENSUS.md`. Keystone decision records
(`no-amas/actor/keystone/decision-input-buzz-block-buzz-as-an-amas-surface-source`,
`decision-input-informed-dissent-swap-cmux-for-buzz-brian`) record Brian
already choosing Buzz as amas's session surface.

### Artifact inventory (for later reading)

- `docs/CONCEPTS.md`, `docs/MEMORY-SYSTEM-OVERVIEW.md` (**richest single
  document** — file:line-cited audit of the memory system),
  `docs/REFLECTION.md`, `docs/FEDERATION.md`, `docs/ADAPTERS.md`,
  `docs/PERSONAS.md`, `docs/BRIDGE-SEND-CUSTODY.md`, `docs/FLOWS.md`.
- `docs/planning/` (~65 files): `trust-model.md`, `proof-gated-memory.md`,
  `prospective-memory.md`, `recall-benchmark-locomo.md`,
  `recall-benchmark-longmemeval.md`, `recall-benchmark-results.md`,
  `memory-write-governance.md`, `memory-integrity-battery.md`,
  `context-economy-2026-07-10.md`, `sessions-first-class-buzz-2026-07-28.md`,
  `buzz-one-way-absorption-2026-08-01.md`,
  `absorption-donor-field-notes-2026-08-01.md`,
  `roles-yaml-custody-contract.md`, `orchestration-economics.md`,
  `self-observability.md`, `north-star-vision-gpt56sol-2026-07-11.md`.
- `docs/archive/2026-06/`, `2026-07/` (~90 dated postmortems/RCAs/findings).
- `config/roles.yaml` (kit, 196 lines): ratified role/capability contract,
  tiered ceremony, cross-model gate, terminal-verdict vocabulary
  (`APPROVE`/`BLOCK`/`CONFIRMED`/`NOT-REFUTED`), honesty rules ("a status is
  `verified` only if its verification command RAN GREEN ON THIS HOST").
- `no-amas/actor/<name>/*`: 1,004 claims with YAML frontmatter (keystone
  ~250, kiln ~40, levain ~50, loom ~65, fondant journal ~90);
  `manifests/inventory.txt` (1,751 lines) is the full listing.
- `no-amas/project/*`: wire architecture notes incl.
  `wire/wire-architecture` (enrichment pipeline),
  `wire/wire-session-lifecycle-spec`,
  `wire/project-wire-replay-cursor-architecture`.
- `packages/session-bridge/README.md`: the Buzz coding-session protocol's
  direct ancestor.

### Overlap with the Buzz effort

1. **Session protocol ancestry:** session-bridge already defines
   `buzz-coding-session-{metadata,lifecycle-command,lifecycle-receipt,transcript,provider-catalog}/v1`,
   the `{driver, instanceId, sessionId, generation}` target, kinds
   **44221–44225**, durable `eventSeq`/`revision`, crash-safe turn delivery
   with `clientUserMessageId` reconciliation, and the explicit **"projection
   ≠ command authority"** split.
2. **Claim-status lattice, field-tested:** `packages/memory/src/claim.ts` —
   `status: claim|proven|refuted|stale` (mechanical truth, prover-only)
   crossed with `curation_state: uncurated|pending|vouched|rejected`
   (endorsement; specced in `trust-model.md` but **never added to the
   schema** — see finding 2 below). Write-time invariant: no actor can mint
   `proven` via direct write (`claim.ts:462-480`). Ordered recall cascade:
   `refuted/stale` > `proven` > `vouched` > `unverified claim`
   (`trust-model.md §4`).
3. **First-turn briefs:** "launch inheritance"
   (`apps/amas/src/launch-memory.ts`, `MEMORY-SYSTEM-OVERVIEW.md §4.3`) — a
   continuity breadcrumb read unconditionally + ranked recall at boot, with
   a measured negative result (finding 3).
4. **Custody-time checkpoints:** `docs/REFLECTION.md` — propose-only
   reflection; human merges; never auto-applied ("Auto-applied personality
   rewrites compound sycophancy and silently regress capability").
5. **Relay-side augmentation precedent:** wire enrichment pipeline
   (`no-amas/project/wire/wire-architecture:20-42`) — per-channel ordered
   stages, each seeing prior stages' output, returning scored
   `EnrichmentResult`s.
6. **Broker → relay ancestry:** signed HTTP+SSE broker, Ed25519;
   `plugins/channel-claude-code`/`channel-codex` ≈ `buzz-acp`/`buzz-dev-mcp`.
7. **Gap in the donor:** commit↔session linking was *not* solved in amas
   (delegated to Entire, treated as opt-in/secondary) — Addendum D's
   Git-join is new ground.

### Top reusable findings (with citations)

1. **Two-axis trust model is the crux** (`trust-model.md:23-33`,
   `MEMORY-SYSTEM-OVERVIEW.md §2.1–2.2`): truth (mechanically
   proven/refuted) and endorsement (human-vouched) are orthogonal — "vouched
   ≠ fact." Recall cascade: refuted/stale always override; proven +
   read-time reverify = fact; vouched = "endorsed craft," never asserted as
   fact; else "unverified claim says…". **Signatures over vouching must bind
   placement, not just content**, or a vouch replays into a different tier
   (`trust-model.md:47-56`, MUST-FIX from adversarial review).
2. **A governance system can be fully specified and still not close the
   loop** (`MEMORY-SYSTEM-OVERVIEW.md §11`): `curation_state` was designed,
   architect-validated, documented as done — never added to the schema;
   "vouching" was in practice an unaudited git merge. Track build status per
   axis explicitly (IMPLEMENTED vs PLANNED/SPECCED).
3. **Measured negative result on inheritance** (§4.3): identical code,
   opposite outcomes — fresh `toolsmith` inherited its 2 trusted claims;
   fresh `lead` inherited **nothing** because the curator's vouched claims
   were still `probationary`. The trust-state gate, not the plumbing, blocks
   curated craft from reaching fresh agents.
4. **Recall benchmark methodology + result**
   (`recall-benchmark-results.md`): Config A (FTS5+RRF) P@1=0.633; Config D
   (real embedder, BGE-small) P@1=0.767 — lift **entirely in the paraphrase
   category** (0.300→0.600), near-zero elsewhere; Config B (lexical-hash
   fake embedder) adds zero — clean negative control. Production caps
   (≤5 hits/query, ≤3/tier, ≤8,000 chars) measurably reduce precision vs raw
   fusion. LoCoMo (Apache-2.0) and LongMemEval (MIT) evaluated as vendorable
   corpora, never integrated (`:138-163`) — ready-made task list. Replicate
   the A/B/C/D methodology for Buzz's recall/brief evaluation.
5. **Prospective/cue-conditioned memory** (`prospective-memory.md`):
   similarity retrieval systematically fails to resurface deferred decisions
   ("stop the bus"). Fix: `prospective` block in claim frontmatter —
   `cue.predicate` (machine-checkable), `cue.terms` (lexical reactivation),
   `policy: once|cooldown`; two fire routes — "focal" (cue terms ride
   existing recall, zero infra) and "monitoring" (cheap boot-time predicate
   check, no daemon). Reusable for "superseded/deferred, revisit-when-X"
   claims in the citator.
6. **Context economy** (`context-economy-2026-07-10.md`): unconditional
   per-turn recall injected 125–170 tokens on *every* prompt with no
   relevance floor (weather probe still returned memories); tool output was
   the single largest token lever, dwarfing persona+memory; est. 15–30%
   total token reduction from output budgeting + relevance floors + dedup
   without weakening any gate.
7. **Ceremony priced by blast radius** (`config/roles.yaml:5-31`): tier-0/1
   vs tier-2 review ceremony; measured: 10 PRs/evening under tiered ceremony
   vs 1 row/3 days under uniform. Spin-breaker: "three consecutive rounds
   that REFRAME rather than refine = one missing input. STOP."
8. **Cross-model-family gating** (`roles.yaml:26-28`,
   `context-economy:41-43`): "different model" ≠ "different model *family*"
   (self-preference bias is a top verifier failure mode); enforcement
   existed (`CrossModelViolationError`) but wiring to make it load-bearing
   was still a gap.
9. **Absorption plan is the bridge artifact:** check `agiterra/buzz` (or
   wherever it landed relative to this fork) for `docs/absorption/PLAN.md` +
   the five census documents **before re-deriving anything** — they may
   contain finished per-capability dispositions.

---

# Part III — Consolidated synthesis and next actions

## III.1 What the four reports jointly establish

1. **The dual-stream architecture is independently validated from three
   directions.** The memory field converged on capture-raw-in-flow /
   distill-post-hoc / never-destroy-the-raw-layer (Letta sleep-time,
   LangMem, OpenAI dreaming) — the transcript/checkpoint split. The
   distill-vs-verbatim question is genuinely unsettled (Aug 2025 vs Jan 2026
   papers contradict), which vindicates keeping signed raw events as ground
   truth with checkpoints as derived, superseded artifacts. Graphiti's
   bi-temporal invalidate-don't-delete is the citator's currency semantics
   as prior art.

2. **Memory poisoning is the strongest external argument for the
   signed-relay design.** MINJA (76.8% ASR evading sanitization) and
   MemoryGraft demonstrate planted memories firing weeks later; the
   literature's prescription — signed, attributable writes with explicit
   actors and policy gates — describes the relay. Flat memory files
   (Anthropic memory tool, Devin Knowledge) have no per-write actor binding.
   This is the security story, not just the continuity story.

3. **The relay companion-call pattern sits in genuine white space.** MCP's
   draft interceptor spec (SEP-1763) can validate or replace but cannot
   *append with provenance* — a proposable upstream extension. Claude Code
   merges hook context but unattributed — labeling the companion result
   beats current practice. No published eval tests two separately-attributed
   result sets handed to a model. Design cautions from day one: (a) preview
   + server-minted handle (SEP-2567), not full inlining; (b) authority-scope
   the companion search to the current actor's read rights or the lethal
   trifecta is reconstructed at the middleware layer; (c) hold both shapes —
   auto-companion vs advertised `relay_search` tool (Augment's
   expose-as-tool lesson; amas's paraphrase-only embedding lift) — and let
   the amas-style A/B/C/D benchmark decide.

4. **amas reframes the effort as a second iteration.** Kinds 44221–44225 and
   projection≠authority were born in `packages/session-bridge`; the
   claim/proven/refuted/stale × curation lattice is the citator,
   field-tested, with two hard lessons attached (spec-drift: curation axis
   documented as done but never in the schema; inheritance: vouched claims
   never reached fresh agents). Prospective/cue-conditioned memory is a
   novel addition nothing in the web survey had. Commit↔session joining is
   the one thing amas did *not* solve — Addendum D's Git-join is new ground.

5. **The commit↔session link is now table stakes; the citator is the
   differentiated part.** GitHub ships `Agent-Logs-Url` trailers on every
   Copilot-agent commit; Entire's `Entire-Checkpoint` trailer is the same
   primitive; Entire's capture layer was commoditized within 24 hours of
   launch. The leverage everyone converges on is **session→context
   reinjection** (Amp `@T-id`, Devin Session Insights, Factory Knowledge
   droid) — which is exactly Buzz's continuity program. Entire's named
   failure modes (no retention policy; best-effort redaction) are design
   inputs: Buzz should decide retention and redaction explicitly, not
   inherit them.

## III.2 Recommended next actions (in order)

1. **Find the absorption documents.** Locate `docs/absorption/PLAN.md` +
   `ABSORPTION-LEDGER.md`, `IMPORT-MAP.md`, `UPSTREAM-MAP.md`, `SYSTEMS.md`,
   `OWNERSHIP.md`, `CAPABILITY-CENSUS.md` (stated home: `agiterra/buzz`)
   before re-deriving any amas capability disposition.
2. **Port the two-axis trust model into the citator spec explicitly** —
   mechanical truth vs endorsement as orthogonal axes; amas's ordered recall
   cascade; the placement-binding signature MUST-FIX; the two failure
   lessons (schema drift, inheritance gate) as cautionary notes with per-axis
   IMPLEMENTED/SPECCED status tags.
3. **Prototype the companion call as a labeled, handle-backed,
   authority-scoped augmentation**; evaluate auto-inject vs advertised tool
   with the amas A/B/C/D methodology (LoCoMo / LongMemEval are vendorable,
   already licensed-checked in `recall-benchmark-results.md`).
4. **Ship the commit↔session link early** (it is table stakes) as the
   substrate for intent-review and Dispatches-style leverage — Buzz already
   holds the signed events those features need; the witnessed-transition
   design (Part I, D.6) is the fork-native form.
5. **Adopt prospective/cue-conditioned claims** into the checkpoint/citator
   design for "deferred — revisit when X" decisions.
6. **Set explicit retention and redaction policy** for all new kinds
   (44233/44234 especially), gated on the report's §9.1 publication ruling.

## III.3 Open threads for the next session

- Addendum D is a draft in this document only — decide whether it graduates
  into `FABLE_SESSION_CONTINUITY_RESEARCH_REPORT.md` as a real Addendum D.
- Kind numbers 44233/44234 are provisional — confirm against
  `buzz-core/src/kind.rs` at integration time.
- The Entire founder correction (Dohmke, not Friedman) should propagate to
  any notes that repeated the earlier guess.
- The amas kit contains **Ed25519 private keys** (`state/crews.db`) — handle
  the migration kit as sensitive material during any absorption work.
- P1 still has never run (ledger §2.10) and remains the gate on the entire
  seed/checkpoint/commit-language track.

---

# Part IV — The Pulse convergence (Sol's synthesis + Fable's refinements)

*Added 2026-08-19 after Andy's Project Pulse handoff
(`PROJECT_PULSE_HANDOFF.md`, design-only, no code) and Sol's reading of it
against Part I. Sol's synthesis is recorded first; Fable's assessment and
refinements follow. Status: agreed direction between the two agent readings;
not yet reviewed with Andy.*

## IV.1 Sol's synthesis (recorded)

**The two documents meet at the project digest (39011) — not at the
summarizer (44242).** Dual-stream establishes the evidence; Pulse projects
it into "who is doing what, what materially changed, and should I wait,
consult, or proceed?" **Pulse becomes the human-and-agent-facing citator for
active project work.**

The combined model:

| Coordination question | Source | Honest interpretation |
| --- | --- | --- |
| What do they intend? | 44240 pulse entry | Author-signed claim |
| What have they been doing? | 44242 over 44225/44241 | Model-generated, relay-attested summary |
| What actually changed? | 44233 Git transition | Witnessed material change |
| Did it pass a check? | 44234 check-at-tree | Mechanically observed result |
| Is it still present/current? | Citator projection | Derived `asOf` judgment |
| Wait / consult / proceed? | 39011 digest | Client policy over all the above |

Why they need each other: **Pulse solves the thesis's product problem**
(the evidence architecture gets immediate leverage — coordination between
simultaneous sessions — before any future execution needs rehydration; a
44233 transition also updates the project view immediately instead of
waiting up to 5 minutes for a model summary). **The thesis solves Pulse's
truth problem** (a rolling summary can say "implementing idempotency keys"
but cannot prove anything changed, landed, passed, or remains; the
transition/check pair supplies exactly those facts, and the citator computes
currency at read time instead of freezing it into stored data).

**The three-layer active-work card** — every stream renders three visibly
separate layers: (1) *claimed scope* (44240 prose), (2) *automatically
inferred activity* ("Automatic summary · <model> · 2m ago"), (3) *witnessed
results* ("Observed commit `abc123` touching `pool.rs`; tests passed at tree
`def456`"). Attribution language per D.6.3: "observed during this session,"
never "created by this agent."

**Sol's five integration changes to the Pulse contract (before the
automatic phases):**

1. Add witnessed evidence to 39011 — per stream: recent 44233 transitions,
   related 44234 checks, citator currency stamped `asOf`. Now shows
   coordination-relevant evidence; History remains the exhaustive Git feed.
2. **Make summary provenance dereferenceable** — 44242's proposed `source`
   is only event/token counts: accounting, not provenance. It needs exact
   source event IDs, sequence coverage, or a content-addressed source
   manifest (the Addendum A `coverage.eventIdsDigest` pattern), or an agent
   cannot inspect what supports the summary.
3. Optional evidence citations on 44240 — a milestone can cite commit OIDs,
   transition/check events, or a tree OID; prose stays an attributable
   claim; citations let the citator show which parts have material support.
4. Never flatten evidence into the model summary — commit subjects remain
   bounded verbatim claims attached to OIDs (projection-time commit
   summarization would violate the proven-facts principle); 44242
   summarizes conversation, 44233/44234 stand beside it.
5. Reuse the existing session→project join (`sessionRef → projectRef` via
   44223) for the Git kinds; no new project association. No kind collision:
   Pulse reserves 44231–44239 headroom; the thesis proposes 44233/44234.

**Sol's two tensions:** *Retention* — 44241 cannot honestly be both
regenerable-memory source and deleted-after-14-days material; choose per
stream. *Authorization* — 44225 sources may be channel-scoped while 44242
summaries are project-member-readable; unless memberships are identical,
summarization widens access. Proposed rule: **a derived summary may never
have broader read authority than its source set.**

## IV.2 Fable's assessment and refinements

**Endorsed.** The synthesis improves on Part I's own framing: Addendum D
defined the citator as a read-time projection but gave it no product
surface; Sol supplies the surface, and the consequence is a unification —
**the first-turn brief's Changes chapter (vertical, successor-facing) and
the 39011 digest (horizontal, peer-facing) are two renderings of one
projection layer.** Build the citator once; feed both. This prevents two
divergent implementations of currency logic that could disagree.

**Refinement 1 — two grades of citator (required for honesty).** The digest
is relay-computed and the relay has no repository: it cannot compute *live*
currency, only fold **witnessed** ref state as of the last 44233 it saw.
Make the distinction explicit:

- **Relay-grade** (feeds 39011): currency over witnessed transitions only,
  stamped "as of last witnessed transition at `<t>`." Can lag; says so —
  carried in Pulse's existing freshness header.
- **Workspace-grade** (feeds the brief): computed provider-side, where
  `context_projector.rs` already runs with repo access — live refs, live
  dirty state.

Same evidence model and rules, two disclosed honesty grades. Without this
the digest would present stale containment as current — the exact
frozen-but-current failure the ledger has recorded twice (§2.4 and the
`completeAsOf` lesson).

**Refinement 2 — resolve the retention tension with one derived rule:**
**a summary's evidence class equals its weakest surviving source.**
Coding-session summaries stay evidence-backed (44225 is permanent); an
ACP-window summary whose 44241 source expired is classified *at read time*
as "operational summary — source expired" and is never admitted into
continuity evidence. No per-summary bookkeeping; honors both the storage
pragmatism and the raw-layer-is-ground-truth principle (Part II.1).

**Refinement 3 — promote the authorization rule to a fabric-wide law:**
**derivation never widens read authority — derived content inherits the
intersection of its sources' ACLs.** The same law already appears as the
companion-call scoping requirement (Part II.2 risks) and Addendum B.4's
snapshot-sharing caution; state it once, cite it thrice.

**Additional intersections recorded from the Pulse review (Fable,
2026-08-19, pre-Sol):** pulse entries are the *prospective* counterpart of
commit subjects (intent claims vs materialized claims) and enter the same
claim lattice; the claimed-vs-actually-touched ratio (44240 `codeAreas` vs
44233 changed paths) is the natural anti-Goodhart audit for scope-squatting
once agents learn peers read the pulse; the digest injection at session
birth is cross-agent content in the system prompt and needs
evidence-not-instructions framing plus a describe-don't-obey rule in the
summarizer prompt (memory-poisoning findings, Part II.1); `buzz-summarize`
is the infrastructure substrate the relay companion-call idea (Part II.2)
was waiting for; 44242 vs 44231 are different provenance classes
(post-hoc relay-attested vs custody-time self-attested) and the brief must
label them differently if both ever feed it.

**Open with Andy:** the five contract changes and two tensions above map
onto his open questions 1 and 3; the two-grade citator belongs in the
39011/freshness-header design; nothing here blocks his Phase 1 (44240 +
CLI + digest injection), which is pure claims-surface and needs no Git-join
machinery.
