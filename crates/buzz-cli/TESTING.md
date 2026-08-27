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
| 63 | `sessions list` | ☐ | Empty channel → `[]`; compact keeps target/title/status/model/createdAt |
| 64 | `sessions transcript` | ☐ | `--target` and `--session`; md turns + tool outcomes; jsonl seq numerically ordered |
| 65 | `sessions tools` | ☐ | Call/error counts, error rate, `itemKinds` incl. `other`; `--target` narrows |
| 66 | `sessions export` | ☐ | Files + manifest.json; non-empty `--out` refused with exit 1 |
| 67 | turn-stage receipts (kind 44224, §6.13.1) | ☐ | NOT YET RUN LIVE — `sessions list`/`transcript` unaffected by a turn receipt on an otherwise-known or unknown target |
