# Memory Explorer experiment

Default-off desktop feature `memory-explorer`, entered through Artifacts → Explore.
The existing editor, drafts and previews keep their existing boundaries.

The native commands capture a process-local authorized snapshot, read regular UTF-8
git blobs at that immutable commit (4 MiB each), and release its capability on
unmount. Every blob request rechecks community, identity, project and source.
The reader owns a disposable worker and in-memory index (256 documents / 16 MiB).
Archives not reached by explicit links stay visible and load on demand. A budget
stop is disclosed; loading another file can evict the earlier index.

The generic parser handles GFM positions, headings, reference links and local
anchors. The optional `adapters/beekeeper.ts` enrichment requires the canonical
map and ledger in the tree; it recognizes finding blocks and explicit SV rows.
Connections retain their basis, source line and unresolved/candidate targets.
Statuses remain literal source claims. Refresh swaps snapshots; it does not
update the text someone is reading when a repository ref notification arrives.

Focused checks (activate Hermit at the repository root first):

- `cargo test --manifest-path desktop/src-tauri/Cargo.toml memory_explorer --lib`
- In `desktop`: `node --import ./test-loader.mjs --experimental-strip-types --test src/features/memory-explorer/parseMarkdown.test.mjs`
- In `desktop`: `pnpm build:e2e`, then `pnpm exec playwright test --project=smoke tests/e2e/memory-explorer.spec.ts tests/e2e/project-agents-repo.spec.ts --workers=1`
  The traversal spec reads actual agents-repository bytes at `182304e`, but its
  Tauri transport is mocked. Set `MEMORY_EXPLORER_AGENTS_REPO` if that repository
  is not the sibling `../../agiterra-beekeeper-agents` relative to `desktop`.

Removal: delete this directory, native `managed_agents/memory_explorer.rs`, its
module and command registrations, the three `memory_explorer_*` commands in
`commands/agents_repo.rs`, the lazy route and Files entry, the flag, the two GFM
direct dependencies, the worker decoder resolver in `vite.config.ts`, and the
Explorer mock handlers/spec/Playwright registration. No authored document,
relay schema or event cleanup is needed. Remove the UI alongside the flag:
unknown flags are treated as stable/enabled by the shared feature mechanism.
