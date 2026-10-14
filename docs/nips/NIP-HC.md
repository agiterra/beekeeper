# NIP-HC — Host commissioning

`draft` `optional` `client`

Commissioning is how a machine becomes an agent host for one relay without a
desktop: an **owner key** attests a freshly minted **provider key**, the
provider key proves it is admitted by the relay, and the host writes a
`host.json` it can act on later with no owner credential present. This page
describes `bee host commission` as a protocol — what is signed, what is sent,
what is stored where — so another implementation can commission a host, and
so a reader of the relay can tell a commissioned provider from a key someone
typed in.

The implementation is
[`crates/beekeeper-cli/src/commands/host_commission.rs`](../../crates/beekeeper-cli/src/commands/host_commission.rs)
with the minting and custody helpers in
[`crates/beekeeper-host-core/src/provision.rs`](../../crates/beekeeper-host-core/src/provision.rs).
The operator runbook is [`docs/headless-extensions.md`](../headless-extensions.md)
§ Install and commission; this page is the contract under it.

## Participants and keys

| key | held by | appears on the wire as |
| --- | --- | --- |
| owner | the person commissioning (`BEEKEEPER_PRIVATE_KEY`, env only) | the first element of the `auth` tag; never the signer of a provider event |
| provider | the host (minted at commission, restricted file) | `pubkey` of every 44222/44223/44224/44225/46022/46023/24223/24224 the host publishes |
| relay | the relay (`self` in NIP-11) | signer of 46013/46014/46015 and NIP-29 metadata |

The provider key is scoped to **one relay**: the provider store is keyed by
canonical relay URL, and commissioning the same home against a second relay
mints a second identity. Nothing ties one provider key to two communities.

## The attestation

The provider key carries a [NIP-OA](NIP-OA.md) `auth` tag minted by the owner
at commission time:

```json
["auth", "<owner pubkey hex>", "", "<64-byte Schnorr sig, hex>"]
```

- `conditions` is the **empty string**: no `kind=` and no `created_at` clause.
  The attestation is reusable on every event the provider signs; revocation is
  by refusing to re-attest and by relay-side membership, not by a window
  (NIP-OA § Security Properties).
- Preimage: `nostr:agent-auth:<provider pubkey hex>:` (the trailing colon
  precedes the empty conditions), hashed SHA-256 and Schnorr-signed by the
  owner (`beekeeper_sdk::nip_oa::compute_auth_tag`).
- The tag is stored verbatim as a JSON array string (`authTag` in the
  provider record) and handed to the provider process as
  `BEEKEEPER_AUTH_TAG`. Every event the provider publishes carries it. On a
  closed relay (`BEEKEEPER_REQUIRE_RELAY_MEMBERSHIP=true`) with
  `BEEKEEPER_ALLOW_NIP_OA_AUTH=true`, a key bearing a valid tag authenticates
  by proving its owner is a relay member and gets session-scoped access; on
  an open relay the tag is read for provenance only and grants nothing
  (`crates/beekeeper-relay/src/config.rs` `allow_nip_oa_auth`).
  `write_commissioning` verifies the tag against the provider pubkey before
  writing anything.

A host records at most one attestation per relay. Re-running commission with
a **different** owner against an existing provider record is refused
(`existing provider belongs to a different owner; identity preserved`) — the
identity is never silently re-homed.

## The steps

1. **Resolve prerequisites locally, before any network.** `--home` must be
   absolute; `--adapter` (and `--provider-command`, if given) must resolve to
   an executable file; a `--credential-file` must be a regular file with mode
   `0600`; each `--runtime-install-root` must be a directory that is not the
   OS home, not the service home, and not inside or around the host's custody
   directory. Any failure here is a usage error and nothing is written.
2. **Mint or reuse the provider identity.** Load
   `<home>/<instance>/host/service/session-provider/coding-session-provider.json`
   (`STORE_FILE_NAME`). If a record for this relay exists, reuse it — a lost
   reply or a retry keeps the same key and `instanceId`. Otherwise
   `mint_provider_record`: generate a key, derive `instanceId` from the pubkey
   prefix, compute the attestation. **Persist the store before contacting the
   relay**, so a lost reply never strands a minted key.
3. **Write custody under one management lock**
   (`persist_commissioned_custody`): the scoped provider key file
   (`host_key_file_path`), `host.json` (`host_config_path`), and the provider
   store, all `0600`, all atomic writes. The lock is released before the relay
   is contacted. The owner's secret is never written: the store, the config
   and the key file are tested to contain neither its hex nor its bech32
   form.
4. **Prove admission.** Authenticate to the relay **as the provider key**,
   with the attestation tag, and run one read:

   ```json
   {"kinds":[44220],"authors":["<provider pubkey>"],"limit":1}
   ```

   The result is discarded; what matters is that the relay authenticated the
   provider key and answered. This proves relay admission and nothing more.
5. **Report.** One JSON line on stdout:

   ```json
   {"provider_pubkey":"…","instance_id":"…","commissioned":true,
    "relay_admitted":true,"runtime_prerequisite":"adapter_executable",
    "runtime_account":"unconfirmed","worker":"not_started",
    "home":"…","state_dir":"…"}
   ```

   `runtime_account: "unconfirmed"` and `worker: "not_started"` are disclosed
   non-answers, like NIP-11's `unknown`: commissioning does not start the
   host, does not call the model, and cannot say whether the account behind
   the adapter can perform a turn. A real turn is separate evidence.

Steps 1–3 are purely local; a commissioned host can therefore be prepared on a
machine that is offline and admitted later. Step 4 is the only network call
and it is idempotent.

## What is stored where

| file | contents | secret |
| --- | --- | --- |
| `<host>/host.json` | `HostConfig` v1: `relayUrl`, `providerPubkey`, the `RuntimeDescriptor` list (`instanceRef`, `driver`, `runtime`, `agentCommand`, `agentArgs`, `defaultModel`, `allowedModels`, …), limits, `providerCommand`, `writtenAt` | no |
| `<host>/<key file>` | the provider's private key | yes, `0600` |
| `<host>/service/session-provider/coding-session-provider.json` | the provider store: per-relay `{providerPubkey, instanceId, authTag, createdAt, relayUrl, privateKeyNsec}` plus machine-wide limits | yes (`privateKeyNsec`), `0600` |
| `<state>/runtime-credentials-source.json` | `{"path": <credential file>}` — a pointer, resolved again at every session start | no (the file it points at is) |
| `<state>/runtime-installations.json` | `{"runtimes": {"<runtime>-primary": [<roots>]}}` | no |

`<host>` is `layout::host_dir(home, instance)`; `<state>` is
`<host>/service/session-provider/<provider pubkey>/`. The `instance`
(production or a named dev instance) comes from the environment
(`layout::Instance::from_env`). Nothing commission writes names the owner.

**No secret travels.** The credential file's contents are never read by
`commission`; only its path and mode are checked. The attestation is public
data. The provider key leaves the machine only as signatures.

## The runtime descriptor

`--runtime` picks the descriptor written into `host.json`. In the code at the
time of writing it accepts `claude`, `codex` or `goose` and maps each to its
driver (`claude-agent-acp`, `codex-acp`, `goose-acp`); the pluggable slice
(spec Step 1, Lane A) extends this so that **any** name is accepted when
`--driver <command>` is given, with `--driver-arg` and a repeatable
`--credential-env VAR`, and adds `credentialEnv`/`credentialSource` to the
descriptor. See `docs/runtime-descriptor.md` once that lands; the fields in
`host.json` are the descriptor's and this page does not restate them.

## What a reader can verify from the relay

- A provider's events carry an `auth` tag whose owner is a known member and
  whose signature verifies against the event's `pubkey` (NIP-OA § Client
  Behavior). Display it as "authorized by `<owner>`", never as authorship.
- The provider's `kind:44222` catalog ([NIP-CSL](NIP-CSL.md)) names the
  `instanceRef`s `host.json` lists; `kind:24224` presence says the host is
  running.
- Nothing on the relay says *which machine* a provider is, what its home
  directory is, or what adapter it runs. Those are host-local by design.

## What commissioning is not

It is not service registration (`bee host install`), not starting the host
(`beekeeper-host run`), not model authentication, and not channel membership
for the provider — a provider that will answer host steps ([NIP-HS](NIP-HS.md))
must still be put in the channel, exactly as any member is.
