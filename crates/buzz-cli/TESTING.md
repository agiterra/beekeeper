# buzz-cli Live Testing Guide

Manual testing runbook for verifying every CLI command against a local relay.
An agent or developer follows this step by step, running each command and
checking the output.

---

## 1. Prerequisites

Docker services running and healthy:

```bash
docker compose ps
# buzz-postgres   healthy
# buzz-redis      healthy
```

If not running: `just setup` from the repo root.

Tools: `jq`, `curl`, Rust toolchain.

---

## 2. Build the CLI

```bash
cargo build -p buzz-cli
```

Use `cargo run -p buzz-cli --` or the built binary at `target/debug/buzz`.

---

## 3. Start the Relay

In a separate terminal:

```bash
cd REPOS/buzz-nostr
set -a && source .env && set +a
cargo run -p buzz-relay
```

Verify:

```bash
curl -s http://localhost:3000/_liveness
# "ok" or 200 status
```

The `.env` should have `BUZZ_REQUIRE_AUTH_TOKEN=false` for local dev.

---

## 4. Mint Test Credentials

### Option A: buzz-admin (full scopes including admin)

This mints a token with all CLI-relevant scopes (including `admin:channels`)
via direct DB access. Use this for testing admin operations (archive,
delete-channel, add/remove-channel-member).

```bash
DATABASE_URL="${DATABASE_URL:?set DATABASE_URL for the local Buzz database}" \
cargo run -p buzz-admin -- mint-token \
  --name "cli-test" \
  --scopes "messages:read,messages:write,channels:read,channels:write,users:read,users:write,files:read,files:write,admin:channels"
```

This generates a keypair and prints:
- **Private key (nsec)** — save for `BUZZ_PRIVATE_KEY` testing

Export:

```bash
export BUZZ_RELAY_URL="http://localhost:3000"
export BUZZ_PRIVATE_KEY="nsec1..."   # from the mint output
```

### Scope reference

| Scope | Self-mintable | Needed for |
|-------|:---:|------------|
| `messages:read` | ✅ | `messages get`, `messages thread`, `messages search`, `feed get` |
| `messages:write` | ✅ | `messages send`, `messages edit`, `messages delete`, `reactions`, `messages vote` |
| `channels:read` | ✅ | `channels list`, `channels get`, `channels members` |
| `channels:write` | ✅ | `channels create`, `channels update`, `channels join`, `channels leave`, `channels topic`, `channels purpose` |
| `users:read` | ✅ | `users get`, `users presence` |
| `users:write` | ✅ | `users set-profile`, `users set-presence`, `users set-status` |
| `files:read` | ✅ | — |
| `files:write` | ✅ | — |
| `admin:channels` | ❌ | `channels archive`, `channels unarchive`, `channels delete`, `channels add-member`, `channels remove-member` |

---

## 5. Unit Tests

```bash
cargo test -p buzz-cli
# Expected: see cargo test -p buzz-cli for current count

cargo clippy -p buzz-cli -- -D warnings
# Expected: zero warnings
```

---

## 6. Live Testing — Command by Command

Run each command, verify exit code 0 and check output. Most commands
return JSON (pipe through `jq .` to validate). Commands are ordered so
earlier ones create resources that later ones need.

### 6.1 Channels

```bash
# channels create (stream)
bee channels create --name "test-stream" --type stream --visibility open \
  --description "CLI test channel" | jq .
# Save the channel ID:
CHANNEL_ID=$(bee channels create --name "test-cli" --type stream --visibility open | jq -r '.channel_id')
# Expected: {"event_id":"...","accepted":true,"message":"...","channel_id":"<uuid>"}

# channels create (forum) — needed for messages vote later
FORUM_ID=$(bee channels create --name "test-forum" --type forum --visibility open | jq -r '.channel_id')

# channels list
bee channels list | jq .
# Expected: [{"channel_id":"...","name":"...","description":"...","created_at":N}]
bee channels list --visibility open | jq .
bee channels list --member | jq .

# channels get
bee channels get --channel "$CHANNEL_ID" | jq .
# Expected: {"channel_id":"...","name":"...","description":"...","created_at":N,"pubkey":"..."} or null

# channels update
bee channels update --channel "$CHANNEL_ID" --name "test-cli-updated" \
  --description "Updated" | jq .
# Expected: {"event_id":"...","accepted":true,"message":"..."}

# channels topic
bee channels topic --channel "$CHANNEL_ID" --topic "Test topic" | jq .
# Expected: {"event_id":"...","accepted":true,"message":"..."}

# channels purpose
bee channels purpose --channel "$CHANNEL_ID" --purpose "Testing" | jq .
# Expected: {"event_id":"...","accepted":true,"message":"..."}

# channels join (may already be a member from create)
bee channels join --channel "$CHANNEL_ID" | jq .
# Expected: {"event_id":"...","accepted":true,"message":"..."}

# channels leave
# NOTE: Fails with 400 "cannot remove the last owner" if this identity is the
# sole owner (which it is after channels create). To test leave successfully,
# first add-member a second pubkey as owner. The relay enforces ≥1 owner.
bee channels leave --channel "$CHANNEL_ID" | jq .
# Expected: {"event_id":"...","accepted":true,"message":"..."} (or 400 if last owner)

# Re-join so we can send messages
bee channels join --channel "$CHANNEL_ID" | jq .
# Expected: {"event_id":"...","accepted":true,"message":"..."}

# channels archive (requires admin:channels scope)
bee channels archive --channel "$CHANNEL_ID" | jq .
# Expected: {"event_id":"...","accepted":true,"message":"..."}

# channels unarchive
bee channels unarchive --channel "$CHANNEL_ID" | jq .
# Expected: {"event_id":"...","accepted":true,"message":"..."}
```

### 6.2 Canvas

```bash
# canvas set
bee canvas set --channel "$CHANNEL_ID" --content "# Test Canvas" | jq .

# canvas set from stdin
echo "# Canvas from stdin" | bee canvas set --channel "$CHANNEL_ID" --content - | jq .

# canvas get
bee canvas get --channel "$CHANNEL_ID"
# Expected: raw markdown string, or: null
```

### 6.3 Messages

```bash
# messages send
MSG=$(bee messages send --channel "$CHANNEL_ID" --content "Hello from CLI test" | jq .)
echo "$MSG"
EVENT_ID=$(echo "$MSG" | jq -r '.event_id')

# messages send with reply + broadcast
REPLY=$(bee messages send --channel "$CHANNEL_ID" --content "Reply" \
  --reply-to "$EVENT_ID" --broadcast | jq .)
echo "$REPLY"
REPLY_ID=$(echo "$REPLY" | jq -r '.event_id')

# messages send with mentions — @name in content is auto-resolved, no flag needed
bee messages send --channel "$CHANNEL_ID" --content "Hey @someone" | jq .

# messages send with NIP-27 nostr:npub1… inline mention — auto-resolved to p-tag
bee messages send --channel "$CHANNEL_ID" \
  --content "Check with nostr:npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg on this" | jq .

# messages send from stdin — safe path for content with shell metacharacters
# (backticks, $vars, code blocks) that would otherwise be expanded by the shell.
echo 'Body with `backticks` and $vars stays literal.' \
  | bee messages send --channel "$CHANNEL_ID" --content - | jq .

# messages get
bee messages get --channel "$CHANNEL_ID" | jq .
bee messages get --channel "$CHANNEL_ID" --limit 5 | jq .

# messages thread
bee messages thread --channel "$CHANNEL_ID" --event "$EVENT_ID" | jq .

# messages search
bee messages search --query "Hello" | jq .
bee messages search --query "CLI test" --limit 5 | jq .

# messages edit
bee messages edit --event "$EVENT_ID" --content "Edited by CLI test" | jq .

# messages delete
bee messages delete --event "$REPLY_ID" | jq .
```

### 6.4 Diff Messages

```bash
# messages send-diff from stdin
echo '--- a/foo.rs
+++ b/foo.rs
@@ -1,3 +1,3 @@
-fn old() {}
+fn new() {}' | bee messages send-diff \
  --channel "$CHANNEL_ID" \
  --diff - \
  --repo "https://github.com/example/repo" \
  --commit "abcdef1234567890abcdef1234567890abcdef12" | jq .

# messages send-diff with metadata
echo "diff content" | bee messages send-diff \
  --channel "$CHANNEL_ID" \
  --diff - \
  --repo "https://github.com/example/repo" \
  --commit "abcdef1234567890abcdef1234567890abcdef12" \
  --file "src/main.rs" \
  --lang "rust" \
  --description "Refactored main" | jq .

# messages send-diff with branch + PR metadata
echo "diff content" | bee messages send-diff \
  --channel "$CHANNEL_ID" \
  --diff - \
  --repo "https://github.com/example/repo" \
  --commit "abcdef1234567890abcdef1234567890abcdef12" \
  --parent-commit "1234567890abcdef1234567890abcdef12345678" \
  --source-branch "feature/cli" \
  --target-branch "main" \
  --pr 42 | jq .
```

### 6.5 Reactions

```bash
# Send a message to react to
REACT_MSG=$(bee messages send --channel "$CHANNEL_ID" --content "React to this")
REACT_ID=$(echo "$REACT_MSG" | jq -r '.event_id')

# reactions add
bee reactions add --event "$REACT_ID" --emoji "👍" | jq .

# reactions get
bee reactions get --event "$REACT_ID" | jq .
# Expected: {"reactions":[{"emoji":"...","count":N,"pubkeys":["..."]}]}

# reactions remove
bee reactions remove --event "$REACT_ID" --emoji "👍" | jq .
```

### 6.6 DMs

```bash
# dms list
bee dms list | jq .
# Expected: [{"dm_id":"...","participants":["..."],"created_at":N}]

# dms open (needs a real pubkey — use your own or a test one)
# Get your own pubkey first:
MY_PUBKEY=$(bee users get | jq -r '.[0].pubkey // empty')
echo "My pubkey: $MY_PUBKEY"

# dms open with a synthetic pubkey (relay will create the user)
DM_RESULT=$(bee dms open --pubkey "0000000000000000000000000000000000000000000000000000000000000001")
echo "$DM_RESULT" | jq .
# Expected: {"event_id":"...","accepted":true,"message":"...","dm_id":"<uuid>"}
DM_ID=$(echo "$DM_RESULT" | jq -r '.dm_id')

# dms add-member (requires messages:write scope — NOT admin:channels)
bee dms add-member --channel "$DM_ID" \
  --pubkey "0000000000000000000000000000000000000000000000000000000000000002" | jq .
```

### 6.7 Users & Presence

```bash
# users get — own profile (0 pubkeys)
bee users get | jq .
# Expected: [{...profile...}] — always returns an array, even for single results

# users get — single pubkey
bee users get --pubkey "$MY_PUBKEY" | jq .

# users get — batch (2+ pubkeys)
bee users get --pubkey "$MY_PUBKEY" --pubkey "$MY_PUBKEY" | jq .

# users set-profile
bee users set-profile --name "CLI Test Agent" --about "Testing buzz-cli" | jq .

# users presence
bee users presence --pubkeys "$MY_PUBKEY" | jq .

# users set-presence
bee users set-presence --status online | jq .
bee users set-presence --status away | jq .
bee users set-presence --status offline | jq .
# Note: set-presence may fail — kind:20001 is ephemeral and rejected by the HTTP bridge

# users set-status — NIP-38 kind:30315 on the d:general coordinate
bee users set-status --text "reviewing PRs" --emoji "🔍" | jq .
bee users set-status --text "no emoji this time" | jq .

# users set-status — emoji-only status (intentional: text is blank, emoji is kept)
bee users set-status --text "" --emoji "🎶" | jq .

# users set-status --clear — removes the status (empty content, d:general only)
bee users set-status --clear | jq .

# --clear is mutually exclusive with --text/--emoji
bee users set-status --clear --text "nope" 2>&1; echo "exit: $?"
# Expected: exit 1 — clap conflict error
```

### 6.8 Channel Members (add/remove require admin:channels)

```bash
# channels add-member
bee channels add-member --channel "$CHANNEL_ID" \
  --pubkey "0000000000000000000000000000000000000000000000000000000000000001" \
  --role member | jq .

# channels members
bee channels members --channel "$CHANNEL_ID" | jq .
# Expected: [{"pubkey":"...","role":"..."}]

# channels remove-member
bee channels remove-member --channel "$CHANNEL_ID" \
  --pubkey "0000000000000000000000000000000000000000000000000000000000000001" | jq .
```

### 6.9 Workflows

```bash
# workflows create
# NOTE: trigger uses `on:` tag (serde internally tagged enum).
# Valid triggers: message_posted, reaction_added, diff_posted, schedule, webhook
# Steps use `action:` tag: send_message, send_dm, set_channel_topic, add_reaction, etc.
WF=$(bee workflows create --channel "$CHANNEL_ID" \
  --yaml 'name: test-wf
trigger:
  on: webhook
steps:
  - id: step1
    action: send_message
    text: "Hello from workflow"' | jq .)
echo "$WF"
WF_ID=$(echo "$WF" | jq -r '.workflow_id')

# workflows list
bee workflows list --channel "$CHANNEL_ID" | jq .

# workflows get
bee workflows get --workflow "$WF_ID" | jq .
# Expected: {"workflow_id":"...","content":"<yaml>","created_at":N,"pubkey":"..."} or null

# workflows update (requires --channel)
bee workflows update --channel "$CHANNEL_ID" --workflow "$WF_ID" \
  --yaml 'name: test-wf-updated
trigger:
  on: webhook
steps:
  - id: step1
    action: send_message
    text: "Updated"' | jq .

# workflows trigger
# NOTE: May return 400 "workflow not found" — the relay indexes workflow
# definitions into a DB table asynchronously. If the definition event hasn't
# been indexed yet, the trigger handler won't find it.
bee workflows trigger --workflow "$WF_ID" | jq .

# workflows runs
bee workflows runs --workflow "$WF_ID" | jq .
# Expected: [] — relay stores runs in DB, not as Nostr events; empty is normal

# workflows approve — requires a workflow run waiting for approval
# This is hard to test ad-hoc without a workflow that has an approval gate.
# Test the validation instead:
bee workflows approve --token "00000000-0000-0000-0000-000000000000" 2>&1 || true
# Should fail with relay error (token not found), not a validation error
# To test the deny path: bee workflows approve --token <UUID> --approved false

# workflows delete
bee workflows delete --workflow "$WF_ID" | jq .
```

### 6.10 Feed

```bash
bee feed get | jq .
bee feed get --limit 5 | jq .
# Expected: [{id,pubkey,kind,content,created_at,tags}] — sig-stripped, sorted newest-first
```

### 6.11 Forum & Voting

```bash
# Send a forum post (kind 45001) to the forum channel
FORUM_POST=$(bee messages send --channel "$FORUM_ID" \
  --content "Forum post for vote testing" --kind 45001 | jq .)
echo "$FORUM_POST"
FORUM_EVENT_ID=$(echo "$FORUM_POST" | jq -r '.event_id')

# messages vote (up)
bee messages vote --event "$FORUM_EVENT_ID" --direction up | jq .

# messages vote (down)
bee messages vote --event "$FORUM_EVENT_ID" --direction down | jq .
```

### 6.12 Notes (NIP-23 long-form, kind:30023)

Editable team-knowledge notes keyed by `(kind:30023, you, d=slug)`. `set` is an
idempotent upsert; `rm` is a NIP-09 a-tag deletion. Output is plain text (refs),
not JSON — except `get`/`ls`, which emit JSON.

```bash
# set (first publish — --title required, body from stdin)
cat <<'EOF' | bee notes set --name dco-check --title "DCO Check" \
  --summary "How we verify DCO" --tag dco --tag ci --content -
Run `git log --format='%(trailers:key=Signed-off-by)'` ...
EOF
# → prints event_id / naddr / coordinate / slug / title

# set (edit — omit --title to carry it forward; published_at preserved)
echo "Updated body." | bee notes set --name dco-check --content -

# get by name (own author resolves directly; cross-author #d query otherwise)
bee notes get --name dco-check | jq .
bee notes get --name dco-check --content-only

# get by naddr (exact coordinate; paste the naddr from a set/get above)
bee notes get --naddr "$NADDR" | jq .

# ls (own by default; --author all across the team; --tag filters)
bee notes ls | jq .
bee notes ls --tag dco | jq .
bee notes ls --author all --limit 10 | jq .

# rm (NIP-09 a-tag deletion; subsequent get must 404)
bee notes rm --name dco-check
# → prints deleted <coordinate> / deletion <event-id>
bee notes get --name dco-check   # exits non-zero: not found

# rm of a slug you never published → NotFound, no kind:5 emitted
bee notes rm --name does-not-exist   # exits non-zero
```

### 6.13 Coding Sessions (kinds 44223/44224/44225)

Read-only analysis over a channel that a coding-session provider has published
into. Nothing here writes; a channel with no sessions returns `[]` rather than
an error. See `docs/coding-session-analysis.md` for the direct-SQL equivalents.

```bash
# list (empty channel → [])
bee sessions list --channel "$CHANNEL_ID" | jq .
bee --format compact sessions list --channel "$CHANNEL_ID" | jq .
# → [{"target":"coding-session/v1|...","title":...,"status":...,"model":...,"createdAt":"..."}]

# Save a target key for the commands below
TARGET=$(bee sessions list --channel "$CHANNEL_ID" | jq -r '.[0].target')
SESSION=$(bee sessions list --channel "$CHANNEL_ID" | jq -r '.[0].sessionId')

# transcript by target (markdown) and by session id (resolved through list)
bee sessions transcript --channel "$CHANNEL_ID" --target "$TARGET"
bee sessions transcript --channel "$CHANNEL_ID" --session "$SESSION"

# transcript as raw signed events — one per line, signature included
bee sessions transcript --channel "$CHANNEL_ID" --target "$TARGET" --format jsonl | head -3
# Every line must verify independently; seq order is numeric (10 after 9)
bee sessions transcript --channel "$CHANNEL_ID" --target "$TARGET" --format jsonl \
  | jq -r '.content | fromjson | .eventSeq' | sort -c -n && echo "seq ordered"

# tools — whole channel, then one generation
bee sessions tools --channel "$CHANNEL_ID" | jq .
bee sessions tools --channel "$CHANNEL_ID" --target "$TARGET" | jq '.tools'
bee --format compact sessions tools --channel "$CHANNEL_ID" | jq .

# export — refuses a non-empty directory
rm -rf /tmp/buzz-sessions-export
bee sessions export --channel "$CHANNEL_ID" --out /tmp/buzz-sessions-export | jq .
ls /tmp/buzz-sessions-export
bee sessions export --channel "$CHANNEL_ID" --out /tmp/buzz-sessions-export; echo "exit: $?"
# stderr: {"error":"user_error","message":"--out ... is not empty; exports never overwrite ..."}
# exit: 1

# Neither --target nor --session → clap refuses before any relay call
bee sessions transcript --channel "$CHANNEL_ID" 2>&1; echo "exit: $?"
# exit: 1

# Unknown session id → NotFound
bee sessions transcript --channel "$CHANNEL_ID" --session no-such-session 2>&1; echo "exit: $?"
# exit: 1
```

#### 6.13.1 Turn-stage receipts (kind 44224, D4 / NIP-CSL §"turn-stage receipts") — NOT YET RUN LIVE

**Status: not yet observed live.** This block documents how to observe the four
turn-stage `kind:44224` receipts (`turn_queued`, `turn_started`, `turn_dropped`,
`turn_refused`) once a provider that publishes them is running end-to-end
(Slice 1, Lanes 1A/1B of `docs/CREW_SESSIONS_PLAN.md`). Nothing below has been
run against a live relay as of this writing — it is the intended procedure,
not a verified result.

No `bee sessions` subcommand surfaces raw receipt events directly today —
`sessions list`/`sessions transcript` deliberately never create or confirm a
generation from a turn receipt (see `resolve_sessions` in
`crates/buzz-cli/src/commands/sessions.rs`, and NIP-CSL's "turn-stage
receipts" section), and this lane's brief is read-side decoding, not a new
subcommand (`bee sessions send`/`create`/`inbox` land in Slice 4). To watch
the raw events, use the same `POST /query` bridge and direct-SQL routes
`docs/coding-session-analysis.md` documents for every other kind:

```json
{ "kinds": [44224], "#h": ["<channel-uuid>"] }
```

```sql
SELECT id, pubkey, created_at, content::jsonb
FROM events
WHERE kind = 44224
  AND deleted_at IS NULL
  AND tags @> '[["h", "<channel-uuid>"]]'::jsonb
ORDER BY created_at, id;
```

What to check in `content`, once a live 44220 turn command produces receipts:

- `status` is one of `turn_queued`, `turn_started`, `turn_dropped`,
  `turn_refused` (the six lifecycle statuses — `created`, `resumed`, `stopped`,
  etc. — are the separate, existing vocabulary).
- Exactly six keys (`schema`, `commandId`, `status`, `session`, `error`,
  `turnId`) for `turn_started`; exactly five (no `turnId` key at all) for the
  other three.
- `session` names the exact target the causing `kind:44220` command addressed,
  for all four statuses — never `null`.
- `error` is `null` for `turn_queued`/`turn_started`; `{"code":"QUEUE_FULL",...}`
  for `turn_dropped`; one of `UNAUTHORIZED_OPERATOR`/`UNKNOWN_TARGET`/
  `STALE_GENERATION`/`SESSION_CLOSED` for `turn_refused`.
- `commandId` matches the `commandId` of the `kind:44220` command that
  provoked it, and (per NIP-CST) the same `commandId` should appear on the
  `kind:44225` `user_prompt` item the corresponding `turn_started` opened.
- `bee sessions list`/`bee sessions transcript` must not change their answer
  (status, `confirmed`) for a target whose only new event is a turn receipt —
  this is exactly what `a_turn_receipt_for_an_unknown_target_creates_no_row`
  and `a_turn_receipt_does_not_confirm_or_change_the_status_of_a_known_target`
  pin in `crates/buzz-cli/src/commands/sessions.rs`, against synthetic events;
  running the same check against a live relay is the residual this block
  leaves open.

#### 6.13.2 Authority grants (kind 44228) — npub/hex targets, agent-marked roster — NOT YET RUN LIVE

**Status: unit-tested only (`cargo test -p buzz-cli --lib` —
`grantee_pubkey_resolves_hex_and_npub_to_the_same_value`,
`grantee_pubkey_rejects_a_display_name_and_malformed_input`,
`roster_marks_only_pubkeys_the_channel_metadata_named_as_actors`,
`roster_agent_set_is_empty_when_no_execution_ever_named_an_actor`); not yet
observed against a live relay.** `bee sessions grant`/`revoke` accept
`--pubkey` as either a 64-char lowercase hex pubkey or an `npub1…` bech32 key
(`nostr::PublicKey::parse`, the same resolver `messages.rs`'s `--author` and
`--mention` use) — never a display name, so a grant target is always exact.
Both forms resolve to the identical hex before the 44228 transition is built;
the wire content (NIP-CSAT) and the relay never see the npub form.

```bash
# grant by hex (existing behavior, unchanged)
bee sessions grant --channel "$CHANNEL_ID" --genesis "$GENESIS" \
  --pubkey "$AGENT_HEX_PUBKEY" --role collaborator | jq .

# grant by npub — resolves to the same hex on the wire
bee sessions grant --channel "$CHANNEL_ID" --genesis "$GENESIS" \
  --pubkey "$AGENT_NPUB" --role collaborator | jq .
# Confirm both submissions produced the same granteePubkey in the roster:
bee sessions roster --channel "$CHANNEL_ID" --genesis "$GENESIS" | jq '.grants'

# revoke by npub
bee sessions revoke --channel "$CHANNEL_ID" --genesis "$GENESIS" --pubkey "$AGENT_NPUB" | jq .

# malformed --pubkey (neither hex nor npub) → usage error before any relay call
bee sessions grant --channel "$CHANNEL_ID" --genesis "$GENESIS" \
  --pubkey "not-a-key" --role collaborator 2>&1; echo "exit: $?"
# exit: 1
```

`bee sessions roster` marks each grant (and each pending, un-receipted
transition) `"agent": true` when the channel's coding-session metadata
(kind 44223) has ever named that pubkey as a seated actor (`agentRef`,
plan D1/D6 — see NIP-CSL's "Actor and role" section). This is a fact about
the pubkey gathered from the channel's own metadata history, never a guess
from the pubkey's shape:

```bash
bee sessions roster --channel "$CHANNEL_ID" --genesis "$GENESIS" | jq .
# → {"genesisRef":"...","headEventId":"...","headSeq":N,
#    "grants":[{"pubkey":"...","role":"collaborator","agent":true}, ...],
#    "pending":[...]}
```

This residual — seating an actual managed-agent seat (Slice 3, Lane 3A/3B)
and confirming `roster` marks it `"agent": true` from a live `agentRef` —
is left open pending that lane; the unit tests above pin the decode/marking
logic against synthetic metadata in the meantime.

#### 6.13.3 Crew verbs — `send` / `create` / `inbox` / `status` (S4 Lane 4A)

`bee sessions` gained the four write/mailbox verbs of plan D5. They address a
*seat* — one provider execution — and every one of them refuses rather than
guesses when a name is ambiguous.

**Status: RUN LIVE — see the recorded run at the end of this block for what was
and was not exercised.**

Two envelope facts govern the writes, and both come from the relay's own
validators rather than from taste:

- Kinds 44220 and 44221 accept **exactly three tags** (`h`, `cs-v`/`csl-v`,
  `cs-target`/`csl-command`) — `validate_coding_session_command_envelope` and
  `validate_coding_session_lifecycle_command_envelope` in
  `crates/buzz-relay/src/handlers/ingest.rs`. So these two kinds are signed
  with `sign_event_unchecked`, never with `sign_event`, whose NIP-OA `auth`
  tag injection would make the event **invalid**. Membership delegation still
  reaches the relay: `submit_event` sends the same tag in the `x-auth-tag`
  header, which is where `POST /events` reads it
  (`crates/buzz-relay/src/api/bridge.rs`). If a run under `BUZZ_AUTH_TAG`
  ever comes back `invalid: unsupported coding-session command tag`, that
  regression is the cause.
- A `boundary` turn **omits** the `deliver` key. The payload is
  `deny_unknown_fields`, so a relay built before the field existed refuses any
  payload carrying it, and `boundary` is what an absent key already meant.
  Confirm on the wire:

  ```bash
  bee messages … # (any read that shows the raw 44220; or query directly)
  curl -s -X POST "$RELAY_HTTP/query" -H 'Content-Type: application/json' \
    -d "[{\"kinds\":[44220],\"#h\":[\"$CHANNEL_ID\"]}]" \
    | jq -r '.[0].content | fromjson | .action'
  # boundary → {"type":"thread.turn.start","text":"..."}    (no "deliver" key)
  # steer    → {"type":"thread.turn.start","text":"...","deliver":"steer"}
  ```

**`sessions status` output shape follows stdout.** With neither `--json-lines`
nor `--no-json-lines`, and with no explicit `--format`, it prints **NDJSON** —
one JSON object per execution, one per line, no envelope — when stdout is a
pipe or a file, and the single document when stdout is a terminal. Naming
`--format` explicitly, or passing `--no-json-lines`, always gets the document,
piped or not. That matters when you are reading the *channel-level* keys, which
only the envelope carries: every example below that needs `founders` or
`leaseSnapshotRecords` therefore says `--format json`, and every one that reads
per-execution fields is left bare. (This is unrelated to `sessions transcript
--format jsonl`, which is whole signed events rather than these rows.)

```bash
# ── status ────────────────────────────────────────────────────────────────
# One row per execution: seat, liveness, open turn, queue depth, turn budget.
# Bare and piped, with no --format named: NDJSON, one object per line.
bee sessions status --channel "$CHANNEL_ID" | jq .
# → {"target":"coding-session/v1|...","sessionId":"8063fcfc-...",
#      "generation":1,"actor":"ede63017...","role":"lead",
#      "sessionRef":"ccf74cc3-...","seat":"ede63017·lead",
#      "founder":"3d3b7169...","createSigner":"3d3b7169...",
#      "runtime":"claude","model":"default","status":"running",
#      "live":"live","liveness":"live","lastSignedSeq":154,
#      "openTurn":{"commandId":"48adca62-...","runningFor":"2m",...},
#      "queuedTurns":0,
#      "turnBudget":{"used":10,"limit":200,"exhausted":false}}
# → {"target":"coding-session/v1|...","seat":"1ddd35c6·builder",...}
#   …one line per execution, and nothing else: no brackets, no envelope, and
#   zero bytes (exit 0) when the channel has no executions.

# The envelope — `channel`, the channel-level `founders` array, and
# `leaseSnapshotRecords` — exists only in the single document, so ask for it
# by name. This is also the form to use in a script that indexes into
# `.executions`.
bee --format json sessions status --channel "$CHANNEL_ID" | jq .
# → {"channel":"175c3165-...",
#     "executions":[{"target":"coding-session/v1|...","seat":"ede63017·lead",
#      "live":"live","liveness":"live","lastSignedSeq":154,
#      "openTurn":{...},"queuedTurns":0,
#      "turnBudget":{"used":10,"limit":200,"exhausted":false}}, ...],
#     "founders":["3d3b7169..."],
#     "leaseSnapshotRecords":2}
#   (2 leases answered, which is why both rows read "live"; see below for what
#    a 0 there means)
bee --format compact sessions status --channel "$CHANNEL_ID" | jq .

# `live` comes from a kind-24223 lease, which is EPHEMERAL: the relay serves it
# from a Redis snapshot, never from stored events, so it describes this instant
# and has no history. `leaseSnapshotRecords` is how many the snapshot held —
# 0 means no provider is attached right now, and every row then reads `quiet`
# (age since its newest signed 44225) or `unknown` (nothing signed yet), never
# `live`. `released` is a positive claim: a `released` lease, or a durably
# stopped execution.

# `turnBudget` (plan D9 / NIP-CSL's `turnBudget` fork amendment): `null` until
# a provider has echoed an umbrella turn budget onto this execution's kind
# 44223 metadata — either this build predates the budget, or the umbrella has
# none configured. Once a provider is publishing it:
#   json:    "turnBudget": {"used": 7, "limit": 20, "exhausted": false}
#   compact: "turnBudget": "7/20"
# `exhausted` (json only) mirrors `used >= limit`, the same condition that
# makes the provider answer the next non-founder turn `turn_refused` /
# `BUDGET_EXHAUSTED` (NIP-CSL). The count is the typed optional `turnBudget`
# field of `SessionMetadata`, read off the newest metadata row; a `bee` build
# that predates the field ignores the key rather than failing to decode the
# row, while a half-written `turnBudget` (one of `used`/`limit` missing) drops
# the whole metadata record as malformed rather than reading as `null`.
# The number reported is the umbrella's, not one execution's: every row that
# claims the same `sessionRef` prints the highest `used` any of them has
# echoed, so an idle seat never advertises room the umbrella no longer has.

# ── inbox ─────────────────────────────────────────────────────────────────
# Turns addressed to executions whose `agentRef` equals THIS identity's pubkey,
# oldest first, each with the newest receipt stage its commandId was answered
# with. A sibling's traffic never appears here even though the relay serves it.
bee sessions inbox --channel "$CHANNEL_ID" | jq .
# → {"channel":"...","identity":"<my pubkey>","seats":1,
#    "turns":[{"eventId":"...","from":"...","commandId":"...","deliver":"boundary",
#              "text":"rebase and re-run the gate","stage":"turn_started",
#              "turnId":"...","errorCode":null}, ...]}

# Cursor: exclusive, and it must name a row of MINE — a cursor pointing at
# somebody else's turn is refused rather than silently restarting the inbox.
LAST=$(bee sessions inbox --channel "$CHANNEL_ID" | jq -r '.turns[-1].eventId')
bee sessions inbox --channel "$CHANNEL_ID" --since "$LAST" | jq '.turns | length'
# → 0
bee sessions inbox --channel "$CHANNEL_ID" --since 0000000000000000000000000000000000000000000000000000000000000000 2>&1; echo "exit: $?"
# stderr: {"error":"not_found","message":"--since ... names no turn addressed to this identity ..."}
# exit: 1

# ── send ──────────────────────────────────────────────────────────────────
# `--to` is tried as an exact cs-target key, then a session id, then a role.
TARGET=$(bee sessions list --channel "$CHANNEL_ID" | jq -r '.[0].target')
echo 'rebase and re-run the gate' \
  | bee sessions send --channel "$CHANNEL_ID" --to "$TARGET" --content - | jq .
# → {"event_id":"...","accepted":true,"message":"","seat":"...","role":null,
#    "sessionRef":null,"liveness":"quiet 3m","commandId":"<uuid>",
#    "target":"coding-session/v1|...","deliver":"boundary"}

# By session id → the NEWEST generation of that execution.
bee sessions send --channel "$CHANNEL_ID" --to "$SESSION" --content 'go' | jq '.target'

# By role. A role is only unique inside one umbrella, so a role lookup needs
# one: --session-ref, or the umbrella the caller's own seat sits in. Without a
# scope it is REFUSED, never widened.
bee sessions send --channel "$CHANNEL_ID" --to builder \
  --session-ref "$UMBRELLA" --content 'take the build lane' | jq .
bee sessions send --channel "$CHANNEL_ID" --to builder --content 'x' 2>&1; echo "exit: $?"
# stderr: "... role 'builder' is only unique inside one umbrella session, and
#          none was given — pass --session-ref (seen here: ...)"
# exit: 1

# Ambiguity is an error listing the candidates — never a guess:
# stderr: "role 'builder' in umbrella u-1 matches 2 executions — pass --to with
#          one of: coding-session/v1|..., coding-session/v1|..."

# Delivery classes. `steer` and `interrupt` are written to the wire; `boundary`
# is omitted (above). NOTE: `capabilities.threadSteer` is false for every v1
# runtime in this build (plan ruling R3), so a `steer` earns a
# `turn_degraded`/`STEER_UNSUPPORTED` receipt beside its `turn_queued` and runs
# at the next boundary. That is the honest downgrade, not a failure.
bee sessions send --channel "$CHANNEL_ID" --to "$TARGET" --deliver steer --content 'while you are there…' | jq .
#  accepted:true                                   <- the relay stored it
#  deliveryStatus:"turn_degraded"                  <- what the provider answered
#  delivered:true
#  delivery:"steer requested, provider degraded to boundary"
bee sessions send --channel "$CHANNEL_ID" --to "$TARGET" --deliver interrupt --content 'stop' | jq .

# Every send waits up to 10s for the FIRST turn receipt answering its own
# commandId and prints what it said (ledger 80 c: `accepted:true` alone told a
# steer's sender its words had gone in mid-turn when they had not). Nothing
# answering is its own answer — `delivered:null`,
# `deliveryStatus:"unconfirmed"` — and is never rendered as success.
bee sessions send --channel "$CHANNEL_ID" --to "$TARGET" --content 'go' --no-wait | jq '.delivery'
#  "the relay stored the command; --no-wait skipped the receipt read, so
#   whether the turn reached the execution is unknown"

# --reply-to is REFUSED, and says why:
bee sessions send --channel "$CHANNEL_ID" --to "$TARGET" --content 'x' --reply-to 7 2>&1; echo "exit: $?"
# stderr: "--reply-to is refused: kind 44220 carries no reply reference. Its
#          envelope is exactly three tags (h, cs-v, cs-target) and its payload
#          is deny_unknown_fields ..."
# exit: 1

# ── send --readdress (plan ruling R1) ─────────────────────────────────────
# A turn answered `turn_dropped`/NO_LIVE_EXECUTION or
# `turn_refused`/STALE_GENERATION did not run and will not run: re-addressing
# it is the SENDER's job. `--readdress <commandId>` re-signs the same text
# against the CURRENT generation of the same execution.
OWED=$(bee sessions inbox --channel "$CHANNEL_ID" \
  | jq -r '.turns[] | select(.errorCode=="NO_LIVE_EXECUTION") | .commandId' | tail -1)
bee sessions send --channel "$CHANNEL_ID" --readdress "$OWED" | jq .
# → {"event_id":"...","accepted":true,"readdressOf":"<old commandId>",
#    "readdressReason":"turn_dropped/NO_LIVE_EXECUTION",
#    "readdressedFromGeneration":3,"resumedBy":"<pubkey that signed the resume>",
#    "commandId":"<new uuid>","target":"coding-session/v1|...","deliver":"boundary"}
#
# The three questions R1 left open, each answered by a refusal rather than a
# guess:
#   which generation? the highest generation of the same
#     (driver, instanceId, sessionId). If that is still the generation that
#     refused the turn AND no `live` lease answers for it, the re-send is
#     refused: "resume it … and re-address then".
#   who resumed it?   the signer of the newest 44221 `session.resume` naming an
#     earlier generation, reported as `resumedBy` (null when none is on record).
#   session closed?   refused: "execution … was durably stopped (generation N,
#     status stopped) — a stopped session accepts no turns; create a new one".
#
# A commandId that was answered anything else is refused rather than re-sent:
bee sessions send --channel "$CHANNEL_ID" --readdress "$STARTED_COMMAND_ID" 2>&1; echo "exit: $?"
# stderr: "command '...' was answered turn_started — --readdress is only for a
#          turn_dropped/NO_LIVE_EXECUTION or turn_refused/STALE_GENERATION"
# exit: 1

# ── doctor, on the same owed turn ─────────────────────────────────────────
# `doctor` reads 44224 receipts and 44220 commands alongside 44225 transcripts,
# because every `turn_dropped`/`turn_refused` site in the provider publishes a
# receipt and no transcript item, and the receipt never says what was asked. So
# a refused command is a row of its own with verdict `answered` and the
# commandId `--readdress` takes — it is not silently absent, and a turn whose
# items exist but whose command was later refused is not reported `unfinished`.
bee --format compact sessions doctor --channel "$CHANNEL_ID" --target "$TARGET"
# → "<commandId>  answered  0.0s  0 items"
#   "    - answered turn_dropped (NO_LIVE_EXECUTION) with no turn: these words
#          never ran and never will — re-address them with `bee sessions send
#          --readdress <commandId>`"
bee sessions doctor --channel "$CHANNEL_ID" --target "$TARGET" \
  | jq '.turns[] | {turnId, commandId, answeredStage, answeredCode}'
# The `--readdress` line appears only for the two answers `--readdress` accepts
# (`turn_dropped`/NO_LIVE_EXECUTION, `turn_refused`/STALE_GENERATION) *and* only
# over a `thread.turn.start`, read from the 44220 itself. A final answer — a
# `QUEUE_FULL` drop, or the `NO_TURN_IN_FLIGHT` a cancel with nothing in flight
# earns — still gets its row, stage and code, but no recovery verb:
#   "    - answered turn_dropped (QUEUE_FULL) with no turn: these words never
#          ran and never will"
# A `thread.turn.interrupt` refused for a stale generation is answered with the
# same status and code as a refused turn, so only the command tells them apart:
# it gets the same row and no verb, because `--readdress` refuses it for
# carrying no text. Same for a commandId whose 44220 is not in the channel.
# Advising a re-send in any of those cases points at a command `send
# --readdress` refuses.

# ── create ────────────────────────────────────────────────────────────────
# Publishes one 44221 `session.create`; the brief becomes `initialTurn`.
echo 'stand up the fixture harness' | bee sessions create \
  --channel "$CHANNEL_ID" \
  --session-ref "$UMBRELLA" --genesis "$GENESIS" \
  --provider-instance "$PROVIDER_INSTANCE_REF" \
  --provider-authority "$PROVIDER_AUTHORITY_HEX" \
  --model claude-opus --title 'fixture harness' --brief - | jq .
# → {"event_id":"...","accepted":true,"message":"","commandId":"<uuid>","seated":false}

# Three flags are REFUSED here, each naming the mechanism rather than a policy:
bee sessions create --channel "$CHANNEL_ID" --provider-instance x \
  --provider-authority "$PROVIDER_AUTHORITY_HEX" --actor "$AGENT_HEX" 2>&1; echo "exit: $?"
# stderr: "--actor is refused …: an agent seat's key material is host-local
#          custody the CLI does not hold, so a seat created here would be
#          answered ACTOR_UNAVAILABLE by the provider. Create seated executions
#          from the desktop …"
# --role   → "ACTOR_ROLE_PAIR — a role and an actor are a pair and neither is
#             valid alone, and --actor is refused here."
# --driver → "a create names a provider instance (--provider-instance) and its
#             catalog authority (--provider-authority); the driver slug is
#             minted by the provider into the target it returns."
# exit: 1 for each.

# ── hire (plan D14) ───────────────────────────────────────────────────────
# Publishes one 44221 `session.hire`: a REQUEST to the umbrella's host, not a
# create. The relay checks authority on ingest (founder-or-grant on the
# umbrella, the same standing a steer needs); the host applies its own policy
# and answers by publishing a seated create — whose receipts are this hire's
# receipts — or by refusing with a 44220 turn.
bee sessions hire --channel "$CHANNEL_ID" --session-ref "$UMBRELLA" \
  --role builder --brief ./briefs/lane-c.md | jq .
# → {"event_id":"…","accepted":true,"message":"","commandId":"<uuid>",
#    "sessionRef":"…","genesisRef":"…","role":"builder",
#    "outcome":"created","detail":"the host seated 4f2c1ab9 as builder on
#     claude-primary — <cs-target>",
#    "seat":{"commandId":"…","actor":"…","seat":"4f2c1ab9\u00b7builder",
#            "role":"builder","providerInstanceRef":"claude-primary",
#            "model":"claude-sonnet-4-6","target":"…","status":"created"},
#    "code":null,"reason":null}
# exit: 0

# --genesis is resolved from the channel when omitted. Two geneses claiming one
# umbrella is an error listing both, never a coin flip:
bee sessions hire --channel "$CHANNEL_ID" --session-ref "$UMBRELLA" \
  --role builder --content 'x' 2>&1; echo "exit: $?"
# stderr (no genesis in channel): "no coding-session genesis in this channel
#   founds umbrella …" → not_found, exit 1

# The brief is required — a seat hired with nothing to do is a bug:
bee sessions hire --channel "$CHANNEL_ID" --session-ref "$UMBRELLA" --role builder \
  2>&1; echo "exit: $?"
# stderr: "one of --brief <file> or --content <text> is required: a hired
#          seat's first turn is the brief …" → user_error, exit 1

# The four non-zero outcomes, and the exit code each earns:
#   refused      → the host's policy refused; "code" is one of HIRE_OFF,
#                  HIRE_ROLE_NOT_ALLOWED, HIRE_LIMIT, HIRE_NO_IDENTITY (this
#                  computer holds no identity for that role — only its
#                  operator can fix it), HIRE_ROLE_BUSY (it holds the role and
#                  every identity that IS it is already seated in this
#                  umbrella — brief that seat instead of hiring again),
#                  HIRE_PROVIDER_NOT_ALLOWED, HIRE_MODEL_NOT_OFFERED (the
#                  model is not in the chosen provider's catalog and is no
#                  alias the host could translate) or HIRE_STALE (the request
#                  is older than the host's 15-minute answering window);
#                  "detail" appends the remedy for the code; exit 1
#   failed       → the host seated it and the PROVIDER refused the create;
#                  "code" is the receipt's own (e.g. ACTOR_UNAVAILABLE); exit 1
#   seating      → a seated create was published, no provider receipt inside
#                  60 s; exit 5
#   unconfirmed  → nothing answered at all, or --no-wait; exit 5
bee sessions hire --channel "$CHANNEL_ID" --session-ref "$UMBRELLA" \
  --role builder --content 'x' --no-wait | jq '{outcome, detail}'
# → {"outcome":"unconfirmed","detail":"the relay stored the hire; --no-wait
#     means nothing was asked what became of it"}; exit 5

# An unauthorised signer is refused BY THE RELAY, on ingest:
# stderr: "relay rejected event: restricted: only the session founder or a
#          granted operator may hire"

# A relay that predates session.hire refuses the payload as MALFORMED. The CLI
# must name the relay, never the request (WIRE RULE):
# stderr: {"error":"relay_error","message":"this relay does not accept hire
#          requests yet — it validates kind 44221 against a closed action list
#          that has no `session.hire` in it … The relay said: relay rejected
#          event: invalid: coding-session lifecycle command action type is
#          unsupported"} → exit 2
```

##### Recorded live run

Run **2026-08-27 04:00–04:06 UTC** against a local `buzz-relay` built from this
branch (`cargo build -p buzz-relay`, debug) on `http://localhost:3000`, backed
by the docker `buzz-postgres` / `buzz-redis` dev services. Identity:
`93240b3e…` (`buzz-admin generate-key`). Channel
`944a3b6e-d43c-4a72-aa05-8cc4e2473919`, umbrella
`f75b3f56-ca72-4277-a1f9-4b6aee727929`.

Three kind-44223 metadata events were published directly (a `builder` seat at
generations 1 and 2, a `verifier` seat at generation 1, all seated on the
caller's own pubkey), plus one kind-44224 `turn_dropped`/`NO_LIVE_EXECUTION`
and one kind-44221 `session.resume`. The relay has **no envelope validator for
provider-authored kinds** (`ingest.rs`: "The four provider-authored
coding-session kinds get no envelope validator"), which is why a seeder can
stand in for a provider here.

| check | result |
| --- | --- |
| `sessions status` on an empty channel | piped, no `--format`: **0 bytes**, exit 0 (NDJSON of nothing is nothing). `--format json`: `{"channel":"…","executions":[],"founders":[],"leaseSnapshotRecords":0}`, exit 0 |
| `sessions inbox` on an empty channel | `{"seats":0,"turns":[]}`, exit 0 |
| `sessions status` after seeding | 3 executions, each `seat":"93240b3e·builder"`/`·verifier`, `"live":"unknown"` (no lease answered), `"sessionRef"` echoed |
| `sessions create` (unseated, 44221) | `accepted:true`, event `1faf7a2a…` — **this is the proof that `sign_event_unchecked` + exactly-three-tags is right**; `sign_event` would have added an `auth` tag and been rejected `invalid: unsupported coding-session lifecycle command tag` |
| `sessions create --actor` | refused, exit 1, message names `ACTOR_UNAVAILABLE` |
| `sessions create --role` | refused, exit 1, message names `ACTOR_ROLE_PAIR` |
| `sessions create --driver` | refused, exit 1, message names `--provider-instance` |
| `sessions hire` | added after this run; its own recorded live run is below |
| `sessions send --to builder` (no `--session-ref`) | resolved through the caller's own seat to umbrella `f75b3f56…`, and to **generation 2** — the newest — event `5ce7e81f…`, `accepted:true` |
| boundary payload on the wire | `{"type":"thread.turn.start","text":"rebase and re-run the gate"}` — **no `deliver` key**, read back out of Postgres |
| `--deliver steer` on the wire | `{"type":"thread.turn.start","text":"…","deliver":"steer"}`, `accepted:true` |
| `sessions inbox` after two sends | both rows, oldest first, `deliver` `boundary` then `steer`, `stage:null` (no provider to answer) |
| `sessions send --to <gen-1 cs-target key>` | addressed generation 1 explicitly, `accepted:true` |
| `sessions send --readdress <that commandId>` | after seeding the `turn_dropped`/`NO_LIVE_EXECUTION`: `readdressOf` the old id, `readdressReason:"turn_dropped/NO_LIVE_EXECUTION"`, `readdressedFromGeneration:1`, `resumedBy:"93240b3e…"` (the 44221 resume's signer), new `target` = **generation 2** |
| re-addressed text on the wire | byte-identical to the original 44220's `text`, verified in Postgres |
| `--to builder --session-ref <foreign uuid>` | `not_found`, exit 1: "no seat holds role 'builder' in umbrella …; the role exists in: f75b3f56…" — the role did **not** resolve across umbrellas |
| `--readdress` of a command with no turn receipt | refused, exit 1: "has no turn receipt yet — nothing says it did not run" |
| `sessions inbox --since <last eventId>` | 0 turns |
| `sessions inbox --since <unknown id>` | `not_found`, exit 1 |
| `sessions send --reply-to` | refused, exit 1, message names the three-tag envelope and `deny_unknown_fields` |

##### Recorded live run — `sessions hire` (2026-08-28 17:30–17:50 UTC)

Against a local `buzz-relay` built from this branch (debug) on
`http://127.0.0.1:3077`, backed by the docker `buzz-postgres` / `buzz-redis`
dev services. Founder `88cfb21c…`, granted operator `8b2bd4e6…`, channel
`ddcccba6-893b-4fcb-bf29-543ddecc260d`, umbrella
`5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10`, genesis `b13cbd5f…`. The umbrella's
genesis, the host's seated creates and the provider's receipts were published
by throwaway seeders (there is no host implementation yet — that is another
lane); **everything the CLI and the relay do is real.**

| check | result |
| --- | --- |
| founder publishes a hire | `accepted:true`, event `3fe71e03…`; `--genesis` resolved from the channel automatically |
| the stored bytes | read back from Postgres: `{"type":"session.hire","sessionRef":"5b7e1c2a…","genesisRef":"b13cbd5f…","role":"builder","providerInstanceRef":null,"model":null,"brief":"Rebase the lane and run the gate."}` — the seven keys, explicit nulls |
| granted operator publishes a hire | `accepted:true`, event `3ad96681…` — a 44228 `collaborator` grant is enough |
| a stranger publishes a hire | **relay** refuses on ingest: `relay error 400: restricted: only the session founder or a granted operator may hire`, exit 2 |
| a hire naming an umbrella no genesis claims (`--genesis` forced) | **relay** refuses: `restricted: no coding-session genesis in this channel claims that sessionRef, so nothing here can authorize a hire into it`, exit 2 |
| the same, with `--genesis` omitted | refused locally before publishing: `not_found`, exit 1, message names the umbrella |
| host answers with a seated create + `created` receipt | `outcome:"created"`, `seat.seat:"8b2bd4e6·runner"`, `seat.target:"coding-session/v1\|16:claude-agent-acp10:instance-114:runner-session1:1"`, exit **0** |
| host answers with a refusal turn | `outcome:"refused"`, `code:"HIRE_OFF"`, `reason:"hiring is switched off on this computer"`, exit **1** |
| `--no-wait` | `outcome:"unconfirmed"`, detail says *nothing was asked*, exit **5** |
| no brief / empty brief / `Builder` / non-UUID `--session-ref` | `user_error`, exit 1, each naming its own rule |

**Not exercised live:** the `failed` outcome (a provider receipt refusing the
seated create) and the `seating` outcome (a create with no receipt inside 60 s)
— unit-tested only; and the *"this relay does not accept hire requests yet"*
path, which needs a relay built before this branch. No real host implements
`session.hire` yet, so the policy codes other than `HIRE_OFF` have never been
produced by anything but a seeder.

**Not exercised live, and why.** No real provider was attached, so no
`turn_queued`/`turn_started` receipt, no lease, and no transcript existed:
`status`'s `live`/`quiet <age>`/`released` branches, `openTurn`, `queuedTurns`,
and inbox `stage`/`turnId` were exercised only by the unit tests in
`crates/buzz-cli/src/commands/sessions/crew_tests.rs`. Neither was
`--deliver interrupt` end to end (nothing was running to cancel), nor the
`STALE_GENERATION` arm of `--readdress`, nor the "durably stopped" refusal, nor
a run under `BUZZ_AUTH_TAG` (which is the case the `sign_event_unchecked`
choice exists for — see the envelope note above). Those remain open until the
S4 acceptance run seats two managed agents in one umbrella.

---

### 6.14 Raw events (`bee events query`)

The debugging verb: one authenticated REQ, no contract decoding, no writes.
`--kinds` is required — the relay's p-gate answers 403 to a filter that names
none, so the CLI refuses it locally instead of turning a knowable input error
into an auth-shaped one.

```bash
# The 20 newest coding-session metadata events in a channel, reduced for scanning
bee --format compact events query --kinds 44223 --channel "$CHANNEL_ID" --limit 20 | jq .
# → [{"id":"…","kind":44223,"pubkey":"…","createdAt":"2026-08-27T…","h":"<channel>",
#    "summary":"first 120 chars of raw content, newlines collapsed"}]

# Full signed events, signature included, newest-first by (created_at, id)
bee events query --kinds 44226 --channel "$CHANNEL_ID" | jq '.[0]'

# Several kinds, an author, and a time window (RFC 3339 or Unix seconds)
bee events query --kinds 44221,44224 --channel "$CHANNEL_ID" \
  --authors "$PUBKEY" --since 2026-08-01T00:00:00Z --until 1787875200 | jq 'length'

# `--h` writes the same filter key as `--channel`, for an h-scope that is not a UUID
bee events query --kinds 44226 --h "$CHANNEL_ID" | jq 'length'

# No kinds → refused before any request
bee events query --channel "$CHANNEL_ID" 2>&1; echo "exit: $?"
# stderr: {"error":"user_error","message":"--kinds is required: a filter with no kinds is refused by the relay with 403."}
# exit: 1

# A malformed author, id, channel, kind, or timestamp is refused locally too
bee events query --kinds 44223 --authors deadbeef 2>&1; echo "exit: $?"
# stderr: {"error":"user_error","message":"--authors must be a 64-character lowercase hex string: deadbeef"}
# exit: 1
bee events query --kinds 44223 --since yesterday 2>&1; echo "exit: $?"
# exit: 1

# Nothing matched is an answer, not a failure
bee events query --kinds 44226 --channel "00000000-0000-0000-0000-000000000000"; echo "exit: $?"
# stdout: []
# exit: 0
```

What it deliberately does not do: no live subscription (`events sub`), no write
path (`events publish`), and no `--search` — NIP-50 belongs to
`messages search`. `--format compact` never parses `content`, so an event whose
payload is malformed — usually the exact event a raw query is looking for —
still prints a row, with the raw content as its `summary`.

### 6.15 Who founded a session (`sessions status` / `sessions list`)

Both commands name the founder of each execution. This exists because a probe
aimed at "a 3-day-quiet session" once landed in someone else's session
(`docs/SESSION_STATE.md` item 73): the row said what was running, never whose
it was.

`--format json` is not optional in the first line below: it reads `founders`
and `.executions`, both of which live only in the envelope, and a bare piped
`sessions status` prints NDJSON rows instead (see the output-shape note in
§6.13.3). Bare, this exact command fails `jq: error (at <stdin>:1): Cannot
iterate over null`.

```bash
bee --format json sessions status --channel "$CHANNEL_ID" | jq '{founders, executions: [.executions[] | {target, seat, founder, createSigner}]}'
# → {"founders":["3d3b7169a13a8311b480bdfce85b4a0c7ff9b185832cbc6e547db7bbcf96c05e"],
#    "executions":[{"target":"coding-session/v1|…","seat":"1ddd35c6·builder",
#      "founder":"3d3b7169a13a…","createSigner":"3d3b7169a13a…"}, …]}
bee --format compact sessions status --channel "$CHANNEL_ID" | jq .
# → [{"target":"…","seat":"…","founder":"3d3b7169","live":"quiet 2h",…}]

bee sessions list --channel "$CHANNEL_ID" | jq '[.[] | {target, founder, createSigner}]'
bee --format compact sessions list --channel "$CHANNEL_ID" | jq .
```

Two fields, each derived once (`crates/buzz-cli/src/commands/sessions/crew.rs`,
`build_founder_index`) and used by both commands:

- **`createSigner`** — the pubkey that signed the `session.create` (44221) a
  lifecycle receipt (44224) joined to this execution. Only the provider the
  create *named* may answer it, so a stranger's receipt joins nothing.
- **`founder`** — the pubkey that signed the genesis (44226) that create named
  by event id, when that genesis is in the channel and founds the umbrella the
  create claimed.

`sessions status` also prints a channel-level `founders` array: every distinct
non-null founder above. It is an envelope key, so it appears under `--format
json` (or `--no-json-lines`) and not in the NDJSON rows a bare piped call
prints — the per-execution `founder` field is in both.

**`null` means the channel does not contain the record that would say** — a
create that aged out, a create no provider ever answered, a create with no
`genesisRef`, or a genesis published elsewhere. It never falls back to the
provider's key: the provider signs *every* execution in the channel, so that
fallback would make every session look like it belonged to the same person.
A `session.resume` founds nothing, so a resume signer never appears in either
field; the founding create's signer carries forward to every later generation
of the same execution instead.

Compact prints the short (8-character) form of the founder pubkey, the same
form `seat` uses; JSON prints full 64-character hex.

**What it costs.** `sessions status` already fetched 44221 (it needs resumes)
and now also fetches 44226 — genesis events are one per umbrella, so the
addition is roughly *one event per session*, not per turn. `sessions list`
previously fetched only 44223 + 44224 and now also fetches 44221 + 44226: one
create per execution plus one resume per resume, plus one genesis per umbrella.
Both stay a single `#h`-scoped `POST /query` with explicit `kinds`.

---

## 7. Error Path Testing

Verify the CLI produces correct JSON on stderr and correct exit codes.

```bash
# Exit 1: Invalid UUID
bee channels get --channel "not-a-uuid" 2>&1; echo "exit: $?"
# stderr: {"error":"user_error","message":"invalid UUID: not-a-uuid"}
# exit: 1

# Exit 1: Invalid hex64
bee messages delete --event "not-hex" 2>&1; echo "exit: $?"
# stderr: {"error":"user_error","message":"must be a 64-character hex string: not-hex"}
# exit: 1

# Exit 1: Invalid --type value (clap validates the enum — multi-line error)
bee channels create --name x --type invalid --visibility open 2>&1; echo "exit: $?"
# stderr: {"error":"user_error","message":"error: invalid value 'invalid' for '--type <CHANNEL_TYPE>'\n  [possible values: stream, forum]\n..."}
# exit: 1

# Exit 1: Invalid --direction value
bee messages vote --event "$(printf '0%.0s' {1..64})" \
  --direction sideways 2>&1; echo "exit: $?"
# exit: 1

# Exit 1: Empty body guard
bee users set-profile 2>&1; echo "exit: $?"
# exit: 1 (at least one field required)

# Exit 3: No auth configured
env -u BUZZ_PRIVATE_KEY \
  cargo run -p buzz-cli -- channels list 2>&1; echo "exit: $?"
# stderr: {"error":"auth_error","message":"auth error: BUZZ_PRIVATE_KEY is required (use --private-key or set env var)"}
# exit: 3

# Not-found returns null, not an error (exit 0)
bee channels get --channel "00000000-0000-0000-0000-000000000000"
# stdout: null
# exit: 0
```

---

## 8. Auth Testing

Test authentication.

```bash
# Private key (BUZZ_PRIVATE_KEY)
BUZZ_PRIVATE_KEY="nsec1..." bee channels list | jq .
# Should succeed

# No auth → exit 3
env -u BUZZ_PRIVATE_KEY \
  cargo run -p buzz-cli -- channels list 2>&1; echo "exit: $?"
# stderr: {"error":"auth_error","message":"auth error: BUZZ_PRIVATE_KEY is required (use --private-key or set env var)"}
# exit: 3
```

---

## 9. Cleanup

```bash
# Delete test channels
bee channels delete --channel "$CHANNEL_ID" | jq .
bee channels delete --channel "$FORUM_ID" | jq .
```

---

## 10. Checklist

| # | Command | Tested | Notes |
|---|---------|:------:|-------|
| 1 | `messages send` | ☐ | Basic, reply, broadcast, mentions, stdin |
| 2 | `messages send-diff` | ☐ | Stdin, metadata, branch/PR |
| 3 | `messages edit` | ☐ | |
| 4 | `messages delete` | ☐ | |
| 5 | `messages get` | ☐ | With limit |
| 6 | `messages thread` | ☐ | |
| 7 | `messages search` | ☐ | With limit |
| 8 | `messages vote` | ☐ | Up and down |
| 9 | `channels list` | ☐ | With visibility, member |
| 10 | `channels get` | ☐ | |
| 11 | `channels create` | ☐ | Stream and forum |
| 12 | `channels update` | ☐ | |
| 13 | `channels topic` | ☐ | |
| 14 | `channels purpose` | ☐ | |
| 15 | `channels join` | ☐ | |
| 16 | `channels leave` | ☐ | |
| 17 | `channels archive` | ☐ | Needs admin:channels |
| 18 | `channels unarchive` | ☐ | Needs admin:channels |
| 19 | `channels delete` | ☐ | Needs admin:channels |
| 20 | `channels members` | ☐ | |
| 21 | `channels add-member` | ☐ | Needs admin:channels |
| 22 | `channels remove-member` | ☐ | Needs admin:channels |
| 23 | `canvas get` | ☐ | |
| 24 | `canvas set` | ☐ | Direct and stdin |
| 25 | `reactions add` | ☐ | |
| 26 | `reactions remove` | ☐ | |
| 27 | `reactions get` | ☐ | |
| 28 | `dms list` | ☐ | |
| 29 | `dms open` | ☐ | |
| 30 | `dms add-member` | ☐ | Needs messages:write |
| 31 | `users get` | ☐ | Self, single, batch |
| 32 | `users set-profile` | ☐ | |
| 33 | `users presence` | ☐ | |
| 34 | `users set-presence` | ☐ | online, away, offline |
| 35 | `workflows list` | ☐ | |
| 36 | `workflows create` | ☐ | |
| 37 | `workflows update` | ☐ | |
| 38 | `workflows delete` | ☐ | |
| 39 | `workflows trigger` | ☐ | |
| 40 | `workflows runs` | ☐ | |
| 41 | `workflows get` | ☐ | |
| 42 | `workflows approve` | ☐ | Validation only (needs approval gate); bare = approve, `--approved false` = deny |
| 43 | `feed get` | ☐ | |
| 44 | `social publish` | ☐ | |
| 45 | `social set-contacts` | ☐ | |
| 46 | `social event` | ☐ | |
| 47 | `social notes` | ☐ | |
| 48 | `social contacts` | ☐ | |
| 49 | `repos create` | ☐ | |
| 50 | `repos get` | ☐ | |
| 51 | `repos list` | ☐ | |
| 52 | `repos protect list` | ☐ | Empty/populated rules; unknown rules visible; malformed rule reported in validation_error |
| 53 | `repos protect set` | ☐ | Create and replace complete exact-ref rule; verify metadata is preserved |
| 54 | `repos protect remove` | ☐ | Remove exact ref; missing rule → NotFound |
| 55 | `upload file` | ☐ | |
| 56 | `pack validate` | ☐ | Local, no relay |
| 57 | `pack inspect` | ☐ | Local, no relay |
| 58 | `notes set` | ☐ | First publish, edit/carry, --clear-tags, ambiguity, empty-stdin guard |
| 59 | `notes get` | ☐ | By name, by naddr, --content-only, cross-author, ambiguous → exit 1 |
| 60 | `notes ls` | ☐ | Own, --author all, --tag, --limit |
| 61 | `notes rm` | ☐ | Delete→get 404, double-delete idempotent, missing slug → NotFound |
| 62 | `users set-status` | ☐ | Text+emoji, text only, emoji-only (`--text ""`), `--clear`, `--clear` + `--text` → exit 1 |
| 63 | `sessions list` | ☐ | Empty channel → `[]`; compact keeps target/title/status/model/founder/createdAt |
| 64 | `sessions transcript` | ☐ | `--target` and `--session`; md turns + tool outcomes; jsonl seq numerically ordered |
| 65 | `sessions tools` | ☐ | Call/error counts, error rate, `itemKinds` incl. `other`; `--target` narrows |
| 66 | `sessions export` | ☐ | Files + manifest.json; non-empty `--out` refused with exit 1 |
| 67 | turn-stage receipts (kind 44224, §6.13.1) | ☐ | NOT YET RUN LIVE — `sessions list`/`transcript` unaffected by a turn receipt on an otherwise-known or unknown target |
| 68 | `events query` | ☐ | `--kinds` required (verbatim refusal, exit 1); compact row survives non-JSON content; empty result → `[]`, exit 0 |
| 70 | `sessions hire` (44221 `session.hire`) | ☑ | founder + granted-operator accepted, stranger and unknown-umbrella refused by the relay, host `created` (exit 0) and `refused`/HIRE_OFF (exit 1), `--no-wait` unconfirmed (exit 5). Open: the `failed`/`seating` outcomes and the old-relay sentence |
| 69 | `sessions status` / `list` founder | ☐ | `founder`/`createSigner` per row, `founders` array on `--format json` status (an envelope key — not in bare piped NDJSON); `null` when the channel holds no joined create; never the provider's key |
