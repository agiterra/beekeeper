# Relay-only remote work fixture

Validation status (2026-11-05): the corrected fixture passed 20 checks with
no failures, including exact replay dispatch observation and full-channel
transcript/effect checks. Independent bounded delta review passed. The earlier
14/0 run predates the strengthened oracle; the subsequent 8/1 run exposed an
environment-override error in the harness. That error is fixed; both prior runs
remain historical evidence. This proves only the local scope described below.

Run from the repository with Hermit activated:

```sh
CARGO_BUILD_JOBS=2 cargo build --locked -p beekeeper-cli -p beekeeper-host \
  -p beekeeper-session-provider -p beekeeper-relay -p git-credential-nostr
python3 scripts/host-remote-work-check.py --evidence /tmp/remote-work-evidence
```

Requires Docker and the images used by `scripts/host-manage-check.py`, Python 3,
Node and the repository's desktop `nostr-tools` dependency. It starts isolated
Postgres/Redis/object-storage containers, a relay, and two foreground Hosts with
independently commissioned provider keys. It never registers a service or uses
an installed Host. No model or production credentials are needed. Temporary
homes contain fixture keys and remain local for debugging; do not publish them.

The coordinator is a client in A's isolated environment. Both Host providers
connect outbound to the relay. A addresses a standalone channel-owned execution
on B by provider signing key and runtime alias, then sends a turn to the exact
created target. This is explicit selection, not placement or session migration.
The destination administrator provisions the channel-to-directory map locally;
network addresses do not establish execution or project identity.

The existing scripted ACP adapter recognizes the fixture phrase and appends a
SHA-256 proof to `remote-work-proof.jsonl` in its execution directory. There is
no arbitrary command or path supplied by the sender. A verifies signed progress,
turn-start receipt and successful terminal transcript result. Restart/reconnect
replays both the exact signed event and a fresh envelope with the same command
ID. Fresh IDs must differ from the original and be accepted. For both fresh
IDs the fixture waits for B's `relay command dispatch completed` debug record
before comparing all destination-signed transcript IDs in the dedicated channel
and checking that the local effect count stays one. It checks the full channel
again after the unauthorized command, so a new turn or execution cannot hide
behind an original-turn filter. The started receipt must name the selected
execution, and the terminal item must explicitly report success/completed.

Source note: `Provider::handle_relay_event` in
`crates/beekeeper-session-provider/src/lib.rs` emits the debug event ID and
channel ID only after awaited 44220/44221 dispatch. The local unsigned log is
processing evidence, not a new signed disposition, task completion receipt or
exactly-once guarantee for external effects. No payload or authority is logged.
The bounded barrier fails if processing cannot be established. An ungranted channel member must be refused without another
effect. A generic relay rejection is insufficient: the fallback requires the
specific `UNAUTHORIZED_OPERATOR` code or fails as unestablished. With B down, a fresh create must remain unconfirmed during the stated
observation window; relay acceptance is not completion.

The fixture uses same-owner NIP-OA attestations and ordinary channel/session
authority. It grants no cross-owner resource access. Standalone channel scope is
supported here; project-role materialization, per-task read grants, container and
Kubernetes execution, takeover fencing and crash-during-effect semantics are not
claimed. Durable command dedupe does not guarantee exactly-once external effects.

This local POSIX fixture is protocol evidence only. Windows compatibility and
real cross-machine acceptance require separate runs. For the next two-machine
acceptance, use an isolated WSS relay, independently commissioned identities on
each machine, and locally authorized directory maps. Run the coordinator only on
A and the scripted destination only on B; block inbound peer access. Record
binary hashes, OS/architecture, signed event IDs, target generation and effect
hash; repeat reconnect, identical-command redelivery, missing-grant and offline
cases. Preserve B's state across restart. Do not use a live production Host for
that procedure.
