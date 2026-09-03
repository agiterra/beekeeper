# buzz-cli Live Testing Guide

Manual testing runbook for verifying every CLI command against a local relay.
An agent or developer follows this step by step, running each command and
checking the output.

Routed seats exercise this runbook too.

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
# create. The relay checks authority on ingest: founder or active operator may
# hire any role; an active lead seat may hire only a non-lead role. The host
# applies its own policy
# and answers by publishing a seated create — whose receipts are this hire's
# receipts — or by refusing with a 44220 turn.
bee sessions hire --channel "$CHANNEL_ID" --session-ref "$UMBRELLA" \
  --role builder --brief ./briefs/lane-c.md | jq .
# → {"event_id":"…","accepted":true,"message":"","commandId":"<uuid>",
#    "sessionRef":"…","genesisRef":"…","role":"builder",
#    "outcome":"created","detail":"the host seated 4f2c1ab9 as builder on
#     claude-primary — <cs-target>",
#    "seat":{"commandId":"…","actor":"…","seat":"4f2c1ab9\u00b7builder",
#            "createEventId":"…","receiptEventId":"…",
#            "role":"builder","providerInstanceRef":"claude-primary",
#            "model":"claude-sonnet-4-6","target":"…","status":"created"},
#    "seatGrantEventId":"…","seatGrantAccepted":true,
#    "seatGrantAlreadyActive":false,"seatGrantError":null,
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

# The non-zero outcomes, and the exit code each earns:
#   refused      → the host's policy refused; "code" is one of HIRE_OFF,
#                  HIRE_ROLE_NOT_ALLOWED, HIRE_LIMIT, HIRE_NO_IDENTITY (this
#                  computer holds no identity for that role — only its
#                  operator can fix it), HIRE_ROLE_BUSY (it holds the role and
#                  every identity that IS it is already seated in this
#                  umbrella — brief that seat instead of hiring again),
#                  HIRE_PROVIDER_NOT_ALLOWED, HIRE_MODEL_NOT_OFFERED (the
#                  model is not exactly in the chosen provider's catalog;
#                  this path has no aliases or translations) or HIRE_STALE
#                  (the request is older than the host's 15-minute answering window);
#                  "detail" appends the remedy for the code; exit 1
#   failed       → the host seated it and the PROVIDER refused the create;
#                  "code" is the receipt's own (e.g. ACTOR_UNAVAILABLE); exit 1
#   seating      → a seated create was published, no provider receipt inside
#                  120 s; exit 5
#   unconfirmed  → nothing answered at all, or --no-wait; exit 5
#   created_ungranted → provider-created execution is live but signed evidence
#                  or accepted `grant-seat` authority failed; output retains
#                  create/receipt ids plus `seatGrantError`; do not hire again;
#                  repair it with `bee sessions seat-repair` (§6.13.4); exit 1
#   ambiguous    → MORE THAN ONE seated create for this role verifies against
#                  its own receipt, so which seat this hire got is genuinely
#                  contested. The CLI refuses to choose — the two tie-breakers
#                  that look obvious (earliest, newest) both read the author's
#                  own `created_at`, which is the defect T2.5 removed — names
#                  both create event ids, and prints the `seat-repair` command
#                  to settle it deliberately. Nothing is written; exit 1
bee sessions hire --channel "$CHANNEL_ID" --session-ref "$UMBRELLA" \
  --role builder --content 'x' --no-wait | jq '{outcome, detail}'
# → {"outcome":"unconfirmed","detail":"the relay stored the hire; --no-wait
#     means nothing was asked what became of it"}; exit 5

# An unauthorised signer (including a lead trying to hire another lead) is
# refused BY THE RELAY, on ingest. The reason names the accepted founder /
# operator / non-lead-only lead authority rule.

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
| a stranger publishes a hire | **historical relay build** refused on ingest: `relay error 400: restricted: only the session founder or a granted operator may hire`, exit 2. Current acceptance also recognizes an active lead seat for non-lead hires; that matrix still needs the re-run tracked in row 70 below. |
| a hire naming an umbrella no genesis claims (`--genesis` forced) | **relay** refuses: `restricted: no coding-session genesis in this channel claims that sessionRef, so nothing here can authorize a hire into it`, exit 2 |
| the same, with `--genesis` omitted | refused locally before publishing: `not_found`, exit 1, message names the umbrella |
| host answers with a seated create + `created` receipt | `outcome:"created"`, `seat.seat:"8b2bd4e6·runner"`, `seat.target:"coding-session/v1\|16:claude-agent-acp10:instance-114:runner-session1:1"`, exit **0** |
| host answers with a refusal turn | `outcome:"refused"`, `code:"HIRE_OFF"`, `reason:"hiring is switched off on this computer"`, exit **1** |
| `--no-wait` | `outcome:"unconfirmed"`, detail says *nothing was asked*, exit **5** |
| no brief / empty brief / `Builder` / non-UUID `--session-ref` | `user_error`, exit 1, each naming its own rule |

**How a hire now chooses its answer (T2.5).** It does not choose by time.
Every seated create for this `(umbrella, role)` published after the hire's own
`since` cutoff is assessed, and the one whose provider receipt cryptographically
verifies is the answer — the same rule `seat-repair` already used. So an earlier
create that no provider ever answered can no longer shadow the seat that is
actually running, and a 44221 anyone can publish can no longer park the hire on
a create that will never verify. To check it by hand: publish a seated create
for a role, let it go unanswered, run the hire, and confirm the outcome names
the *verified* create's `commandId` rather than the older one.

**Not exercised live:** the `failed` outcome (a provider receipt refusing the
seated create), the `seating` outcome (a create with no receipt inside 120 s),
and the `ambiguous` outcome (two verifying creates — reachable only by seating
the same role twice with two answered providers)
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

#### 6.13.4 `seat-repair` — grant the seat a hire created and lost

The recovery path for the one hire failure nothing else undoes: the host
seated the role, the provider answered `created`, and no `grant-seat` ever
reached the accepted 44228 chain — because the receipt landed after the hire's
window closed (cleantest, 2026-09-01: receipt signed 19:11:28, three seconds
after the 19:11:25 hire, and the then-60 s window still missed it), or because
the grant write itself failed. **Re-hiring cannot recover it** — a fresh hire
takes a `since` cutoff before its own request, which excludes the create that
already exists, and would seat a *second* agent. `seat-repair` never builds a
44221; the only event it can submit is one 44228 `grant-seat`.

```bash
# The repair. One read of the channel, no wait and no poll.
bee sessions seat-repair --channel "$CHANNEL_ID" --session-ref "$UMBRELLA" \
  --actor "$ACTOR_PUBKEY"
# → {"outcome":"granted","actor":"…","role":"builder","createEventId":"…",
#    "receiptEventId":"…","seatGrantEventId":"…","detail":"appended an
#    accepted grant-seat …"}; exit 0

# Safe to re-run: the second run writes nothing.
bee sessions seat-repair --channel "$CHANNEL_ID" --session-ref "$UMBRELLA" \
  --actor "$ACTOR_PUBKEY"
# → {"outcome":"already_granted", …}; exit 0

# --genesis is resolved from the channel unless two geneses claim the label.
# --format compact keeps {outcome, actor, role, seatGrantEventId}.
```

The outcomes and the exit code each earns:

| outcome | means | exit |
| --- | --- | --- |
| `granted` | a new accepted `grant-seat` now names this exact actor-role pair | 0 |
| `already_granted` | the pair already held an accepted seat; nothing written | 0 |
| `no_receipt_yet` | no candidate create has a **bound** provider receipt — nothing yet proves an execution to grant authority for; every candidate is named, with any unbound receipts counted; nothing written | 5 |
| `refused` | the provider refused a create, a bound receipt failed the full chain, no founder-signed create names the actor, or the actor holds a **different** role; nothing written | 1 |
| `ambiguous` | the verifying creates disagree about the **role** this actor holds, and the accepted authority chain does not already settle it; each disputed role is named with the commandIds claiming it, and nothing is written | 1 |

Every outcome prints the same seven keys on stdout, including the ones with no
create to name (`role`, `createEventId`, `receiptEventId` come back `null`).

**How the create is chosen — evidence, never `created_at`.** This is the whole
of the repair's discovery, and the rule is that a self-asserted timestamp
decides nothing:

1. Candidates are every 44221 seated create for `(session-ref, actor)` **signed
   by the umbrella's genesis signer**. A create from any other key is not a
   candidate at all, so no channel member can park the repair by publishing one.
2. For each candidate, every receipt naming its commandId is filtered by the
   same binding checks `verify_hire_evidence` applies — kind, signer equal to
   the create's `providerAuthorityPubkey`, exact tags, commandId, and
   target/provider-instance parity. An unbound receipt is **ignored and
   counted**, never fatal: a later forgery carrying the commandId must not be
   able to deny the only recovery path.
3. A candidate verifies when it has a bound `created`-class receipt *and* the
   full `verify_hire_evidence` chain passes, provider metadata included.
4. The verifying candidates are grouped by the write they imply — `(actor,
   role)`, which is all a `grant-seat` carries. Candidates are first deduped by
   commandId, so a retried submit (same command, new event id) is one candidate.
   - **One group** — however many candidates are in it — is repaired: every
     member implies byte-identical authority, so there is nothing to choose
     between. This is what keeps the idempotent second run working on an
     umbrella that hired the same actor twice.
   - **More than one group** is a real disagreement about the role, and only
     the founder can settle it. If the accepted 44228 chain *already* seats
     this actor in one of the disputed roles, that recorded decision wins and
     the run reports `already_granted`. Otherwise: `ambiguous`, exit 1, nothing
     written, each disputed role named with its commandIds.
   - **None verifying** reports the provider's refusal if there is one,
     otherwise `no_receipt_yet`, and **always names every candidate** rather
     than one create as if it were the only one.

`ambiguous` means *disagreement about what would be written*, never a count.
It cannot be cleared by re-running alone — both creates are signed history and
neither can be withdrawn — so the way out is to grant the role you mean on the
umbrella's authority chain and run again. Note that `bee sessions grant` covers
the collaborator/viewer tiers only; role seats are published by the founder's
Desktop hire host (and by this command).

Other rules, and the reason each exists:

- **The role comes from the create, never from a flag.** A repair that took
  the role from its caller could grant a role no host ever seated.
- **An actor already seated in another role is never overwritten** — revoke
  that seat first.
- **Exit 5 invites a retry, so the ignored count matters.** A provider bug that
  emits a persistently mis-targeted receipt reads as `no_receipt_yet` for ever.
  The `N unbound receipt(s) ignored` clause in `detail` is what tells an
  operator to stop waiting and go look at the provider.
- **It never re-hires.** Asserted against a recording relay in
  `crates/buzz-cli/src/commands/sessions/seat_repair_tests.rs`: every case
  checks that no kind-44221 event reached the wire.

**Not exercised live yet.** The unit suite drives the whole command against a
local recording relay; the live acceptance against cleantest is tracked with
the 2026-09-01 batch.

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

### 6.16 The model catalog, and the registry checked against it

`bee sessions catalog` prints the live kind:44222 provider catalog. **That
catalog is the only list of models this product offers.** A create, a hire, or
a rubric row naming an id that is not in it is naming something nobody is
serving, and the right answer is to say so and refuse — never to translate the
id onto a neighbouring one that happens to be offered.

```bash
# Every provider/model pair on offer in this channel.
bee sessions catalog --channel "$CHANNEL_ID"

# The narrow view: provider, model, default flag, window, vendor.
bee --format compact sessions catalog --channel "$CHANNEL_ID"
```

**What each field means.** `contextWindow`, `family`, `vendor` and `deprecated`
are per-model metadata the publisher carries only where it knows the fact:
`contextWindow` from the driver's own report or the provider's recorded table
(`crates/buzz-session-provider/src/context_window.rs`), `family` from the same
kind of table, `vendor` from a runtime that can serve exactly one. `null` in
any of them means **nobody said** — it is never a default a caller may assume,
and the publisher omits a fact rather than guessing it. `deprecated` is `null`
on every row today because no adapter reports deprecation; the field is there
so one that starts to needs no schema change.

**Verify, on a host running the provider:**

- Every id the picker offers appears here, and nothing else does. Cross-check
  against a live seat: `bee --format compact sessions status --channel
  "$CHANNEL_ID"` prints the `model` a running execution is actually on, and
  that string must be one of the ids this command lists.
- `--format json` names the `signer`, `revision` and `eventId` on every row.
  Two provider hosts in one channel are **two** catalogs with two independent
  revision counters, and this command reconciles nothing: expect rows from both
  signers, not a merged one.
- A catalog body that does not parse is listed under `malformed` with the exact
  reason, not skipped. To see it, publish a 44222 whose body has its keys in the
  wrong order — the reader compares bytes, so it comes back
  `catalog body does not re-serialize to its own bytes …`.

`bee sessions registry check` compares the **model registry** to that same
catalog. Brian's ruling of 2026-08-30 replaced item 94's flat rubric table with
`team/model-registry.yaml`: eleven execution targets, ten 1-5 operational
priors each, and a `rating` block on every row saying `operational_opinion` /
`confidence: low` with its author and date. Nothing may render those priors as
measurements.

```bash
# From inside a checkout; the registry is found by walking up from $PWD.
bee sessions registry check --channel "$CHANNEL_ID"

# Or name it.
bee --format compact sessions registry check --channel "$CHANNEL_ID" \
  --registry ./team/model-registry.yaml
```

**The two directions are deliberately not symmetric, and only one fails.**

- `stale` — execution targets the catalog offers today that **no registry row
  covers**. This is staleness: a target on offer with nothing behind it, which
  the lead will reach for anyway. Exit **4**.
- `dormant` — registry rows the catalog does **not** offer today. **Legal, and
  exit 0.** The registry is allowed to hold an opinion about a model this host
  is not serving right now — a model coming back, or a second host's inventory.
  A dormant row is reported and never counted against freshness.
- `variants` — offered ids a row covers by the base rule without naming
  literally. Informational.

A bracket suffix is a variant of its base: `gpt-5.6-sol[high]` and
`gpt-5.6-sol[max]` are one model at two effort levels, and a row naming
`gpt-5.6-sol` has decided about both. `default` is a provider's pointer at
whatever the host is set to — never a row, never a gap.

**Verify:**

- With the shipped registry against the live catalog: exit 0, `"isStale":
  false`, `stale` and `dormant` both empty, and 33 entries under `variants`
  (the effort brackets). Confirm with `echo $?`.
- Add a model to the provider's `BUZZ_CSP_RUNTIMES` (or let a runtime discover
  a new one) and re-run: the new id shows up under `stale` and the exit code is
  4. **This is the "what happens when a new model gets added?" case** — the
  registry does not silently absorb it and the command does not pass.
- Stop a provider so its models leave the catalog: its rows show up under
  `dormant` and the exit code is still **0**. A registry that knows a model
  nobody is serving today is not a stale registry, and a check that said
  otherwise would be red every time a host went down.
- Run it from a subdirectory with no `--registry`: it still finds the file by
  walking up. Run it somewhere with no checkout: it fails naming **every**
  directory it tried.

### 6.17 Routing — which execution target, and why

> "The lead chooses the capability required. The router chooses the execution
> target."
>
> "Select the least expensive execution target whose expected failure mode is
> acceptable for the task."

`bee sessions route` is that router. You name a **class** and a **risk triple**;
it names the provider, the model and the effort. **There is no `--model` on
this command** — a lead that names a model has skipped the only step that can
be checked.

```bash
# A standard builder job.
bee sessions route --channel "$CHANNEL_ID" --class builder --risk 3,3,2

# A bounded mechanical job, which is the only kind Spark may take.
bee sessions route --channel "$CHANNEL_ID" --class runner --risk 1,1,1 --scope bounded

# Deliberately sample a challenger, and mark the record so the result counts.
bee sessions route --channel "$CHANNEL_ID" --class builder --risk 3,3,2 --challenger-sample

# A verifier for work a codex builder did: cross-provider by rule.
bee sessions route --channel "$CHANNEL_ID" --class verifier --risk 3,3,2 \
  --counterpart-provider codex-primary

# Just the routing record — the host's answer shape, NOT what a hire carries.
# For the object a hire attaches, read `.proposed` from --format json.
bee --format compact sessions route --channel "$CHANNEL_ID" --class architect --risk 5,4,4
```

**The order, and why it is an order rather than a score.** Live catalog →
hard requirements (modality, tools, context window, known failure modes,
per-target constraints) → class gate (every numeric minimum) → risk tier →
effort → **then** the cheapest expected accepted completion among the
survivors. Cost and speed are consulted only after every gate has passed, so
they can never compensate for a capability deficit. There is no weighted
product anywhere in it, and `capability_match × cost_efficiency × velocity` is
the thing the ruling explicitly forbids.

**The tier is derived, never passed.** Risk = impact × uncertainty ×
irreversibility, each 1-5, so 1-125: **1-8 FAST** (effort `low`), **9-39
STANDARD** (`medium`), **40-125 DEEP** (`high`). `--tier` is refused with that
explanation rather than accepted — a tier a caller can set is a risk assessment
nobody made. The router never purchases `xhigh`, `max` or `ultra`; those are
human override only, and `Effort` in
`crates/buzz-core/src/coding_session_routing.rs` cannot even hold them. It also
never escalates effort after a failure: a failed high goes to a **different
target or a reviewer**, not to more thinking tokens on the same model.

**Review is not a synonym for deep.** `reviewRequired` fires on the spec's own
trigger list — risk ≥ 40, irreversibility ≥ 4, or any of
`securityBoundary`, `contractChange`, `outsidePlan`, `builderUncertain`,
`testsInsufficient`, `leadRequests` under `--review-flags`. A cheap fast job
that touches an auth boundary needs a reviewer; an expensive deep job does not
on that ground alone.

**The cost formula, spelled out.** `--format json` prints it verbatim under
`costFormula`:

```text
cost_prior    = 6 − costEfficiency     (1.0 cheapest … 5.0 dearest)
latency_prior = 6 − velocity           (1.0 fastest  … 5.0 slowest)
retry_prior   = expected attempts to acceptance (1.0 for everything until telemetry)

expected_cost = retry_prior × (cost_prior + latency_prior)
```

The two priors add because they are two costs paid on the same attempt; the
retry prior multiplies because it counts attempts. **List price is not a term.**
It is recorded per row and printed as `listPriceUsdPerM`, because the ruling is
that our real cost is quota lanes (Claude subscription vs Codex plan), not API
list price. A target whose `costEfficiency` nobody scored is ranked on latency
alone, **after** every target that has a cost prior — never guessed cheap.

**Verify:**

- `--class builder --risk 3,3,2` → `claude-primary/sonnet`, effort `medium`,
  tier `standard`. Terra appears in the table as `rejected` with standing
  `challenger`, because a challenger holds no route until sampled.
- Add `--challenger-sample` to that same command → `codex-primary/gpt-5.6-terra[medium]`
  and `"challengerSample": true` in the record. That is the Terra question
  answered: it demonstrates a purpose on sampled jobs, and the record says the
  result should be attributed to a sample.
- `--class architect --risk 5,4,4` → `codex-primary/gpt-5.6-sol[high]`, runner-up
  `claude-primary/opus[1m]`, `reviewRequired: true` with reasons
  `["risk 80 >= 40", "irreversibility 4 >= 4"]`.
- `--class runner --risk 1,1,1` **without** `--scope`: Spark is `rejected` and
  the detail says the task's scope was never stated. **Unstated is not
  bounded.** Add `--scope bounded` and it becomes `eligible` — and still is not
  chosen, because Brian recorded **no** cost prior for it and a target with no
  cost prior cannot win a cheapest-completion comparison. That is disclosed in
  the table (`"costPrior": null`), never smoothed into a guess.
- `--class lead --risk 4,4,3`: exactly three targets are eligible —
  `gpt-5.6-sol`, `opus[1m]`, `claude-fable-5[1m]` — which is Brian's §4 seed.
  Haiku is the cheapest thing in the registry and is `rejected`: cost cannot buy
  its way past a class gate.
- `--class runner --risk 1,1,1 --profile '{"taste":5.0}'`: **exit 4**, and the
  message names the binding trait, the minimum it wanted, the best score
  anything available actually has, and one line per rejected row. It does not
  fall back to the smartest model, and it does not fall back at all.
- Every rejected row in `--format json` carries a `detail` sentence. A rejection
  with no reason is a bug.
- Any fact that gated a candidate carries its provenance under `factsUsed`. Note
  that `tools` is **lane-drafted** — the 44222 catalog publishes no tool
  metadata — and the researcher gate depends on it. If those values are wrong,
  that gate is wrong, and `factsProvenance` in the registry says so out loud.

**Routing a hire — the hire asks, the host answers.** A hire carries the
routing **REQUEST**, never the routing record:

```json
{"class":"builder","risk":{"impact":3,"uncertainty":3,"irreversibility":2},
 "proposed":{"chosen":{"provider":"claude-primary","model":"sonnet","effort":"medium"},
             "runnerUp":{"provider":"codex-primary","model":"gpt-5.6-luna[medium]","effort":"medium"},
             "reason":"…","registryVersion":1,"catalogRevision":7}}
```

`class` and `risk` are required. `profile`, `override`, `challengerSample`,
`reviewFlags` and `proposed` are **omitted** when they have nothing to say —
never written as an explicit `null`. `risk` carries **three** keys and no
`score`: the product is arithmetic the host does, and a score a requester can
set is a number that can disagree with its own factors.

`bee sessions hire --class builder --risk 3,3,2 …` still runs the router
locally, but only for **disclosure**: what it got is attached as `proposed`,
the host routes for itself against its own live catalog, and any disagreement
comes back on the create as `routing.proposedDisagreement`. A local router that
cannot answer (no readable registry, a catalog this machine cannot fetch, a
class nothing here clears) is **not** fatal — the hire goes out without
`proposed` and the CLI's own output says why, under `proposedUnavailable`. The
founder's host is the one that decides, and refusing here for a target *this*
machine cannot see would be a refusal nobody asked for.

The hire's top-level `model` and `providerInstanceRef` are written **only** for
an override, and then they equal `override.model`. A routed hire that filled
them in with its own pick would be dictating a target while claiming to ask a
question — which is exactly what shipped on 2026-08-30 and was dropped in
silence.

`--override-model` is the one way a hire dictates a target. It needs `--class`,
`--risk` and `--because` (an unexplained override is indistinguishable from a
bug), and it must name something the catalog offers; the router's own pick
survives on the create's record as `runnerUp` so the table shows what was
displaced.

```bash
bee sessions hire --channel "$CHANNEL_ID" --session-ref "$UMBRELLA" \
  --role builder --risk 3,3,2 --class builder --content 'Rebase the lane.'

bee sessions hire --channel "$CHANNEL_ID" --session-ref "$UMBRELLA" \
  --role builder --class builder --risk 3,3,2 \
  --override-model 'opus[1m]' --because 'Brian asked for Opus on this one' \
  --content 'Rebase the lane.'
```

`--model` **without** `--class` is still an unrouted hire: no `routing` key at
all, and the hire is byte-identical to the seven-key form that shipped before
the router existed.

**Verify:**

- `bee --format json sessions route … | jq .proposed` is exactly the object a
  hire attaches. Copy *that*, never `.routing` — the record on a hire is
  refused by the relay, by the key that does not belong.
- A hire whose `routing` does not parse is answered `HIRE_MALFORMED`, and the
  reason names the failing key. It is never dropped: on 2026-08-30 a routed
  hire the relay had accepted was classified malformed by the host and
  discarded with no kind:44220, no log line and nothing on screen, and the
  lead waited fifteen minutes for an answer that was never coming.
- `bee sessions hire --override-model X` without `--because` exits 1 naming
  the missing flag; without `--class` it exits 1 pointing at `--model` for an
  unrouted hire.

**The shared fixtures, and what they pin.**
`testdata/routing/hire-request-fixture.json` holds three requests (a fast
builder with no override; a standard builder with an override and its
`because`; a deep architect with review flags, a profile and a challenger
sample) plus the two shapes that must be refused. Its counterpart
`testdata/routing/create-record-fixture.json` holds the three records that
answer them, including one that discloses a `proposedDisagreement`. Three
implementations read those two files: `buzz-core`'s validator
(`crates/buzz-core/src/coding_session_lifecycle_command.rs`), this CLI's
emitter (`crates/buzz-cli/src/commands/sessions/crew_tests.rs`) and the
desktop's parser. That is what makes them one contract rather than three that
happen to agree today.

**The fixture both implementations are pinned to.**
`testdata/routing/live-catalog-665076ce.json` holds the real 46-pair catalog
this relay served on 2026-08-30 plus six recorded decisions. The Rust router
asserts them in
`crates/buzz-core/src/coding_session_routing.rs::every_recorded_decision_in_the_fixture_still_holds`,
and the desktop router pins to the same file, so the two cannot silently
disagree. A change in routing behaviour has to be a deliberate edit to that
file.

**Where the same rule lives on the desktop.** The Agents screen carries a
staleness badge (`data-testid="rubric-stale-badge"`) whose module applies the
identical comparison this command makes, pinned to the same
`testdata/routing/live-catalog-665076ce.json` fixture, so the two cannot drift.
Today that badge reads **"unknown (pack not readable)"** on every launch, and
that is the honest state rather than a bug in the badge: the renderer has no way
to read a role pack's files. The only role-pack access the app has is
`scanProjectRolePacks` and `pickCrewRolePacksDirectory`
(`desktop/src/shared/api/tauriTeams.ts:327` and `:341`), and both return a role,
a name and a `packDir` — never file content. The badge says so out loud and
points at this command; when a pack-file reader exists, the one call site in
`AgentsView.tsx` is all that changes.

---

### 6.18 Who this signer is (`sessions whoami`)

No channel, no arguments — one JSON object naming exactly four keys, always
present, `null` rather than omitted when unknown:

```bash
bee sessions whoami
# → {"pubkey":"1ddd35c6…","display_name":"Bob","relay_url":"https://hive.agiterra.org","role":"builder"}
```

- **`pubkey`** — the signer this process is configured with (`client.keys()`),
  never a provider key or an unverified env string.
- **`display_name`** — the relay's kind:0 name for that pubkey, or `null` when
  the relay holds none. A *failed* kind:0 lookup is not `null`: it exits 2
  with the CLI's standard error envelope and prints no object at all.
- **`relay_url`** — `BuzzClient::relay_url()` verbatim, not a second read of
  `BUZZ_RELAY_URL`. The two can disagree: a live run with
  `BUZZ_RELAY_URL=wss://hive.agiterra.org` printed
  `"relay_url":"https://hive.agiterra.org"` (scheme normalized). `whoami`
  always prints what the client will actually use.
- **`role`** — the role slug of the active team seat this signer holds, or
  `null` when it holds none. There is no channel or session ref in a seat's
  process env, so this is discovered from the identity alone: every channel
  the pubkey is a NIP-29 member of (kind:39002), every 44226 genesis in each,
  and that genesis's projected, receipt-backed seat roster
  (`crates/buzz-cli/src/commands/sessions/whoami.rs`). Two active seats with
  different role slugs is never resolved by picking one — the command exits 4
  and names every slug found.

`--format compact` and `--format json` print identically: the whole output is
already the minimal four-key shape `compact` reduces other reads to.

**Unseated key.** hive enforces `relay_membership_required`; a fresh
`BUZZ_PRIVATE_KEY` with no relay membership is refused before `whoami`'s own
logic runs:

```bash
BUZZ_PRIVATE_KEY=$(openssl rand -hex 32) bee sessions whoami
# → exit 3
# {"error":"auth_error","message":"relay error 403: relay_membership_required — …","retryable":false}
```

That refusal is the relay's membership gate, not this command's `role`
resolution — the "no active seat" and "conflicting active seats" paths are
covered by this module's unit tests instead
(`crates/buzz-cli/src/commands/sessions/whoami.rs`, `mod tests`).

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
| 70 | `sessions hire` (44221 `session.hire`) | ☐ | Historical founder/operator run is recorded above. Re-run current receipt-backed authority: founder + operator any role; active lead non-lead only; revoked/stale/wrong-genesis refused; created includes an accepted exact-role `grant-seat`; `created_ungranted` is live and must not be rehired. Open: `failed`/`seating` and old-relay wording. |
| 69 | `sessions status` / `list` founder | ☐ | `founder`/`createSigner` per row, `founders` array on `--format json` status (an envelope key — not in bare piped NDJSON); `null` when the channel holds no joined create; never the provider's key |
| 71 | `sessions assign/report/verdict/acknowledge/complete/block` | ☐ | Body accepts inline JSON, `@path`, or stdin; malformed/wrong-operation body is refused before write; `complete` refuses without an acknowledged approving disposition |
| 72 | `sessions operation get/list` | ☐ | `get --id` verifies the exact signed 44244 and derives `h`/`d`/genesis scope from it; explicit scope remains all-three-or-none; signed provenance, exclusions, conflicts, settlement and canonical terminal disclosed |
| 73 | team-operation provider wake | ☐ | 44244 is stored first; 44220 text contains only `operationId` and `type`; every installed seat pack tells the recipient to run `operation get --id`; an **assignment** shares its `deliveryCommandId` with its wake, every other class derives `cli-wake-v1:<operationId>:<12 hex>` and records none; failed wake leaves stored operation visible and delivery unconfirmed |
| 74 | `sessions audit` | ☐ | Per-turn rows carry the frozen shape; an unreported number is `null`, never `0`; `costUsd` is the producer's own number off the `result` item; `handedTwice`/`roomDownloads`/`retryLoops` populate on a night with waste; a clipped execution's rows carry `toolCallsTruncated: true`; `--format compact` prints the turn rows **and** the `bounds` rows |
| 75 | `sessions hire --check` | ☐ | Publishes nothing (`bee sessions list` shows no new seat); prints `published:false`, `briefBytes`, `briefCapBytes`, role and routing; exit 1 on an empty or oversized brief; refused together with `--no-wait` |
| 76 | `sessions grant-seat` / `revoke-seat` | ☐ | Seat granted by founder / steering operator / lead; a lead cannot grant `lead`; second run is `already_granted` with no write; `revoke-seat` refuses a pubkey with no seat and one holding a different role, and the roster loses the seat after it; `grant --role <slug>` is a parse error naming the two tiers |
| 77 | `sessions policy set/get/clear` | ☐ | `set` refuses a signer who is neither the founder nor the holder of an accepted operator grant, **before signing**; a sub-object nobody set is omitted, never `{}`; a closed-vocabulary miss carries serde's own sentence, listing the four legal words (it does not name the field — see the ledger residual); `set` with no policy flag names `policy clear`; `get` prints `null` (not `{}`) when nobody set one and lists every record it refused with author, time, code and reason; a stranger's later record never wins; all three print the enforcement disclosure |
| 78 | pre-publish fold check on every 44244 verb | ☐ | A causal reference that is absent, excluded or present-but-not-included is refused before signing, naming the id and the rule; a `--supersedes` that changes the subject, the author or the type is refused; a record that points at nothing makes zero relay reads; `complete` adopts your own canonical `mission.blocked` and the answer carries `supersedes` (present and `null` when it corrected nothing) plus a `correctedTerminal` sentence |

---

## Signed team transactions (kind 44244)

Operation bodies are the exact NIP-CSTX `body` object, not free-form prose:

```bash
bee sessions assign --channel "$CHANNEL" --session-ref "$SESSION" \
  --genesis "$GENESIS" --body @assignment.json --wake-to builder

bee sessions operation list --channel "$CHANNEL" --session-ref "$SESSION" \
  --genesis "$GENESIS"

# The exact command a seat runs after receiving
# {"operationId":"<id>","type":"assignment"} in a 44220 turn:
bee sessions operation get --id "$OPERATION_ID" | jq .
# Scope comes only from the verified signed 44244. Supplying scope manually is
# still allowed, but it is all-three-or-none:
bee sessions operation get --id "$OPERATION_ID" --channel "$CHANNEL" \
  --session-ref "$SESSION" --genesis "$GENESIS" | jq .
```

Use `--wake-to` only when a provider execution should be notified. The CLI
stores the signed 44244 first, then sends a kind 44220 pointer. The pointer is
not the assignment: recipients run `bee sessions operation get --id <id>` and
execute it only when `operations[0].canonical` is true; exclusions/conflicts are
reported, not executed. The wake's unsigned `type` hint grants nothing. The CLI
first queries the exact kind-44244 id, verifies id parity,
signature, strict envelope and content, then uses its signed `h`, `d`, and
`cstx-genesis` scope for the full fold. Managed seats need no unsigned scope
environment.

**Wake command ids.** An *assignment* is the only class whose stored record can
name its own delivery before it is signed, and the provider reads exactly that
pairing back as the binding that makes a finished turn owe a report — so an
assignment with `--wake-to` shares one id between the 44244 and the 44220, and
`--delivery-command-id` may preselect it. Every other class (report, verdict,
acknowledgement, mission.completed, mission.blocked) **derives** its wake's
command id from the operation that was just stored and the exact target:
`cli-wake-v1:<operationId>:<first 12 hex of sha256(target key)>`. That is why
those records carry no `deliveryCommandId` and why passing
`--delivery-command-id` alongside `--wake-to` on them is refused: the only id a
caller could hand a report is the one that already delivered its assignment,
already consumed on that target, so the lead runner fences the wake as
`AlreadyConsumed` and the lead is never woken. That is exactly what happened on
2026-09-01 (ledger item 103, finding 4). The namespace is distinct from
Desktop's `team-wake-v1:` and from the provider's own derivation so a reader of
a 44220 can tell which producer minted it; `bee sessions <verb>`'s JSON reports
`delivery.commandIdSource` as `derived` or `shared`.

The CLI verifies the genesis founder, every transaction signature, the relay's
NIP-11 `self` identity, and every kind-40099 receipt backing the contiguous
accepted kind-44228 authority chain. Receipt and transition id/genesis/seq/type/
grantee/role facts must agree exactly. Active operator grants and
`grant-seat`/`revoke-seat` role seats populate the fold context; raw
unreceipted transitions and lifecycle kind 44221/44223 data never substitute
for authority. A missing accepted seat grant remains visibly unauthorized.
After a host returns a signed create, receipt, and matching metadata, the hiring
CLI appends the exact actor/role `grant-seat` and reports `granted: true` only
after relay acceptance is proven. `created_ungranted` preserves all seat
evidence and means the live seat must not be hired again. Legacy
`sessions grant --role collaborator|viewer` does not repair a missing role-seat
transition; `sessions grant-seat` writes one.

---

## Session policy (kind 44245)

A session policy is a **stated intention, not an enforced limit**. Exactly one
field binds anything today — `budget.turns`, at the provider's turn gate — and
every surface says so in the same sentence.

```bash
bee sessions policy set --channel "$CHANNEL" --session-ref "$SESSION" \
  --genesis "$GENESIS" --posture overnight --budget-turns 12 \
  --required-gate "just ci" --irreversible push

bee sessions policy get --channel "$CHANNEL" --session-ref "$SESSION" \
  --genesis "$GENESIS" | jq .

# Withdraw it. `set` with no policy flag is refused and names this command.
bee sessions policy clear --channel "$CHANNEL" --session-ref "$SESSION" \
  --genesis "$GENESIS"
```

**Who may set one, and when it is judged.** The founder, or a seat holding an
operator grant that was accepted *by the moment the record was published*. That
is one rule, in `crates/buzz-core/src/coding_session_policy_fold.rs`, called by
both `bee` and the provider — a later revoke does not retroactively invalidate
a policy signed while the grant stood, and a policy signed before the grant was
accepted is refused. `set` and `clear` pre-check it before signing; `get` folds
it, so a stranger's record published later into the same channel prints as an
`excluded` row with its author, time, code and reason rather than as "the
newest policy".

`get` prints `null` when nobody with standing set one — never `{}` — and a
withdrawal prints as a real record with `setsAnyPolicy: false`, because
"nobody set a policy" and "someone withdrew theirs" are different facts.

The closed vocabularies are parsed by the record's own serde, so
`--posture sprint` is refused by a sentence that names `posture` and lists the
four legal words. A sub-object nobody set is omitted rather than published as
`{}`, which the record refuses.

**What the provider does with it.** `budget.turns` overrides
`BUZZ_CSP_TURN_BUDGET` for that umbrella, through one predicate serving both
the 44220 turn gate and a create's first turn; the refusal names the published
policy rather than the environment variable. The founder is never refused. A
policy published mid-session does not bind until that umbrella's next create or
resume. Everything else in the record is read and shown, never counted.

---

## Who a decision woke, and who it did not

Every `bee sessions decide` answer now carries a `delivery` object, present
whether or not a wake was published. Three of its shapes mean *nobody was
woken*, and only one of them is a problem:

| `delivery.status` | When | What it means |
|---|---|---|
| the wake's own status | `--wake-to`, or a `heldOn` actor holding an active seat | a wake was published; read `delivery` as before |
| `founder-held` | `decide request --held-on founder` | the founder is a person, not an execution. Nothing to wake; the Mission rail's waiting state is how they find out |
| `no-seat` | `heldOn` names a pubkey with no active seat (and, on `decide answer`, an asker with none) | **the problem case.** The party the mission is waiting on will not hear about it. `delivery.heldOn` carries the whole pubkey; grant it a seat or answer the ruling yourself |
| `not-requested` | `bee sessions note`, or a write that named no `--wake-to` | no wake was asked for |

```bash
bee sessions decide request --channel "$CHANNEL" --session-ref "$SESSION" \
  --genesis "$GENESIS" --question 'Ship it?' --held-on founder \
  | jq '.delivery'
# → {"published":false,"status":"founder-held","heldOn":null,"message":"…nobody was woken…"}
```

Before this, all three produced an **absent** `delivery` key, so a ruling nobody
would ever hear about was byte-identical to one deliberately held on a person.

---

## Observations (kind 44246)

An observation is **something its author saw, not a decision**. It settles
nothing, authorizes nothing and excludes nothing; a mission's state is decided
entirely by kind 44244. Any active seat or the founder may observe, and the
relay validates structure only.

> **Residual — read before you run these.** Registering kind 44246 at ingest is
> a separate change. Until it lands the relay stores no 44246, so every `observe`
> below is refused by the relay and `observations` reads an empty fold. The
> refusal is the relay's, not the CLI's: `bee` builds and signs a valid event
> first. Everything in this section is exercised by unit tests
> (`cargo test -p buzz-core coding_session_observation`,
> `cargo test -p buzz-cli --lib observation`) and none of it has been written
> live.

```bash
# Where you are in your own loop.
bee sessions observe checkpoint --channel "$CHANNEL" --session-ref "$SESSION" \
  --phase red --tests-written 4 --tests-red 4 --tests-green 0 \
  --last-command "cargo test -p buzz-core coding_session_observation" \
  --last-summary "4 failed"

# What your gates said. One --gate per row, NAME:OUTCOME:COMMAND, up to 32,
# unique by name. --summary and --duration-ms pair positionally with the rows.
bee sessions observe gate --channel "$CHANNEL" --session-ref "$SESSION" \
  --gate "cargo fmt --check:passed:cargo fmt --check" \
  --gate "cargo clippy:failed:cargo clippy --all-targets -- -D warnings" \
  --summary "" --summary "3 warnings emitted" \
  --duration-ms 1200 --duration-ms 41000

# A declared row may name the commit it ran at. It pairs positionally with the
# rows like --summary does, and the cleanliness word is mandatory: SHA:clean or
# SHA:dirty. A declared row NEVER admits a push, however exactly it names the
# commit — only a row the provider watched does (NIP-GS arm B).
bee sessions observe gate --channel "$CHANNEL" --session-ref "$SESSION" \
  --gate "just ci:passed:just ci" \
  --head-sha "$(git rev-parse HEAD):clean"

# What you found, and what you did about it.
bee sessions observe finding --channel "$CHANNEL" --session-ref "$SESSION" \
  --finding-id 16 --title "the waiting state does not fire" \
  --disposition fixed --ref "$ASSIGNMENT"

# How long a phase took. Every number here is YOUR OWN measurement.
bee sessions observe phase --channel "$CHANNEL" --session-ref "$SESSION" \
  --phase red --started-at-ms 1756800000000 --ended-at-ms 1756800413000 \
  --duration-ms 413000

# The bounded fold, and one line per fact.
bee sessions observations --channel "$CHANNEL" --session-ref "$SESSION" | jq .
bee --format compact sessions observations --channel "$CHANNEL" \
  --session-ref "$SESSION"
```

What to check, and what each answer means:

| Run | Expect |
|---|---|
| `observe gate` with no `--gate` | usage refusal naming `--gate NAME:OUTCOME:COMMAND`; nothing signed |
| `observe gate --gate "just ci:green:just ci"` | refusal listing `passed, failed, not-run` |
| `observe gate` with the same gate name twice | the relay-side validator refuses it: one observation states each gate once |
| `observe checkpoint --phase review` | refusal listing the five loop phases |
| `observe finding --disposition wontfix` | refusal listing the five dispositions (the token is `wont-fix`) |
| `observe finding --assignment <id nobody published>` | **accepted.** The pointer is disclosed under `unresolved`, and excludes nothing |
| two `observe finding` runs with one `--finding-id` | `observations` shows the **later** disposition and lists **both** event ids, `droppedEventIds: 0` |
| two `observe gate` runs naming one gate | same: newest statement, both ids, older event still on the wire |
| a gate republished more than 16 times | the newest 16 ids are listed, `droppedEventIds` counts the rest, and `truncated.entryEventIds` sums them |
| an event whose `pubkey` was rewritten after signing | listed under `ignored` with an `invalid observation signature` reason; **never** attributed to the rewritten key |
| `observations` on a session with none | every collection prints as `[]`, never `null`, and every `truncated` count prints as `0` |
| `--format compact` on a phase row | the line ends `(author's own measurement)` |
| `observe gate --head-sha "$(git rev-parse HEAD)"` | usage refusal: the value must be `SHA:clean` or `SHA:dirty` — a commit named without saying whether the tree matched it is not evidence about that commit |
| `observe gate --head-sha "deadbeef:clean"` | usage refusal naming a lowercase 40- or 64-hex git object id |
| `observe gate --head-sha "<sha>:dirty"` then `observations` | the row carries `headSha` and `dirty: true`, and its `source` is still `declared` |
| `bee git check --ref refs/heads/main` from a seat, with the provider's rows green on `HEAD` | `admitted by arm (B)` naming the mission and the gate list; `--format compact` `prediction.arm` is `observed-gates` |
| the same, after committing one more change | `refused` — `No observed gate row names <new sha>`. The rows are about the old commit and say so |

Two rules worth checking by hand, because they are the ones a reader can be
lied to about:

- **Nothing here is ordered by a time anyone claimed.** `startedAtMs`,
  `endedAtMs` and `durationMs` are the author's own measurement. "Newest" is
  last in the order the relay handed the events over.
- **A malformed 44246 costs only itself.** Publish something unreadable
  alongside a good observation and `observations` still prints the good one,
  with the bad one listed under `ignored` with its reason. This is the whole
  reason these four facts are not kind 44244 subtypes: on that kind, one bad
  envelope reads a whole session as a broken mission.
- **The signature is the author, and it is checked.** The fold verifies every
  event before naming anybody its author, so `author` on a row — and the
  `(author, gate)` / `(author, findingId)` keys that decide which statement is
  newest — is a key that actually signed. It carries **no seat model**: a
  stranger's observation folds like a seat's, listed under its own pubkey, so
  whoever renders one must say whose it is.

---

## Per-turn accounting (`bee sessions audit`)

```bash
bee sessions audit --channel "$CHANNEL" | jq .
bee sessions audit --channel "$CHANNEL" --session-ref "$SESSION" | jq .
bee --format compact sessions audit --channel "$CHANNEL"     # one JSON row per turn
```

Everything it prints comes off kind 44225 and nothing else: the terminal
`result` item's `usage` block and `durationMs`, and the `tool_call` /
`tool_result` pairs around them. Three rules make the numbers worth reading,
and each is worth checking live:

1. **Absent is `null`, never `0`.** A driver that reported no `outputTokens`
   and a driver that measured zero are different facts. Confirm on a turn whose
   adapter published no `usage` block: every token field is `null` and
   `totals.session.inputTokens` is `null` too, not `0`.
2. **`costUsd` is the producer's, or nothing.** The number comes off the
   `result` item's own `costUsd` (`buzz_core::coding_session_payload::result_item`)
   and is never computed here against a price list this binary happens to
   carry. A turn whose producer published no cost reports `null`.
3. **The bound is disclosed, in both output formats.** At most 4,096 items per
   execution; `bounds[].itemsTruncated` says when it stopped and
   `itemsPublished` says how much there was, and `--format compact` prints
   those rows after the turn rows (they carry `itemsBound`, which no turn row
   has).

`toolCalls` prefers the driver's own `usage.toolCalls` and otherwise counts the
`tool_call` items that turn published — both are measurements, neither is a
guess. A count taken from an execution the fold stopped reading is a floor, and
the row says so with `toolCallsTruncated: true` (the totals repeat the flag);
truncation removes exactly the terminal `result` item that would have carried
the driver's own count, so this is the common case on a long night.
`contextWindow` prefers `usage.contextWindow` and otherwise takes the driver's
own `context_window_updated` occupancy item.

Two conventions differ on purpose: the item's **top-level** `inputTokens` /
`outputTokens` (from `TurnCost`) are cache-inclusive, while the `usage` block's
are disjoint from the two cache counts
(`crates/buzz-core/src/coding_session_payload.rs`, `TurnUsageReport`). This
table reads the `usage` block, so a driver that reported only the top-level
pair shows `null` token columns beside a real `costUsd`.

The three waste tables:

- `handedTwice` — the same file path or the same command handed to one seat
  twice or more. `bytes` is what was **published**, and only for the calls that
  were answered: `resultsSeen` says how many of `count` those were, `bytes` is
  `null` when none of them were, and the provider clips a tool result at 8 KiB,
  so `bytesClipped: true` means the real figure is larger.
- `roomDownloads` — `bee sessions status|inbox|send|operation` runs, per seat.
  These pull the room into a seat's own context; `sessions audit` itself is not
  one of them, and neither is a line that merely *mentions* the CLI — the
  executable is recognized in command position only (`echo bee sessions status`
  is not a read).
- `retryLoops` — three or more *consecutive* identical commands that returned
  identical results. Interleaved repeats are work, not a loop, and three
  identical commands with three different results are work too. `count` is the
  longest single run, not the total across runs, and `identicalResults` is
  `null` — not `true` — when the transcript carries no result for the run.

The row shape is frozen with the Desktop Mission Audit tab, so a disagreement
between this table and that screen is a bug in one of them, not a matter of
taste.

---

## Acceptance tests must not publish (`bee sessions hire --check`)

```bash
bee sessions hire --channel "$CHANNEL" --session-ref "$SESSION" \
  --role builder --content "Rebase the lane and run the gate." --check
```

`--check` validates the brief and routing against the same SDK builder a real
hire signs, prints the facts the hire would carry, and **publishes nothing**:

```json
{"check":true,"published":false,"role":"builder","briefBytes":33,
 "briefCapBytes":12272,"briefWithinCap":true,"routed":false,...}
```

Exit 0 when the payload is one the relay would accept, 1 when it is not (an
empty brief, an oversized brief, a bad role slug). A refused check still prints
its facts first — an oversized brief prints `briefWithinCap: false` beside
`briefBytes` and then the refusal — so the numbers are readable on exactly the
run that needs them. It conflicts with
`--no-wait`: waiting for a host that will never be asked is a contradiction.

Use it for every acceptance test of the hire path. On 2026-09-01 a seat's
acceptance test published a live kind:44221 and seated a real agent (ledger item
103, finding 10); verify by running `bee sessions list --channel "$CHANNEL"`
before and after and seeing no new row.

---

## Role seats (`bee sessions grant-seat` / `bee sessions revoke-seat`)

```bash
# The operator tiers, unchanged — and the set is closed, so a typo is a parse
# error listing `collaborator` and `viewer`, never an accepted seat:
bee sessions grant --channel "$CHANNEL" --genesis "$GENESIS" \
  --pubkey "$PUBKEY" --role collaborator

# A role seat — who an actor IS inside one umbrella — has its own verb:
bee sessions grant-seat --channel "$CHANNEL" --genesis "$GENESIS" \
  --pubkey "$ACTOR" --role builder

bee sessions revoke-seat --channel "$CHANNEL" --genesis "$GENESIS" \
  --pubkey "$ACTOR" --role builder
```

`--session-ref` is optional on both: the signed genesis names its own umbrella
and is signature-verified before it is read.

A role seat is the fact the typed team fold reads for verifier standing, and
until these verbs existed only the hire path could write one — which is why
`seat-repair`'s `ambiguous` remedy used to point at the founder's Desktop app.
Writing seat authority is opted into: `grant` keeps a closed two-tier value set
(`collaborator`, `viewer`), so a mistyped tier is refused by the parser instead
of landing an accepted seat for a role nobody meant — which would then refuse
every legitimate grant for that actor until someone ran `revoke-seat`.
Standing is exactly the hire path's: founder, active steering operator, or
active lead; a lead may not grant `lead`; an actor may not nominate itself; an
actor already seated in a **different** role is refused, never silently
re-roled. Granting the same role twice reports `already_granted` and writes
nothing.

`revoke-seat` is refused (exit 1) when the pubkey holds no seat, and when it
holds a different role — the message names the role it actually holds. The
relay's transition matrix remains the gate; this refuses first so an operator
gets a sentence instead of a shape error. Acceptance is re-read from the
receipt-backed projection before it is reported: a submitted transition nothing
accepted exits 5 `unconfirmed`.

To converge an `ambiguous` `seat-repair`: revoke the role the actor holds, then
grant the one you mean with `grant-seat`, then re-run the repair and see
`already_granted`.

---

## Terminal git access (`bee git setup` / `status` / `check`)

`bee git check` asks the **git transport** — the same
`GET <repo>/info/refs?service=git-upload-pack` `git clone` makes, signed by
`git-credential-nostr`'s own key resolution, attestation reader and signing
function. Its exit code is that transport's answer: **0 accepted, 3 denied.**

```bash
bee git status                    # local config only; never says "ready"
bee git check                     # clone/fetch authorization
bee git check --push              # also probes git-receive-pack
bee --format compact git check    # git_transport / relay_http_membership / remedy
```

What to check, on a seat (`NOSTR_PRIVATE_KEY` + `BUZZ_AUTH_TAG` set by the ACP
harness) and on the operator's own shell:

1. The `key` line names the key **git** will present and where it came from. In
   a seat's shell that is `NOSTR_PRIVATE_KEY`, not the operator's key file; when
   both hold different identities, the `note` line says so in one sentence.
2. The `owner` line reports the attestation as present / absent / invalid. When
   it is present the owner hex is the operator, not the seat.
3. The `git` line is the verdict. Cross-check it against reality in the same
   shell — this is the check the command exists to be honest about:

   ```bash
   bee git check --push; echo "check exit $?"
   git push origin HEAD:refs/heads/proof/seat-git-push; echo "push exit $?"
   ```

   **The two exit codes must agree.** They disagreed on 2026-08-29: the check
   exited 3 over `relay_membership_required` while the push from the same key
   succeeded seconds later.
4. `relay HTTP membership:` is a **secondary** line for a different gate. It may
   refuse while git accepts. It must never change the exit code, and no output
   anywhere may advise unsetting `BUZZ_AUTH_TAG` — dropping the attestation
   removes the owner grant a seat's push rides on.

Not runnable from a seat without a relay-known key; say so rather than
reporting a guess.

### `bee git check --ref` names which arm would admit (batch 3, L21)

The prediction now says *which* of the require-verdict rule's two arms it read,
because "admitted" over *"you are a founder"* and over *"a verifier cleared
this commit"* are different facts and a person acting on the answer needs to
know which one they have.

```bash
bee git check --ref refs/heads/main
bee --format compact git check --ref refs/heads/main   # `prediction.arm`
```

Live checks, on a repository whose `refs/heads/main` carries `require-verdict`:

1. **As a founder.** The human line reads `admitted by arm (A) — you are a
   founder of this repository, and a founder's push needs no verdict`, and
   `--format compact` carries `prediction.arm: "founder"`. It must say this
   **even when the repository is bound to no channel and no mission is
   readable** — arm (A) reads no mission, and the relay short-circuits before
   its three queries. A prediction that turned "unreadable" into a refusal for
   a founder would be refusing over a fact the rule does not consult.
2. **As a seat, with a verifier's clearance on the wire.** `prediction.arm` is
   `"verifier-verdict"` and the sentence names both records: the lead's
   disposition and the verifier's refutation, each by short id.
3. **As a seat, with only an approval.** Refused, and the sentence is
   `… and no active verifier seat has cleared the report it approves. The gate
   wants a `refutation` verdict of `not-refuted` on that report …`. Publish one
   with `bee sessions verdict refutation --decision not-refuted` from a seat
   holding the `verifier` role and re-run: the same command must flip to
   admitted with no other change.
4. **As the builder, holding the verifier seat too.** Still refused, naming the
   key: a seat cannot stand as the verifier of its own work. This is a
   deliberate divergence from `bee sessions complete`, whose verifier check
   does accept that shape — a completion is a claim about work, a push is the
   work. Both sentences should be captured in one transcript when this is
   exercised, since the difference is the kind that reads as a bug until it is
   read as a rule.

The refusal an **arm (B)** would have answered does not exist: no gate-row
record on the wire names a commit, so `bee git check --ref` has nothing to
predict from. See NIP-GS's appendix, "The arm that is specified and not
implemented".

## A ruling that names a class, and a completion that needs a verifier (batch 3, L7)

**The relay must carry this core before any client writes a `condition`.** The
relay decodes the whole 44244 body at ingest, so an answer carrying `condition`
is *refused* by a relay predating this landing. The reverse is **not** true: a
body that **omits** `condition` decodes here exactly as it always did, because
the key is **required on write, optional on read** — a reader must never lose
history, and live run 2's three signed answers are the proof (NIP-CSTX, and
REPORT-L7 fix round 1). So: redeploy the relay first, then relaunch the app so
seats run the new bundled `bee`, before the next live run. Records already on
the wire need nothing.

`--condition` is text a person reads. Nothing evaluates it, and no test should
assert that anything does.

```bash
# The class a ruling covers, instead of one commit (live run 2, finding 21).
bee sessions decide answer --channel <uuid> --session-ref <uuid> \
  --genesis <hex64> --request <hex64> --choice-index 0 \
  --condition 'any SHA whose buzz-acp diff against origin/main is empty'

# Read it back: the signed body carries it verbatim under `condition`.
bee --format compact sessions operation get --id <answer-id>
```

Blank, whitespace-only and over-512-byte conditions are refused before signing,
naming the key. Omitting `--condition` writes JSON `null`, which is a different
answer from a missing key and is what every reader expects to see.

`gates.verifierRequired` is read from the umbrella's newest **accepted** 44245
by `bee sessions operation list|get` and by every 44244 write's pre-publish
check. To exercise it live:

```bash
# 1. Set the gate (founder, or a seat holding an operator grant).
bee sessions policy set --channel <uuid> --session-ref <uuid> \
  --genesis <hex64> --verifier-required true

# 2. Settle an assignment with no verifier ruling, then try to complete.
#    The CLI refuses before signing, quoting the fold's own reason and naming
#    the assignment and the report it settled on.
bee sessions complete --channel <uuid> --session-ref <uuid> \
  --genesis <hex64> --assignment <hex64> --summary 'Done'

# 3. Publish the verifier's ruling from the verifier seat, then complete.
bee sessions verdict refutation --channel <uuid> --session-ref <uuid> \
  --genesis <hex64> --assignment <hex64> --report <hex64> \
  --decision not-refuted --summary 'Reproduced the lane gate'
```

A `confirmed` or `blocked` refutation does **not** clear the completion: those
are rulings against it. A ruling signed by a seat that is not an active
`verifier` clears nothing. A report signed by an active `verifier` seat is
itself the ruling — live run 3's shape, where the verification was the
assignment.

**With no policy, or with the flag absent or false, every one of these commands
behaves exactly as it did before.** That is the case to check first when a fold
looks different from yesterday: read `bee sessions policy get` before
suspecting the 44244 fold.
## Mission rows and wip refs (lane L9)

`bee pulse missions` prints exactly the sentences Desktop renders. Nothing in
this section asks a seat to report anything: every row is produced by the hire
host's git hooks, the provider, or the relay.

```bash
export BUZZ_RELAY_URL=wss://hive.agiterra.org BUZZ_PRIVATE_KEY=$(cat ~/.nostr/key)
bee --format compact pulse missions --channel <uuid> --session-ref <uuid> --genesis <hex64> [--repo <repo-id>]
bee pulse prune-wip --repo <repo-id> [--merged <sha,sha>]
```

| Situation | Expected line |
|---|---|
| an unanswered `decision.request` held on the founder | `Waiting on the founder · asked by {Who} · {age} ago: {question}` — first line of the row |
| no readable request timestamp | the same line with the age clause omitted; **never** `0m` |
| no canonical terminal | `Mission running` |
| a completion the fold excluded | `A completion was excluded · {code} · {8hex} — this mission is not completed`, and the state stays `Mission running` |
| no verdict on the wire | `No verdict on the wire` |
| a seat with no 44246 gate row | `No gate row on the wire for {Who} — a claim in prose is not a gate row` |
| an observed row over a declared one | `{Who} · {gate}: {outcome} (observed, over a declared row) · {command}` |
| more than four gates | four rows plus `{n} more gates not shown` |
| a member with no wip ref | `{Who}'s local commits: not shared` — about the relay, not their config |
| a repo with no 30618 | `No ref state on the wire for this repo` |
| any wip ref shown | `Wip refs are pruned when their branch merges or after 30 days` |
| no policy record | `No policy set for this session` |
| a record setting nothing | `Policy withdrawn by {Who}` |
| no phase timing | `No phase timing on the wire for this session` |
| always | `Token cost is not on this surface: Pulse reads no usage events` |
| a 44244 fold error | one row reading `This session's records could not be read: {reason}`, the rest of the digest intact |
| `bee pulse prune-wip` on a ref outside `refs/heads/wip/` | listed under `refused`, never under `delete` |
| a wip ref whose 30618 carries no date | listed under `keep` — unknown is not old |

`bee pulse prune-wip` is **read-only**: it prints the plan and deletes nothing.
Deleting a ref is a push, and the push belongs to whoever holds the credential.

**Not written live.** Nothing in this section has been exercised against
`hive.agiterra.org` by the lane that wrote it: the reads above are safe to run,
and the two producers (the seat hook and the provider's observed rows) need a
live team run to confirm. Treat the table as the expectation, not as evidence.
## The registry bench (`bee sessions registry measure` / `propose`)

Every score in `team/model-registry.yaml` is an opinion — the file's own head
comment says so. Live run 3 (finding 25) is what that cost: the router
disclosed that an incumbent *"cleared the verifier gates (reasoning≥4.5,
judgment≥4.5, verification≥4.7) … nothing else cleared them"* while a Codex
target sat on the same bench uncompared, and **every number in that sentence
was a guess**.

**The rule: a row carries MEASURED scores or none. No lane invents a number.**

### What has and has not been exercised live

| | run? |
|---|---|
| the scorer, every check kind, on fixtures | **yes** — `cargo test -p buzz-core registry_bench` |
| the harness end to end on a **stub runtime** | **yes** — `cargo test -p buzz-cli registry_measure` |
| one **dry run through the real spawn path** against a local fixture | **yes** — see below |
| `measure` against a **real relay and a real model** | **NO. Not once.** |
| `propose` against a real relay | **NO** — its refusals are proven against hand-built events (a fake relay) |
| a `measured:` block in the shipped registry | **NO** — all eleven rows are still opinions |

Nothing below has produced a measured registry row on this machine. Read the
`measured:` block in `team/model-registry.yaml` as absent, because it is.

### The dry run (no relay, no model)

```bash
export BUZZ_CSP_RUNTIMES='[{"instanceRef":"claude-primary","driver":"claude",
  "runtime":"claude","agentCommand":"/path/to/stub-adapter.sh",
  "agentArgs":["--acp"],"allowedModels":["opus[1m]"]}]'
bee sessions registry measure --role verifier --runtime claude-primary \
  --model 'opus[1m]' --channel <uuid> --session-ref <uuid> \
  --repeat 3 --task-timeout 60 --dry-run
```

`--dry-run` resolves the runtime, spawns the descriptor's **own argv** with the
prompt on stdin and the scratch dir as cwd, runs the probes, scores, and
**publishes nothing**. The `command` in the report is the argv that ran — that
is the string a real run's gate row carries verbatim, and it is why a gate row
is evidence where prose is not (finding 26).

### The live run, when somebody does it

```bash
bee sessions registry measure --role verifier --runtime claude-primary \
  --model 'opus[1m]' --channel <uuid> --session-ref <umbrella-uuid> --repeat 3
bee sessions registry propose --role verifier --runtime claude-primary \
  --model 'opus[1m]' --channel <uuid> --session-ref <umbrella-uuid> \
  --founder <64-hex>          # add --write to apply
```

| expectation | why |
|---|---|
| `measure` publishes 3 gate rows, 3 checkpoints, and one finding per failed criterion | one signed row per task-run, before any score is written down |
| every gate row's `command` equals the spawn argv | a row whose command is a paraphrase proves nothing |
| `summary` reads `"<passed>/<total> · failed: <ids>"` | `propose` parses the failed set out of it, per run |
| `--repeat 2` is refused at parse time | fewer than three cannot produce a median |
| `--role lead` is refused: *"the bench for lead has no tasks"* | five roles ship as zero-task stubs |
| `propose` with a mid-run edit to `team/registry-bench/verifier/**` is refused | the `benchHash` moved under the measurement |
| `propose --role runner` may refuse on `velocity` | it is scored from `.bench/duration-ms`, and a wall clock is not stable |
| `registry check` prints `unmeasured` and **exits 0** | eleven opinions; refusing would stop the team |

### Reading the disclosure

`route`'s candidate table carries `"scores": "measured" | "legacy"` and, for an
offered target the registry has never decided about, `"state": "no-row"` with
*"offered by the catalog, no registry row: it was never considered"*. The
routing record's `reason` ends in one of exactly two clauses:

```
; scores measured by registry-bench/verifier v1 on 2026-09-02 (n=3, spread 4.4–4.8 on the binding trait reasoning)
; scores are operational priors, not measurements (rating: operational_opinion, confidence low, brian 2026-08-30) — this row is legacy
```

Once a class carries `benchAvailableSince`, the second becomes
`— legacy row · bench available · N days left`, and after thirty days `route`
refuses the row with the word `unmeasured`.
---

## `bee sessions worktree` — what a session's worktrees hold (L11)

Reads the desktop host's own record of the git worktrees it cut for seats
(`coding-session-workdirs.json`, `worktrees` map, schema v2). **A tree the host
never recorded is listed as `unrecorded` and removed by nothing here** — the
trees that predate the record are never adopted, whatever their branch is
named.

```bash
bee sessions worktree status --session <uuid>
bee --format compact sessions worktree status --all
```

Each row answers with one of eight dispositions — `prunable`, `held`,
`not-settled`, `tip-not-on-relay`, `execution-live`, `unrecorded`, `protected`,
`within-grace` — decided in `buzz_core::worktree_lifecycle`, plus the file
count, the measured reclaimable bytes, and one sentence.

Three things to check on a live machine, because each one has a way of being
quietly wrong:

1. **`dirtyFiles` excludes ignored paths.** In a worktree with a populated
   `target/` and `desktop/node_modules`, `status` must still report
   `dirtyFiles: 0` and `disposition: "prunable"` (or `within-grace`). If build
   output makes a finished tree read as `held`, the count is being taken from
   the wrong listing.

2. **Unknown is not false.** `tipOnRelayKnown: false` means this host could not
   establish the relay's ref state at all; the row's sentence must say "could
   not confirm", never that the branch was not pushed. Kind 30618 is
   parameterized-replaceable, so a `true` says where the ref stands **now** and
   is never a push history — `tipOnRelayLimit` carries that sentence in every
   row.

3. **`prune` refuses rather than skips.** On a session with one held tree:

   ```bash
   bee sessions worktree prune --session <uuid> --confirm; echo "exit $?"
   ```

   must exit non-zero and remove nothing, naming the held row. A run that
   exits 0 having silently passed over held work is the failure this command
   exists to prevent. Without `--confirm` it prints dispositions and removes
   nothing, whatever they say.

`reclaim` is the exception that runs on a held tree: `target/` and
`desktop/node_modules` hold no commits and are removable the moment the session
settles. After `bee sessions worktree reclaim --session <uuid> --confirm`,
every source file in the tree must be byte-identical and `git status
--porcelain` must report the same lines it did before.

For the lane worktrees nobody records, use `just worktrees-prune --dry-run`
first; it prints the full plan (protected / merged-and-clean / merged-but-dirty
/ unmerged) and removes nothing.
## `bee sessions explain` and the verb recipes (batch 3, lane L13)

The vocabulary is compiled into the binary, so this section needs no relay, no
key and no checkout. Run it against the build under test.

```bash
bee sessions explain unseated          # one entry
bee --format compact sessions explain waiting
bee sessions explain                   # every word once, as a JSON array
bee sessions explain unseted; echo "exit $?"
bee sessions explain postgres; echo "exit $?"
```

Expected:

1. `explain unseated` prints a JSON object with `word`, `aliases`, `meaning`,
   `cause`, `command`, `frozenSource` and `frozen`. `frozen` is a **string** for
   a word the batch specification froze a sentence for and JSON `null` for one
   it did not — never `""`, because "nobody froze one" and "one exists" are
   different answers.
2. `--format compact` prints exactly four keys: `word`, `meaning`, `cause`,
   `command`.
3. Bare `explain` prints a JSON array with every word once.
4. `explain unseted` exits **1** and the message reads
   `unknown word "unseted": did you mean "unseated"? …`.
5. `explain postgres` exits **1** with **no** "did you mean" — a wild miss is
   never answered with a confident guess.
6. Aliases resolve: `explain dangling_reference` and `explain DanglingReference`
   both print the `dangling` entry. Every fold exclusion code resolves by its
   snake_case wire spelling and by its Rust `Debug` spelling.
7. With `BUZZ_PRIVATE_KEY` and `BUZZ_RELAY_URL` unset, every command above still
   works. That is the point of the lane: a seat asking what a word means should
   not have to be authenticated or online to find out.

Every `bee sessions <verb> --help` ends with a `Recipe:` block holding one
runnable line, and four verbs (`report`, `observe`, `observations`,
`seat-repair`) additionally carry a `Rule:` block quoting a frozen sentence
byte-for-byte. Spot-check:

```bash
bee sessions seat-repair --help | tail -6
bee sessions report --help | tail -6
```

### The exclusion-code read contract CHANGED (batch 3, lane L13 / REVIEW-L13 F5)

`bee --format json sessions operation list|get` used to print
`.fold.excluded[].code` and `.operations[].exclusion.code` as the Rust `Debug`
spelling — `DanglingReference` — while the desktop adapter printed
`dangling_reference` for the same code. One code, two names, depending on which
surface a seat happened to read.

**Both CLI sites now print the snake_case wire spelling**, so the CLI, the
adapter, and `bee sessions explain` all say the same word:

```bash
bee --format json sessions operation list --channel <uuid> --session-ref <uuid> --genesis <hex64> \
  | jq -r '.fold.excluded[].code'
# was: DanglingReference     now: dangling_reference
```

The `bee sessions assign|report|verdict|…` pre-publish refusal that names an
excluded reference changed with them, and now points at the tool:

```
… the fold excluded it (dangling_reference: <reason>). Run `bee sessions explain
dangling_reference` for what that means. Cite the record that replaced it.
```

**If you have a `jq` filter, a script, or a pack example matching the old
CamelCase spelling, it needs updating.** `bee sessions explain` accepts **both**
spellings, so a seat that copied the old one still gets an answer.

### L13.4 — the acceptance, and why it cannot be measured yet

The intended acceptance is **observed, not declared**: on a live run, a lead's
**first turn** makes zero `Read`/`Grep`/`Glob` tool calls whose path argument is
under `crates/`. No agent is asked whether it read the source — the tool calls
are already signed records in kind 44225.

**This metric is not readable today, and an empty result does not mean it
passed.** Three facts about the current wire, each measured against
`wss://hive.agiterra.org` channel `c0066ddd-8214-4baf-81d2-3046fead0d32`:

1. **`.item.tool.input` is `{}` on every tool call the ACP adapter in use
   publishes.** 53 of 53 rows across two sessions carried `input` with zero
   keys. The serialiser can carry arguments — it reads `rawInput`, `input`,
   `arguments` or `args` (`buzz-session-provider/src/transcript.rs`, `tool_input`)
   — but the adapter sends none for read-class calls. **There is no path on the
   wire at all.**
2. **`toolName` is a display label, not a tool identifier**: `Terminal`,
   `Read File`, `Edit`, `Preparing file…`, `ToolSearch` — never `Read`, `Grep`
   or `Glob`.
3. **`bee sessions tools` has no per-turn breakdown**, so "a lead's *first
   turn*" is not a question that aggregate can answer either.

So an empty result means *"no paths on the wire"*, not *"the lead read no
source"*. **Unknown ≠ empty ≠ zero.** Do not report a clean run from silence
here.

**The observable proxy available today** — a count, with no path and no
per-turn split:

```bash
bee sessions tools --channel <uuid>          # e.g. {"calls":7,"toolName":"Read File"}
```

**The pipeline, with the field names the wire actually uses.** It is written
against the shape a 44225 tool call really carries, so it is runnable and will
start returning rows the moment (1) is fixed — today it correctly returns
nothing, which is why the paragraph above exists:

```bash
bee sessions transcript --channel <uuid> --session <session-id> --format jsonl \
  | jq -r 'select(.kind==44225) | .content' \
  | jq -r 'select(.item.kind=="tool_call")
           | [.item.tool.toolName, (.item.tool.input | tostring)] | @tsv' \
  | awk -F'\t' '$2 ~ /(^|\/)crates\//'
```

The wire shape it reads, for reference:

```json
{"schema":"buzz-coding-session-transcript/v1","eventSeq":24,"turnId":"9e032d7b-…",
 "item":{"kind":"tool_call",
         "tool":{"input":{},"toolId":"toolu_018WCz…","toolKind":"read","toolName":"Read File"}}}
```

**Cross-lane request — what closing L13.4 needs.** The provider must publish the
**path** (never the file's content) for read-class tool calls, either in the
44225 item's `tool.input` or as a kind 44246 observed checkpoint. That is the
`no-asking-agents-to-report` shape: the record is produced by the provider from
observed tool calls, under its own key, with no cooperation from the seat. Until
it exists, this lane's acceptance has **no honest measurement**, and inventing
one that runs today would be a control that lies about what it enforces.

## Project packs (`bee packs`, kind 30624 — lane L23)

Where a project's persona packs live, and what this machine would stage. Read
`docs/nips/NIP-PK.md` first; the rules below are that spec exercised.

**Publish (founder or project Owner only).** The relay's gate is closed by
default — a pack source decides which prompt bytes every seat on the project
runs.

```bash
bee packs set-source --project 30621:<owner-hex>:<slug> \
                     --repo    30617:<owner-hex>:<packs-repo-id> \
                     --ref     refs/heads/main
# or pin exactly:
bee packs set-source --project … --repo … --sha <40-hex> --path packs/roles \
                     --note "pinned for run 5"
```

Expect `{"event_id":…,"accepted":true,"project":"30621:…"}`.

Refusals to check, each at a different layer:

| Attempt | Layer | Expected |
|---|---|---|
| both `--ref` and `--sha` | clap | parse error, exit 1, before any key is loaded |
| neither | `PackSourcePin::resolve` | exit 1, "pass exactly one of --ref …" |
| `--project 30617:…` | `normalize_project` | exit 1, names `30621:<64-hex>:<slug>` |
| `--sha` of 39 hex, or `--ref main` | `build_project_pack_source` | exit 1, naming the field |
| `--path ../../etc` or `/etc/passwd` | `build_project_pack_source` | exit 1, "must be relative" |
| a key that is neither a founder of one of the project's repositories nor an Owner of it | relay | HTTP 403 → **exit 3**, sentence naming how many repositories were searched |

The last row is the one worth doing live: run it under a second identity and
read the sentence. It must say what was checked, not merely "restricted".

**Set a project up from nothing (`init`).** The same three steps the app's
*Create packs repository* performs. Needs the git credential helper
(`just install-git-credentials`).

```bash
bee packs init --project 30621:<owner-hex>:<slug> --dry-run   # plan only
bee packs init --project 30621:<owner-hex>:<slug>
bee packs init --project … --repo-id my-packs --from ./personas/roles --path packs/roles
```

Expect on success one JSON object naming every wire fact produced:
`repo`, `clone_url`, `announce_event_id`, `commit` (40 hex), `pushed_ref`,
`pack_source_event_id`, `roles`.

Order is the safety property, so check the failure paths:

| Attempt | Expected |
|---|---|
| a project that already has a pack source | exit 1 naming the existing repo and pin, nothing published — replacing one is a deliberate `set-source` |
| no `personas/roles` at or above cwd, no `--from` | exit 1 before anything is published |
| `--from` a directory with no role subdirectories | exit 1, "nothing to seed" |
| push fails (helper not installed) | the announce is reported on stderr with the note that **no** pack source was published; the project still stages shipped defaults |

The last row is the one to force deliberately (unset the helper): the invariant
is that a 30624 never points at a repository with no packs in it.

**Read.**

```bash
bee packs get-source --project 30621:<owner-hex>:<slug>
bee packs status     --project 30621:<owner-hex>:<slug> --role builder
bee packs status     --project … --role builder --packs-dir /tmp/packs-probe
```

`get-source` prints an array; an empty array is a real state (no pack source
published) and exit 0, not an error.

`status` separates the wire from the disk, and the separation is the point:

* `source`, `repo`, `pin`, `path`, `would_stage.path` come from the signed
  record;
* `cache_dir`, `cache_present`, `roles_found`, `would_stage.present_in_cache`
  describe **this machine**. `cache_present: false` means this machine has
  never fetched these packs — read it as *unknown*, never as "the repository
  has no such role".

With no pack source, `status` prints `source: null` and the sentence saying a
seat is staged from the session checkout's own `personas/roles/<role>/` and
carries no `packRef`. That absence is the correct answer; nothing should
invent a default pack.

**The word.**

```bash
bee sessions explain pack     # also: packs, packRef, packSource, 30624
```

**Shipped defaults.** With no 30624, `status` reports
`source_kind: "shipped defaults"` and the `fallback_order`
(`packs repository` → `session checkout` → `shipped defaults`). A seat staged
from the app's own bundled packs publishes
`packRef {"repo":"app:shipped","sha":"<app version>",…}` — not a coordinate,
because those packs are not a repository anyone can fetch. A blank version is
refused by the decoder.

**On a seat.** A staged seat's kind:44223 carries
`packRef {repo, sha, role, path}` — `sha` is the commit the host *resolved*,
even when the source pinned a ref, and `role` is the **seat's** role. A seat
metadata event signed before this key existed still decodes; absence reads as
*no pack staged*.
