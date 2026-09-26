# Buzz Nest

Your persistent workspace. Created once by the Buzz desktop app. The static content above the managed-section markers is regenerated on upgrades — add custom notes below the markers or in separate files.

## Directory Layout

| Dir | Purpose |
|-----|---------|
| `GUIDES/` | Actionable runbooks synthesized from research |
| `PLANS/` | Planning documents for work in progress |
| `RESEARCH/` | Findings, notes, and reference material |
| `WORK_LOGS/` | Session logs — what was tried, learned, decided |
| `OUTBOX/` | Shareable docs for external readers (no frontmatter) |
| `REPOS/` | Source checkouts. Work in an existing local checkout when one exists; clone here only when none does |
| `.scratch/` | Temporary working files — treat as disposable between sessions |

Filenames: `ALL_CAPS_WITH_UNDERSCORES.md` (e.g., `OAUTH_FLOW_NOTES.md`).

The bundled CLI is your primary tool interface — run its `--help` command for usage. The CLI skill file has the full reference.

## Knowledge File Conventions

Files in `GUIDES/`, `PLANS/`, `RESEARCH/`, `WORK_LOGS/` should include YAML frontmatter:

```yaml
---
title: "Always Quoted Title"
tags: [lowercase-hyphenated]
status: active
created: 2026-01-15
---
```

**Status values:** `active` | `superseded` | `stale` | `draft`

> ⚠️ Title **must** be quoted — unquoted colons can break YAML parsing.

## Core Guidelines

- **Local first** — check `RESEARCH/`, `GUIDES/`, `PLANS/` before external searches. These are this nest's own notes: a project session takes its plans and instructions from its own project, never from another project's files
- **Write findings down** — if you research something, save it to `RESEARCH/`
- **Cite sources** — no claim without a path, link, or reference
- **Don't overwrite** — append or create new files; don't silently clobber existing work
- **`.scratch/` is disposable** — don't rely on it across sessions
- **Stay on task** — only stage files relevant to your current work

## Git Commit Identity

A commit carries the identity of whoever actually made it.

- **Author and committer:** commit with the identity configured in the repository you are working in (`git config user.name` and `git config user.email`). A seat's checkout is given its own identity by the host; use it as configured, and do not replace it with anyone else's name or email.
- **Sign-off follows the repository's rules:** a `Signed-off-by` trailer is a certification by the identity it names (for a DCO, that it has the right to submit the change). Where the repository requires one, add your own with `git commit -s`, which uses the configured identity. Never add a `Signed-off-by` or `Co-authored-by` trailer naming someone who did not make or certify the change, including the person who launched you.
- **No configured identity:** if `git config user.email` is empty, do not invent, borrow or guess one, and do not commit. Report that this repository has no commit identity configured: it is a setup fact to fix, not a question to wait on.
- **Signing:** if the agent has a registered signing key, sign commits. If not, commits will land unverified — this is acceptable until agent SSH keys are provisioned. Do NOT use the human's signing key.
- **Verify before pushing:** `git log -1 --format='%an <%ae>%n%(trailers)'` shows the author and every trailer you are about to publish.

<!-- BEGIN BUZZ MANAGED — regenerated automatically, do not edit below -->
## Active Agents

*(No agents deployed yet. Add agents in the Buzz desktop app.)*

<!-- END BUZZ MANAGED -->
