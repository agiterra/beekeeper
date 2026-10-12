# Headless/extensibility clean-break candidate — 2026-10-12

Brian's direction is a clean break from the original desktop executor. Fresh
Create/setup and administration use shared headless management; the host/provider
owns execution. Desktop supplies UI, signing, Keychain custody and bootstrap
adapters. There is no automatic legacy import, native artifact conversion,
handoff, desktop executor or fallback. Retained old data is not automatically
executed; tracked live or unproved legacy conflicts are refused. Remote provider
capabilities, custom protected ACP configuration and Solo sessions remain.

The governing plan, map, ledger391–392 and detailed evidence are on agents-topic
`work/headless-extensibility-evidence`, under
`plans/BEEKEEPER_HEADLESS_EXTENSIBILITY_PLAN.md` and
`plans/archive/2026-10-11-headless-extensibility/slice-e/`.
See [clean setup and recovery](agent-recovery.md) and
[host administration](agent-host.md) for current product boundaries.

Fresh isolated CLI/native backend Create, ordinary work with the client closed,
reopen without duplicate execution, strict modern native restart/resume,
relay-only administration, cumulative Stop, Delete/replay and a newly defined
hook have recorded runtime evidence. Final full `just ci` passed on code commit
`b7ad812e2`; whole affected desktop smoke passed1457 tests with one skip.
Independent source/delta reviews closed their findings, and the evidence audit
verified144 signed events with25 correlation checks. Live fixture binary hashes
and later focused HTTP/custom-selector checks are recorded separately.

Shared Delete retires managed execution and removes the exact definition. It
explicitly retains executor custody for recovery; identity archiving is not a
general relay/repository permission ban or key erasure. Native desktop Delete
completed shared retirement but reported client Keychain cleanup incomplete in
the isolated home. No full client-cleanup or native GUI/GUI Quit pass is claimed;
the GUI tool pipe is unavailable. Windows, Linux/release-profile, other-vendor
and cross-account runtime acceptance remain unproved. Historical64477/8e
migration uncertainty/backups stay unchanged, failed/uncertain and outside the
revised release requirements.

Existing Review/Action/GitRef proof behavior remains software-specific; no
general acceptance policy engine or universal Git workflow requirement was
added. Desktop-local embedded Mesh model lifetime remains an external model
dependency; the Codex evidence does not prove it headless. Topic publication is
for review: no main merge, production install/deploy or Brian live-agent
migration has occurred.
