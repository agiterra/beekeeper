# Seat bundles: the contrasting-pack live experiment — runbook, 2026-09-15

Candidate `2606c2fc6` on base `0f87b13d4`, branch `work/seat-bundles-opus`,
ledger [132](../SESSION_STATE.md). **Not landed, not installed.** Nobody has
run this yet; this file is the procedure and the empty result table. Whoever
runs it fills the tables in and dates the heading of § Results.

## What it is for

Two seats in one session run **different packs that both contain a skill
called `marker-skill`**, and each skill tells its holder to quote a phrase
that only its own copy carries, plus a line from a supporting file beside it.
So the run answers four questions at once, none of which a unit test can:

1. **Does the seat read its own bundle?** Seat A must answer with A's marker,
   seat B with B's. The same skill name in both packs is the point — a seat
   reading a shared or wrongly-resolved directory answers with the other
   seat's phrase, or with the first one materialized.
2. **Do supporting files arrive?** `materialize_skills` copied `SKILL.md`
   alone, so the `notes.md` line is the part that was missing before this
   change. A seat that answers the marker but not the notes line is reading
   an old-style materialization.
3. **Is the checkout untouched?** `git status --porcelain` in each seat's
   worktree must be empty, and there must be no `.agents/` directory in it.
   The `.claude/settings.local.json` write fence is the one file this change
   does not remove; it is already excluded.
4. **Does the bundle survive a provider restart?** The bundle is keyed by
   session id, so the same seat must answer the same way after a restart
   without re-materializing into a new directory.

Then one repeat with `BUZZ_SEAT_SKILLS_IN_TREE=1` measures what the change
replaced, on the same machine, in the same session.

## Before you start

- Install the candidate: `scripts/app-from.sh 2606c2fc6`. Each rebuild
  re-prompts the login keychain, so be at the keyboard.
- Have a project with a checkout the lead can hire into, and two agents
  associated with it in different primary roles (one Claude seat, one Codex
  seat — the briefing is provider-independent and this is where that claim
  is checked).
- Note the app data directory. `scripts/app-from.sh` pins the bundle to the
  **dev** identity, so on this machine it is
  `~/Library/Application Support/io.agiterra.beekeeper.app.dev`. A release
  build uses `io.agiterra.beekeeper.app` instead, and
  `~/Library/Application Support/Beekeeper` is neither — that is the tools
  directory (node tools and runtimes). Seat bundles land under
  `agents/seats/<session id>/`, beside the existing `agents/nests/`, with
  `skills/<name>/` and `manifest.json` inside. If in doubt, read the path the
  briefing gave the seat, or `find ~/Library/Application\ Support -type d
  -name seats -maxdepth 3`.

## Step 1 — build the two throwaway packs

Both packs contain a skill directory named `marker-skill`, so only the
contents distinguish them. Run this once; it writes to `/tmp` and touches no
repository:

```bash
for pair in "a:GOLDFINCH-ALPHA:Pack A note: the kettle is on the third shelf." \
            "b:GOLDFINCH-BRAVO:Pack B note: the kettle is in the cellar."; do
  letter=${pair%%:*}; rest=${pair#*:}; marker=${rest%%:*}; note=${rest#*:}
  root=/tmp/seat-bundle-pack-$letter/skills/marker-skill
  mkdir -p "$root"
  printf '%s\n' "$note" > "$root/notes.md"
  cat > "$root/SKILL.md" <<SKILL
---
name: marker-skill
description: Experiment marker for the seat-bundles live check.
---

# Marker

When asked for your marker, answer with exactly this word and nothing else:

$marker

When asked for your notes line, read \`notes.md\` **in this same directory**
and quote its single line verbatim. Do not guess it, do not answer from this
file, and do not copy either file anywhere.
SKILL
done
find /tmp/seat-bundle-pack-? -type f | sort
```

Point each pack at a persona that claims `marker-skill`, the way the project's
other packs do: pack A for the role the first seat holds, pack B for the
second. (If a pack needs a `persona.toml` or equivalent beside `skills/`,
copy the shape from an existing local pack rather than inventing one — the
experiment is about materialization, not about pack authoring.)

## Step 2 — hire the two seats

Start one session as the project's lead in a worktree, and have it hire two
agents: one whose runtime is Claude, one whose runtime is Codex. Record each
seat's label, agent name, runtime, and the session id from the Agents tab.

## Step 3 — ask each seat, before any restart

Send each seat this, verbatim:

> Answer in exactly two lines and nothing else.
> Line 1: your marker, from your `marker-skill` skill.
> Line 2: the single line of `notes.md` that sits beside that skill's
> `SKILL.md`, quoted verbatim.

Then, for each seat, from a terminal (not from the seat):

```bash
SESSION=<session id>
APP=~/Library/Application\ Support/io.agiterra.beekeeper.app.dev
ls "$APP/agents/seats/$SESSION/skills"
cat "$APP/agents/seats/$SESSION/manifest.json"
cd <that seat's worktree>
git status --porcelain      # must print nothing
ls -a | grep -c '^\.agents$'  # must be 0
```

Record the marker, the notes line, the bundle path from the manifest's own
location, `packRef` (null is expected for a local pack), whether
`git status` was empty, and whether `.agents/` was present.

## Step 4 — restart the provider and ask again

Quit and relaunch the app, let both seats reattach, and send the same prompt
again. Record the same fields. The bundle path must be identical — a second
directory for the same session means the session id did not survive the
reattach, which is the thing that would make bundles accumulate per
generation.

## Step 5 — the comparison run

Quit the app, relaunch it with `BUZZ_SEAT_SKILLS_IN_TREE=1` in its
environment, start a **new** session, hire one seat with pack A, and ask the
same question. Expect the old behaviour and record it as such:
`.agents/skills/marker-skill/SKILL.md` in the worktree, `git status` **not**
empty (nothing excludes `.agents/` any more — finding 76), no `skills/` under
the bundle directory, and no notes line, because the in-tree path copies
`SKILL.md` alone. Then quit, drop the variable, and relaunch normally.

## Results — not yet run

Fill in on the day it runs; leave a row's cell as `not observed` rather than
inferring it.

### Before restart

| Seat | Runtime | Pack | Marker | Notes line | `git status` empty | `.agents/` present | Bundle path | `packRef` |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| | | A | | | | | | |
| | | B | | | | | | |

### After provider restart

| Seat | Marker | Notes line | Bundle path identical | `git status` empty |
| --- | --- | --- | --- | --- |
| | | | | |
| | | | | |

### `BUZZ_SEAT_SKILLS_IN_TREE=1`

| Seat | Marker | Notes line | `.agents/skills` in tree | `git status` output | Bundle `skills/` exists |
| --- | --- | --- | --- | --- | --- |
| | | | | | |

## Known limits of this check

- It does not test two seats sharing one working directory; that is still
  refused, now on the git reasons alone (one index and one HEAD per writer,
  items 30 and 80).
- Bundles are not cleaned up when a session ends, so after several runs
  `agents/seats/` holds a directory per session. Removing them is safe while
  the session is not running. See ledger 132 (a).
- `crates/buzz-agent/src/hints.rs:8` still scans `.agents/skills` in the
  current directory (ledger 132 (b)). Seats do not use that path, so a seat run after this
  change finds nothing there — which is correct, not a regression, but it is
  the next thing to read if a skill seems to be "missing" somewhere else.
