# Final CI recovery — 2026-09-13

Scope: read-only recovery of the completed `just ci` transcript at
`../review-2026-09-13-final-ci-sol/just-ci.log` for
`work/steering-integration-astra` at
`1669550d27cfbaaf313e5f3d15b4f3af9f54a7b2`, plus a fresh read of both
published `main` refs. This recovery did not rerun CI, commit, push, install or
change product source. This report is its only edit.

## Verdict

The saved transcript contains every command in the candidate's `just ci`
dependency graph through the final `mobile-test`, and every leg completed
successfully. The last line is Flutter `+2011: All tests passed!`. There is no
missing recipe leg and no need to repeat this same twelve-minute gate merely
to recover its result.

The transcript does not contain the wrapper shell's numeric exit status. It
does contain stronger command-flow evidence than a partial tail: `just` reached
each successive dependency, all test summaries report zero failures, both
builds completed, and the last dependency printed its success terminator.
There was no CI process running when recovery was checked. The log is 29,607
lines and 2,669,886 bytes, created at `2026-09-13T10:12:25-0400` and last
written at `2026-09-13T10:24:57-0400`; its SHA-256 is
`437846e04c8e5358d39b34dbb7b3840b75fa342fd58544a2478ca687d9c6e97e`.

## Recipe reconciliation

The candidate `justfile` defines:

```text
ci: check test-unit desktop-test desktop-build desktop-tauri-check
    desktop-tauri-test web-test web-build mobile-test

check: fmt-check clippy desktop-check desktop-tauri-fmt-check
       desktop-tauri-clippy web-check web-test mobile-check file-size-check
       current-state-check ignore-reasons-check autodeploy-test
       sidecar-parity-check
```

`just` runs the shared `web-test` dependency once. The transcript accounts for
the graph in that order:

| Recipe leg | Transcript evidence | Result |
| --- | --- | --- |
| `fmt-check`, workspace `clippy` | lines 1–24; clippy finishes the dev profile | pass |
| `desktop-check` | lines 25–222; 3 warnings and 6 infos, no errors, then px/pubkey/E2E-registration checks complete | pass |
| desktop Tauri fmt and clippy | lines 223–231; clippy finishes | pass |
| `web-check` | lines 232–235; 112 files, no fixes | pass |
| `web-test` | lines 236–421; 176 passed, 0 failed | pass |
| `mobile-check` | lines 422–529; 536 files formatted with 0 changes; analyzer says `No issues found!` | pass |
| file-size checks | lines 530–565; nine policy tests and desktop/web/mobile scans reach the next recipe | pass |
| current-state size checks | lines 566–583; seven policy tests pass and the map is 171/300 lines, 13,838/24,000 bytes | pass |
| ignored-test ratchet | lines 584–585; 253 bare ignores, at baseline | pass |
| autodeploy checks | lines 586–591; config, behavior and Woodpecker path-filter contracts explicitly pass | pass |
| sidecar parity | lines 592–593; all eight sidecars agree | pass |
| `test-unit` | lines 594–10454; relay-key bootstrap passes, the workspace run ends `All tests passed!`; summed Rust summaries are 7,340 passed, 0 failed | pass |
| `desktop-test` | lines 10455–20904; 9,355 passed, 0 failed | pass |
| `desktop-build` | lines 20905–21428; Vite finishes in 2.01s | pass |
| `desktop-tauri-check` | lines 21429–21435; cargo check finishes | pass |
| `desktop-tauri-test` | lines 21436–24912; summed Rust summaries are 3,353 passed, 0 failed, including 3,252 library tests | pass |
| `web-build` | lines 24914–24937; Vite finishes in 408ms | pass |
| `mobile-test` | lines 24938–29607; final result is 2,011 passed | pass |

The Rust summaries across `test-unit` and `desktop-tauri-test` total 10,693
passed and zero failed. Diagnostic strings containing words such as `error`
occur inside negative-path test names and expected test output. The desktop
Biome warnings/infos and Vite chunk-size notices are non-fatal and are followed
by later recipe legs.

## Relation to focused verification

The earlier
`../review-2026-09-13-combined-verification-sol/verification-report.md`
records TypeScript, scoped Biome, size/px checks and a fresh E2E build as green.
Its selected browser run was 44 passed and one failed; the isolated repeat
failed the same dense-history observation threshold. The accepted landing
limit therefore remains exactly the known dense-history result (286/450 in
the matrix and 336/450 in isolation). The final CI transcript adds a complete
green repository gate; it does not turn that browser limitation green. Brian
explicitly accepted landing with that limitation in
`2026-09-13-startup-smoke-corrections.md`.

## Exact-tree and remote status

The CI ran from the integration worktree whose HEAD is `1669550d2`, with the
startup corrections still in its working tree. At recovery time the worktree
had 22 tracked modifications and five untracked entries. Three untracked
entries are the pre-existing cache links `target`, `desktop/src-tauri/target`
and `node_modules`; the other two are the startup-readiness test and startup
report. The current binary tracked diff hashes to
`0973d80d06242441eeae3b695ce0f012ba373099b6fc199cf2affef8882dede4`;
the untracked readiness test hashes to
`fb92909945ec47f2aecedffd1196393051a0edf56501279d190be807930956a5`.
Those hashes describe the recovered working tree now; the CI transcript did
not itself record a pre/post tree hash, so it cannot independently prove that
identity.

At `2026-09-13T12:04:34-0400`, after activating Hermit and setting
`GIT_TERMINAL_PROMPT=0`, the configured credential helper allowed a fresh
relay fetch. Both the relay and GitHub reported `main` at
`343ea8bd9b56427e665d79b608ab491ce1105c46`. That commit is Andy's desktop
redaction fix on top of the integration base `9aebb1262`; candidate HEAD
`1669550d2` is not its ancestor. Andy's product paths do not overlap the
candidate's dirty product paths, but both sides edit `docs/CURRENT_STATE.md`
and `docs/SESSION_STATE.md`.

The candidate is CI-ready but not yet push-ready under the repository's
linear-history rule. The finalizer must preserve and commit the dirty
candidate, advance local `main` to `343ea8bd9`, rebase the topic commit(s) with
signoff onto that local `main`, reconcile the two documentation overlaps, and
then push through the relay. The recovered CI is complete evidence for the
pre-rebase candidate. Because published `main` advanced, it is not evidence
for the exact post-rebase tree; that is the only landing gap found by this
recovery.
