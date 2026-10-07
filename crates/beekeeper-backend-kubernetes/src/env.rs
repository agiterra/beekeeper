//! Building the pod environment (spec §Launch data, §Entrypoint mapping table).
//!
//! The three tiers are resolved *here*, before serialization, because a
//! Kubernetes Secret's `data` is a flat map with no precedence of its own: if
//! two tiers supplied the same key, whichever entry landed in the map would
//! win silently. Resolving in-provider makes later-wins explicit and testable.

//!
//! # Two spellings, one pod ABI
//!
//! Pod images are digest-pinned, so a pod may run a sprig image built before
//! the `BUZZ_*` → `BEEKEEPER_*` rename: it reads only `BUZZ_*` env and has only
//! the `buzz-*` executables. Three rules keep both generations of image
//! working:
//!
//! * received maps (`launch.policy_env`, `launch.env`, `env_vars`) are
//!   normalized to the `BEEKEEPER_*` spelling first, so an older desktop's
//!   `BUZZ_*` keys mean the same thing and every guard below sees one spelling;
//! * the finished env carries **both** spellings of every name
//!   ([`with_legacy_mirrors`](beekeeper_core::env_compat::with_legacy_mirrors));
//! * the agent and MCP commands are sent under the `buzz-*` names every image
//!   has ([`pod_command_name`]).

use crate::wire::{AgentPayload, LaunchBlock};
use beekeeper_core::env_compat::{both_spellings, mirror_map, normalize_env_keys};
use std::collections::BTreeMap;

/// Keys the authoritative tier owns.
///
/// Load-bearing, not documentation: tier 3 *clears* every key on this list
/// before writing its own values, so a key the authoritative tier has no value
/// for is **removed** rather than left holding a lower-tier value. Plain
/// overwrite is not enough — most of these are written conditionally
/// (`BEEKEEPER_ACP_AGENT_ARGS` only when `launch.args` is non-empty,
/// `BEEKEEPER_ACP_RESPOND_TO` only when set), and without the clear, a lower tier
/// could supply the value for exactly the cases the authoritative tier stays
/// silent on. Clearing is also what the local spawn does: the desktop strips
/// reserved keys from user env before the authoritative layer is written
/// (`env_vars.rs:54-57`), so absent-means-absent in both paths.
const AUTHORITATIVE_KEYS: &[&str] = &[
    "BEEKEEPER_RELAY_URL",
    "BEEKEEPER_PRIVATE_KEY",
    "NOSTR_PRIVATE_KEY",
    "BEEKEEPER_AUTH_TAG",
    "BEEKEEPER_ACP_AGENT_OWNER",
    "BEEKEEPER_ACP_AGENT_COMMAND",
    "BEEKEEPER_ACP_AGENT_ARGS",
    "BEEKEEPER_ACP_RESPOND_TO",
    "BEEKEEPER_ACP_RESPOND_TO_ALLOWLIST",
    "BEEKEEPER_ACP_MCP_COMMAND",
    "BEEKEEPER_ACP_EXIT_AFTER_INACTIVITY",
    START_NONCE_KEY,
];

/// Command names as the pod image knows them: `(name the desktop sends, name
/// every image ships)`.
///
/// The desktop now names the bundled agent `beekeeper-agent` and the MCP server
/// `beekeeper-dev-mcp`, but a digest-pinned sprig image built before the rename
/// has only the `buzz-*` links, while every image — old or new — has those
/// (`Dockerfile.sprig` installs both sets). So the pod is always told the
/// `buzz-*` name.
///
/// Retire this table once every supported sprig image ships the `beekeeper-*`
/// names; until then sending a `beekeeper-*` name to a pod can launch nothing.
const POD_COMMAND_NAMES: &[(&str, &str)] = &[
    ("beekeeper-acp", "buzz-acp"),
    ("beekeeper-agent", "buzz-agent"),
    ("beekeeper-dev-mcp", "buzz-dev-mcp"),
];

/// The name to send a pod for `command`: the `buzz-*` spelling of a bundled
/// binary (see [`POD_COMMAND_NAMES`]), anything else unchanged. Only an exact
/// bare name is mapped; a path or another runtime (`goose`, `claude-agent-acp`)
/// passes through.
pub fn pod_command_name(command: &str) -> &str {
    POD_COMMAND_NAMES
        .iter()
        .find(|(current, _)| *current == command)
        .map_or(command, |(_, pod)| pod)
}

/// Write the attempt's generation under [`START_NONCE_KEY`] and, when the
/// env already carries legacy mirrors, under its `BUZZ_*` twin too — an old
/// image's harness reports the twin, and a stale one would correlate its logs
/// to a different Secret.
pub fn stamp_generation(env: &mut BTreeMap<String, String>, generation: &str) {
    for key in both_spellings(START_NONCE_KEY) {
        if key == START_NONCE_KEY || env.contains_key(&key) {
            env.insert(key, generation.to_string());
        }
    }
}

/// The attempt's generation, as the harness sees it. Also the Secret's name
/// suffix — one generation, one identity — so the reconciler restamps this on
/// every create attempt rather than letting the caller's value persist across
/// a retry.
pub const START_NONCE_KEY: &str = "BEEKEEPER_MANAGED_AGENT_START_NONCE";

/// Presence is the only remote liveness signal (I3), so a launch that
/// suppresses it is non-conforming (L1 item 2) — and unlike a reserved-key
/// collision, there is no "authoritative value" to overwrite it with. Refuse.
const FORBIDDEN_KEY: &str = "BEEKEEPER_ACP_NO_PRESENCE";

/// Kubernetes' own cap on the summed value bytes of a Secret
/// (`MaxSecretSize`, `pkg/apis/core/types.go`). Enforced here so an oversized
/// env surfaces as a named provider error rather than an apiserver rejection
/// partway through a deploy.
const MAX_SECRET_BYTES: usize = 1024 * 1024;

/// A POSIX-shaped env var name: `[A-Za-z_][A-Za-z0-9_]*`.
///
/// Kubernetes validates Secret *keys* as `IsConfigMapKey`
/// (`[-._a-zA-Z0-9]+`), which is looser — `foo.bar` is a legal Secret key.
/// What the kubelet then does with such a key **changed between versions**:
/// through 1.29 it filtered invalid env names out of `envFrom` and emitted an
/// `InvalidEnvironmentVariableNames` warning event
/// (`pkg/kubelet/kubelet_pods.go:646,654` at v1.29.0); from 1.30 that filter
/// is gone (KEP-4369) and the key is injected verbatim. The same manifest
/// would silently drop a variable on one cluster and set it on another, so we
/// fail closed on the provider side and get one deterministic behavior.
fn is_posix_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// An identity component (L1 item 1) is present only if it is nonempty after
/// trimming — and the **trimmed form is what gets stored**. The validator and
/// the writer must never disagree about the value: a guard that accepts
/// `"  wss://relay  "` and then writes it with the padding intact has only
/// moved the failure from a loud refusal to a connect error in the harness.
fn identity_component(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// The harness's `allowlist` gate mode, spelled as the desktop serializes
/// `RespondTo` (kebab-case) and as `beekeeper-acp`'s CLI parses it.
const RESPOND_TO_ALLOWLIST: &str = "allowlist";

/// Every gate mode `beekeeper-acp` accepts, spelled as its `clap::ValueEnum` parses
/// them (`config.rs:95-101`, kebab-case via `RespondTo`'s `Display`).
///
/// Deliberately the **harness's** four and not the desktop's three: the desktop
/// rejects `nobody` on purpose (`managed_agents/types.rs:871-880`), but the
/// harness starts fine with it. This guard exists to cover non-desktop callers,
/// so inheriting a desktop-only narrowing would refuse a launch that works.
const RESPOND_TO_MODES: [&str; 4] = ["owner-only", RESPOND_TO_ALLOWLIST, "anyone", "nobody"];

/// Refuse a respond-to gate the harness will reject at config parse.
///
/// The local spawn path re-validates this before spawning — "doing it here
/// means we never spawn a doomed process" (`runtime.rs:378`) — but the deploy
/// path projects the record's fields straight through. Without this, a gate
/// the harness refuses becomes a pod that exits 1 at startup; `restartPolicy:
/// Never` turns that into `Terminated` → `Delete` → recreate, and each cycle
/// leaves a Secret the in-call path never reaps (only a later deploy's orphan
/// sweep does, at `ORPHAN_SECRET_MIN_AGE_SECS`). The user-visible ending is
/// "startup not confirmed", indistinguishable from a slow cluster.
///
/// Mirrors `beekeeper-acp`'s own rules exactly (`config.rs:95-101,996-1004,629-641`),
/// deliberately including their asymmetry: the allowlist is validated **only**
/// in allowlist mode, and merely warned about otherwise. Validating it in
/// every mode would refuse a deploy whose identical local spawn succeeds —
/// and a stale list is already harmless here, since
/// `BEEKEEPER_ACP_RESPOND_TO_ALLOWLIST` is an authoritative key that tier 3 clears.
fn validate_respond_to_gate(respond_to: &str, allowlist: Option<&[String]>) -> Result<(), String> {
    // Exact, untrimmed: `clap` does not trim, so `" allowlist "` is `rc=2` at
    // the harness — a parse failure even earlier than the config errors below.
    if !RESPOND_TO_MODES.contains(&respond_to) {
        return Err(format!(
            "deploy refused: respond_to {respond_to:?} is not a mode the \
             harness accepts (expected one of {}) — the pod would fail to \
             parse its arguments, be replaced, and leave a Secret behind on \
             every attempt",
            RESPOND_TO_MODES.join(", ")
        ));
    }
    if respond_to != RESPOND_TO_ALLOWLIST {
        return Ok(());
    }
    let entries = allowlist.unwrap_or_default();
    if entries.is_empty() {
        return Err(format!(
            "deploy refused: respond_to is {RESPOND_TO_ALLOWLIST:?} but the \
             allowlist is empty — the harness refuses this at startup, so the \
             pod would fail, be replaced, and leave a Secret behind on every \
             attempt"
        ));
    }
    for entry in entries {
        let trimmed = entry.trim();
        if trimmed.len() != 64 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!(
                "deploy refused: invalid pubkey in respond_to_allowlist: \
                 {entry:?} (must be exactly 64 hex characters)"
            ));
        }
    }
    Ok(())
}

/// Inputs the provider itself supplies to the authoritative tier.
pub struct AuthoritativeInputs<'a> {
    /// The attempt's generation token — also the Secret's name suffix, so the
    /// lifecycle correlator and the Secret generation are one identity.
    pub generation: &'a str,
    /// Resolved from `provider_config.inactivity_seconds`; `None` when the
    /// indefinite opt-in was chosen (which this version refuses elsewhere).
    pub inactivity_seconds: Option<u64>,
}

/// Resolve the full pod environment.
///
/// Order is the spec's, and the function body is deliberately three writes in
/// that order — tier 1, tier 2, tier 3 — so "later wins" is visible rather
/// than argued.
pub fn build_env(
    agent: &AgentPayload,
    auth: AuthoritativeInputs<'_>,
) -> Result<BTreeMap<String, String>, String> {
    let default_launch = LaunchBlock::default();
    let launch = agent.launch.as_ref().unwrap_or(&default_launch);

    let mut env: BTreeMap<String, String> = BTreeMap::new();

    // Tier 1 — overridable behavior defaults. Each received map is
    // normalized to the current spelling on its own, so a tier-2 `BUZZ_X`
    // from an older desktop still overrides a tier-1 `BEEKEEPER_X`.
    env.extend(normalize_env_keys(launch.policy_env.clone()));

    // Tier 2 — user/layered env. The descriptor already merged
    // global < persona < agent, so `agent.env_vars` is NOT re-merged on top
    // (§Launch data tier 2) — doing so would resurrect a layer the desktop
    // already resolved. When the desktop predates the `launch` block we fall
    // back to the legacy field, which is the only case it is the truth.
    if agent.launch.is_some() {
        env.extend(normalize_env_keys(launch.env.clone()));
    } else {
        env.extend(normalize_env_keys(agent.env_vars.clone()));
    }

    // Validate what the lower tiers contributed, before the authoritative
    // tier overwrites any of it. A reserved-key collision is NOT fatal: the
    // spec's precedence is later-wins, so tier 3 simply overwrites it, which
    // is exactly what a local spawn does. Only a key that has no
    // authoritative counterpart to overwrite it — presence suppression — is
    // a refusal.
    for key in env.keys() {
        if !is_posix_env_key(key) {
            return Err(format!(
                "env key {key:?} is not a POSIX environment variable name \
                 ([A-Za-z_][A-Za-z0-9_]*); Kubernetes would treat it \
                 inconsistently across cluster versions"
            ));
        }
        // Both spellings, though normalization already folded an exact
        // `BUZZ_ACP_NO_PRESENCE` in: the comparison ignores case and the
        // normalizer does not, so `buzz_acp_no_presence` reaches here as-is.
        if both_spellings(FORBIDDEN_KEY)
            .iter()
            .any(|forbidden| key.eq_ignore_ascii_case(forbidden))
        {
            return Err(format!(
                "{FORBIDDEN_KEY} (or its legacy spelling) must not be set on a \
                 remote agent: presence is the only signal that a remote agent \
                 is alive"
            ));
        }
    }

    // Tier 3 — authoritative. Every key it owns is cleared first, then the
    // values it has are written, so it wins at a key whether or not it has a
    // value there (see [`AUTHORITATIVE_KEYS`]).
    // Both spellings: the legacy one is what an old image would read.
    for key in AUTHORITATIVE_KEYS
        .iter()
        .flat_map(|key| both_spellings(key))
    {
        env.remove(&key);
    }
    // Identity comes from top-level payload fields, never from `env_vars`
    // (§Reserved-key rule). All three components must be nonempty: an agent
    // that cannot reach a relay is the identityless launch L1 item 1 exists to
    // prevent, and a blank field would otherwise sail through into the Secret
    // and produce a pod that starts, fails to connect, and looks like a
    // network problem.
    let Some(relay_url) = identity_component(&agent.relay_url) else {
        return Err("deploy refused: relay_url is empty — the agent would have \
                    no relay to connect to"
            .to_string());
    };
    env.insert("BEEKEEPER_RELAY_URL".into(), relay_url.to_string());
    env.insert(
        "BEEKEEPER_PRIVATE_KEY".into(),
        agent.private_key_nsec.clone(),
    );
    // The git credential/signing helpers read NOSTR_PRIVATE_KEY.
    env.insert("NOSTR_PRIVATE_KEY".into(), agent.private_key_nsec.clone());

    // Owner: at least one of these must resolve, or the harness cannot match
    // `!shutdown` and §Stop describes a mechanism that does not work.
    let auth_tag = agent.auth_tag.as_deref().and_then(identity_component);
    let owner = launch.owner_pubkey.as_deref().and_then(identity_component);
    match (auth_tag, owner) {
        (None, None) => {
            return Err("deploy refused: neither auth_tag nor launch.owner_pubkey \
                        resolved — without an owner the agent cannot honor \
                        !shutdown"
                .to_string())
        }
        (tag, own) => {
            if let Some(t) = tag {
                env.insert("BEEKEEPER_AUTH_TAG".into(), t.to_string());
            }
            if let Some(o) = own {
                env.insert("BEEKEEPER_ACP_AGENT_OWNER".into(), o.to_string());
            }
        }
    }

    // The harness and MCP binaries are resolved against the *image's* PATH.
    // A host path forwarded from the desktop is guaranteed absent in the
    // container (§Launch data, host-resolved values).
    if let Some(command) = launch.command.as_deref().filter(|c| !c.is_empty()) {
        env.insert(
            "BEEKEEPER_ACP_AGENT_COMMAND".into(),
            pod_command_name(command).to_string(),
        );
    }
    if !launch.args.is_empty() {
        // Comma-joined because that is what the harness's CLI parser decodes,
        // and what the desktop's local spawn does. An argument containing a
        // comma is unrepresentable in both paths; inventing an escaping
        // scheme here would produce args the harness cannot decode.
        env.insert("BEEKEEPER_ACP_AGENT_ARGS".into(), launch.args.join(","));
    }
    env.insert(
        "BEEKEEPER_ACP_MCP_COMMAND".into(),
        pod_command_name("beekeeper-dev-mcp").to_string(),
    );

    if let Some(respond_to) = agent.respond_to.as_deref().filter(|s| !s.is_empty()) {
        validate_respond_to_gate(respond_to, agent.respond_to_allowlist.as_deref())?;
        env.insert("BEEKEEPER_ACP_RESPOND_TO".into(), respond_to.to_string());
    }
    if let Some(list) = agent
        .respond_to_allowlist
        .as_ref()
        .filter(|l| !l.is_empty())
    {
        env.insert("BEEKEEPER_ACP_RESPOND_TO_ALLOWLIST".into(), list.join(","));
    }

    if let Some(secs) = auth.inactivity_seconds {
        env.insert(
            "BEEKEEPER_ACP_EXIT_AFTER_INACTIVITY".into(),
            secs.to_string(),
        );
    }
    // The generation token doubles as the lifecycle-frame correlator, so pod
    // logs and observer frames share one identity (§K8s Secrets).
    env.insert(START_NONCE_KEY.into(), auth.generation.to_string());

    // Dual-write last, so every name above — authoritative or not — reaches
    // an old image under the spelling it reads. The size cap is checked on
    // what the Secret will actually hold.
    let env = mirror_map(env);

    let total: usize = env.values().map(String::len).sum();
    if total > MAX_SECRET_BYTES {
        return Err(format!(
            "agent environment is {total} bytes; Kubernetes caps Secret data \
             at {MAX_SECRET_BYTES}"
        ));
    }

    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload_json(extra_agent: serde_json::Value) -> AgentPayload {
        let mut agent = serde_json::json!({
            "name": "a",
            "relay_url": "wss://relay.example",
            "private_key_nsec": "nsec1example",
            "auth_tag": "tag-1",
        });
        let (serde_json::Value::Object(base), serde_json::Value::Object(extra)) =
            (&mut agent, extra_agent)
        else {
            panic!("expected objects")
        };
        base.extend(extra);
        serde_json::from_value(agent).unwrap()
    }

    fn build(agent: &AgentPayload) -> Result<BTreeMap<String, String>, String> {
        build_env(
            agent,
            AuthoritativeInputs {
                generation: "gen0001",
                inactivity_seconds: Some(7200),
            },
        )
    }

    #[test]
    fn identity_comes_from_top_level_fields() {
        let env = build(&payload_json(serde_json::json!({}))).unwrap();
        assert_eq!(env["BEEKEEPER_RELAY_URL"], "wss://relay.example");
        assert_eq!(env["BEEKEEPER_PRIVATE_KEY"], "nsec1example");
        assert_eq!(env["NOSTR_PRIVATE_KEY"], "nsec1example");
        assert_eq!(env["BEEKEEPER_AUTH_TAG"], "tag-1");
    }

    /// Wren's amendment, and the spec's later-wins rule: a lower tier that
    /// spoofs an authoritative key is *overwritten*, not refused. Refusing
    /// would diverge from the local spawn, where the same env is written
    /// before the authoritative layer and simply loses.
    #[test]
    fn lower_tiers_cannot_spoof_authoritative_values() {
        let agent = payload_json(serde_json::json!({
            "launch": {
                "command": "goose",
                "policy_env": {
                    "BEEKEEPER_PRIVATE_KEY": "nsec1attacker",
                    "BEEKEEPER_MANAGED_AGENT_START_NONCE": "forged",
                },
                "env": {
                    "BEEKEEPER_RELAY_URL": "wss://attacker.example",
                    "NOSTR_PRIVATE_KEY": "nsec1attacker",
                    "BEEKEEPER_AUTH_TAG": "forged-tag",
                    "BEEKEEPER_ACP_AGENT_OWNER": "cafe",
                    "BEEKEEPER_ACP_AGENT_COMMAND": "/bin/sh",
                    "BEEKEEPER_ACP_MCP_COMMAND": "/bin/sh",
                    "BEEKEEPER_ACP_EXIT_AFTER_INACTIVITY": "0",
                },
                "owner_pubkey": "beef"
            }
        }));
        let env = build(&agent).unwrap();
        assert_eq!(env["BEEKEEPER_PRIVATE_KEY"], "nsec1example");
        assert_eq!(env["NOSTR_PRIVATE_KEY"], "nsec1example");
        assert_eq!(env["BEEKEEPER_RELAY_URL"], "wss://relay.example");
        assert_eq!(env["BEEKEEPER_AUTH_TAG"], "tag-1");
        assert_eq!(env["BEEKEEPER_ACP_AGENT_OWNER"], "beef");
        assert_eq!(env["BEEKEEPER_ACP_AGENT_COMMAND"], "goose");
        assert_eq!(env["BEEKEEPER_ACP_MCP_COMMAND"], "buzz-dev-mcp");
        assert_eq!(env["BEEKEEPER_ACP_EXIT_AFTER_INACTIVITY"], "7200");
        assert_eq!(env["BEEKEEPER_MANAGED_AGENT_START_NONCE"], "gen0001");
    }

    /// Tier 1 is *overridable* — user env beats policy defaults, matching the
    /// local spawn, where the user layer is written after them. Getting this
    /// backwards would make remote agents ignore overrides local agents honor.
    #[test]
    fn user_env_overrides_policy_defaults() {
        let agent = payload_json(serde_json::json!({
            "launch": {
                "policy_env": {"GOOSE_MODE": "auto", "BEEKEEPER_ACP_MODEL": "sonnet"},
                "env": {"GOOSE_MODE": "chat"},
                "owner_pubkey": "beef"
            }
        }));
        let env = build(&agent).unwrap();
        assert_eq!(env["GOOSE_MODE"], "chat");
        assert_eq!(env["BEEKEEPER_ACP_MODEL"], "sonnet");
    }

    /// `launch.env` already contains the merged user env, so re-merging the
    /// legacy field would undo a layering the desktop already resolved.
    #[test]
    fn legacy_env_vars_are_not_remerged_when_launch_present() {
        let agent = payload_json(serde_json::json!({
            "env_vars": {"STALE": "yes", "SHARED": "legacy"},
            "launch": {"env": {"SHARED": "resolved"}, "owner_pubkey": "beef"}
        }));
        let env = build(&agent).unwrap();
        assert_eq!(env["SHARED"], "resolved");
        assert!(!env.contains_key("STALE"), "legacy env_vars re-merged");
    }

    /// ...but a desktop predating the `launch` block has nothing else to
    /// offer, so the legacy field is the truth in exactly that case.
    #[test]
    fn legacy_env_vars_used_when_launch_absent() {
        let agent = payload_json(serde_json::json!({"env_vars": {"API": "v"}}));
        let env = build(&agent).unwrap();
        assert_eq!(env["API"], "v");
    }

    #[test]
    fn refuses_when_no_owner_resolves() {
        let agent = payload_json(serde_json::json!({"auth_tag": null}));
        let err = build(&agent).unwrap_err();
        assert!(err.contains("!shutdown"), "unhelpful error: {err}");
    }

    /// An empty string is not an owner. Without this the refusal is
    /// bypassable by a blank field and the pod launches unable to be stopped.
    #[test]
    fn empty_owner_fields_count_as_absent() {
        for blank in ["", "   "] {
            let agent = payload_json(serde_json::json!({
                "auth_tag": blank,
                "launch": {"owner_pubkey": blank}
            }));
            assert!(
                build(&agent).is_err(),
                "whitespace resolved as an owner: {blank:?}"
            );
        }
    }

    /// The other half of every identity guard: what is *stored*. A validator
    /// that trims and a writer that doesn't disagree about the value, and the
    /// padding reaches the harness inside the Secret. Assert on the stored
    /// string — asserting only that the deploy was accepted passes either way.
    #[test]
    fn identity_components_are_stored_trimmed() {
        let agent = payload_json(serde_json::json!({
            "relay_url": "  wss://relay.example  ",
            "auth_tag": "  tag-1  ",
            "launch": {"owner_pubkey": "  beefcafe  "}
        }));
        let env = build(&agent).unwrap();
        assert_eq!(env["BEEKEEPER_RELAY_URL"], "wss://relay.example");
        assert_eq!(env["BEEKEEPER_AUTH_TAG"], "tag-1");
        assert_eq!(env["BEEKEEPER_ACP_AGENT_OWNER"], "beefcafe");
    }

    /// L1 item 1's third identity component. The nsec arm is enforced in
    /// `naming.rs` and the owner arm above; without this one an agent
    /// deploys with nothing to connect to — a pod that starts, fails at the
    /// relay, and reads as a network fault rather than a refused launch.
    #[test]
    fn refuses_empty_relay_url() {
        for blank in ["", "   "] {
            let agent = payload_json(serde_json::json!({"relay_url": blank}));
            let err = build(&agent).unwrap_err();
            assert!(err.contains("relay_url"), "unhelpful error: {err}");
        }
    }

    #[test]
    fn owner_pubkey_alone_is_sufficient() {
        let agent = payload_json(serde_json::json!({
            "auth_tag": null,
            "launch": {"owner_pubkey": "beefcafe"}
        }));
        let env = build(&agent).unwrap();
        assert_eq!(env["BEEKEEPER_ACP_AGENT_OWNER"], "beefcafe");
        assert!(!env.contains_key("BEEKEEPER_AUTH_TAG"));
    }

    #[test]
    fn refuses_presence_suppression() {
        let agent = payload_json(serde_json::json!({
            "launch": {"env": {"BEEKEEPER_ACP_NO_PRESENCE": "1"}, "owner_pubkey": "beef"}
        }));
        let err = build(&agent).unwrap_err();
        assert!(err.contains("BEEKEEPER_ACP_NO_PRESENCE"), "got: {err}");
    }

    /// `foo.bar` is a legal Secret key but not a legal env name: pre-1.30
    /// kubelets drop it, 1.30+ inject it. Refuse rather than behave
    /// differently depending on the cluster.
    #[test]
    fn refuses_non_posix_env_keys() {
        for bad in ["foo.bar", "foo-bar", "1LEADING", "", "has space"] {
            let agent = payload_json(serde_json::json!({
                "launch": {"env": {bad: "v"}, "owner_pubkey": "beef"}
            }));
            assert!(build(&agent).is_err(), "accepted non-POSIX key {bad:?}");
        }
    }

    #[test]
    fn args_are_comma_joined_and_omitted_when_empty() {
        let agent = payload_json(serde_json::json!({
            "launch": {"command": "goose", "args": ["run", "--no-session"], "owner_pubkey": "b"}
        }));
        let env = build(&agent).unwrap();
        assert_eq!(env["BEEKEEPER_ACP_AGENT_ARGS"], "run,--no-session");

        let agent = payload_json(serde_json::json!({
            "launch": {"command": "goose", "args": [], "owner_pubkey": "b"}
        }));
        assert!(!build(&agent)
            .unwrap()
            .contains_key("BEEKEEPER_ACP_AGENT_ARGS"));
    }

    /// The top-level `model`/`provider` fields are display inputs; their
    /// environment consequence is per-runtime and arrives already resolved
    /// inside `launch`. A provider-side mapping is wrong for three of the
    /// four built-in runtimes.
    #[test]
    fn provider_never_maps_model_or_provider_itself() {
        let agent = payload_json(serde_json::json!({
            "model": "claude-opus", "provider": "anthropic",
            "launch": {"owner_pubkey": "beef"}
        }));
        let env = build(&agent).unwrap();
        for key in [
            "BEEKEEPER_AGENT_PROVIDER",
            "BEEKEEPER_AGENT_MODEL",
            "GOOSE_PROVIDER",
            "GOOSE_MODEL",
        ] {
            assert!(!env.contains_key(key), "provider mapped {key} itself");
        }
    }

    /// `turn_timeout_seconds` is deprecated and ignored upstream; the local
    /// spawn does not emit it either.
    #[test]
    fn turn_timeout_is_not_mapped() {
        let agent = payload_json(serde_json::json!({
            "turn_timeout_seconds": 30, "launch": {"owner_pubkey": "b"}
        }));
        let env = build(&agent).unwrap();
        assert!(!env.keys().any(|k| k.contains("TURN_TIMEOUT")));
    }

    #[test]
    fn inactivity_omitted_when_unset() {
        let agent = payload_json(serde_json::json!({"launch": {"owner_pubkey": "b"}}));
        let env = build_env(
            &agent,
            AuthoritativeInputs {
                generation: "g",
                inactivity_seconds: None,
            },
        )
        .unwrap();
        assert!(!env.contains_key("BEEKEEPER_ACP_EXIT_AFTER_INACTIVITY"));
    }

    /// Structural guard over the whole authoritative list at once: whatever
    /// the lower tiers contain, no authoritative key holds a lower-tier value
    /// — including the conditionally-written ones the authoritative tier has
    /// nothing to say about, which must be **absent** rather than spoofed.
    /// This test caught exactly that: `BEEKEEPER_ACP_AGENT_ARGS` is only written
    /// when `launch.args` is non-empty, so plain later-wins overwrite left the
    /// spoofed value in place.
    #[test]
    fn no_authoritative_key_retains_a_lower_tier_value() {
        // Both spellings: an old image reads the legacy one, so a spoof there
        // is as live as a spoof of the current name.
        let spoofed: serde_json::Map<String, serde_json::Value> = AUTHORITATIVE_KEYS
            .iter()
            .flat_map(|k| both_spellings(k))
            .map(|k| (k, serde_json::json!("SPOOFED")))
            .collect();
        // Split across both lower tiers: policy_env and env are separate
        // insertion points, and a fix that only cleared one would pass a
        // single-tier test.
        let agent = payload_json(serde_json::json!({
            "launch": {
                "command": "goose",
                "policy_env": spoofed.clone(),
                "env": spoofed,
                "owner_pubkey": "beef"
            }
        }));
        let env = build(&agent).unwrap();
        for key in AUTHORITATIVE_KEYS.iter().flat_map(|k| both_spellings(k)) {
            assert_ne!(
                env.get(&key).map(String::as_str),
                Some("SPOOFED"),
                "{key} kept its lower-tier value"
            );
        }
        // The keys the authoritative tier had no value for are gone, not
        // merely different.
        for absent in [
            "BEEKEEPER_ACP_AGENT_ARGS",
            "BEEKEEPER_ACP_RESPOND_TO",
            "BEEKEEPER_ACP_RESPOND_TO_ALLOWLIST",
        ]
        .into_iter()
        .flat_map(both_spellings)
        {
            assert!(!env.contains_key(&absent), "{absent} survived the clear");
        }
    }

    /// A 64-hex pubkey, the only allowlist entry shape the harness accepts.
    fn pubkey(fill: char) -> String {
        std::iter::repeat_n(fill, 64).collect()
    }

    /// The gate the harness refuses first (`config.rs:996-1004`). Refusing it
    /// here is the difference between one error message and an unbounded
    /// fail-replace loop that leaves a Secret per attempt.
    #[test]
    fn allowlist_mode_with_an_empty_list_is_refused() {
        for empty in [serde_json::json!([]), serde_json::Value::Null] {
            let agent = payload_json(serde_json::json!({
                "respond_to": "allowlist",
                "respond_to_allowlist": empty,
            }));
            let err = build(&agent).unwrap_err();
            assert!(
                err.contains("the allowlist is empty"),
                "unexpected error: {err}"
            );
        }
    }

    /// `config.rs:629-641` — each entry must be exactly 64 hex characters.
    /// The rejects are the distinct ways to miss that: too short, right length
    /// but not hex, empty, and one character short of valid.
    #[test]
    fn an_allowlist_entry_that_is_not_64_hex_is_refused() {
        for bad in ["abc1234", &"z".repeat(64), "", &pubkey('a')[..63]] {
            let agent = payload_json(serde_json::json!({
                "respond_to": "allowlist",
                "respond_to_allowlist": [pubkey('a'), bad],
            }));
            let err = build(&agent).unwrap_err();
            assert!(
                err.contains("must be exactly 64 hex characters"),
                "{bad:?} was accepted; error was: {err}"
            );
        }
    }

    /// The positive control: the guard refuses bad gates, not every gate.
    /// Without this, a validator that refused unconditionally would pass both
    /// tests above.
    #[test]
    fn a_valid_allowlist_gate_is_accepted_and_comma_joined() {
        let agent = payload_json(serde_json::json!({
            "respond_to": "allowlist",
            "respond_to_allowlist": [pubkey('a'), pubkey('b')],
        }));
        let env = build(&agent).unwrap();
        assert_eq!(env["BEEKEEPER_ACP_RESPOND_TO"], "allowlist");
        assert_eq!(
            env["BEEKEEPER_ACP_RESPOND_TO_ALLOWLIST"],
            format!("{},{}", pubkey('a'), pubkey('b'))
        );
    }

    /// The harness validates the allowlist **only** in allowlist mode and
    /// merely warns otherwise (`config.rs:1005-1010`). A stricter provider
    /// would refuse a deploy whose identical local spawn succeeds, so this
    /// pins the asymmetry rather than leaving it to look like an oversight.
    #[test]
    fn a_junk_allowlist_is_tolerated_outside_allowlist_mode() {
        for mode in ["owner-only", "anyone"] {
            let agent = payload_json(serde_json::json!({
                "respond_to": mode,
                "respond_to_allowlist": ["not-a-pubkey"],
            }));
            let env = build(&agent)
                .unwrap_or_else(|e| panic!("{mode} with a stale list must deploy: {e}"));
            assert_eq!(env["BEEKEEPER_ACP_RESPOND_TO"], mode);
        }
    }

    /// `respond_to` is an opaque `String` on the wire but a `clap::ValueEnum`
    /// at the harness, so an unrecognized mode dies at `rc=2` — before config
    /// parsing runs at all, earlier than either refusal above. Measured
    /// against the built binary: `invalid value 'npub1abc' for '--respond-to'`.
    /// This is the shape our own fixture carried until it was corrected.
    #[test]
    fn a_mode_the_harness_cannot_parse_is_refused() {
        for bad in ["npub1abc", "OWNER-ONLY", "owner_only", "allowlistt", "x"] {
            let agent = payload_json(serde_json::json!({ "respond_to": bad }));
            let err = build(&agent).unwrap_err();
            assert!(
                err.contains("is not a mode the harness accepts"),
                "{bad:?} was accepted; error was: {err}"
            );
        }
    }

    /// `clap` does not trim its value-enum input, so a padded mode is `rc=2`
    /// even though the same string trimmed is valid. Measured: `invalid value
    /// ' allowlist ' for '--respond-to'`. Trimming here would accept a deploy
    /// the harness refuses — the exact direction this guard exists to prevent.
    #[test]
    fn a_padded_mode_is_refused_because_clap_does_not_trim() {
        for padded in [" allowlist ", "allowlist ", " owner-only", "\tnobody"] {
            let agent = payload_json(serde_json::json!({
                "respond_to": padded,
                "respond_to_allowlist": [pubkey('a')],
            }));
            let err = build(&agent).unwrap_err();
            assert!(
                err.contains("is not a mode the harness accepts"),
                "{padded:?} was accepted; error was: {err}"
            );
        }
    }

    /// Positive control for the mode check, and the reason it validates the
    /// harness's four rather than the desktop's three: `nobody` is rejected by
    /// `parse_wire` on purpose (`managed_agents/types.rs:871-880`) but starts
    /// fine at the harness. A guard mirroring the desktop enum would refuse a
    /// working launch from a non-desktop caller — the callers this guard is
    /// for. Without this test, refusing `nobody` would pass everything above.
    #[test]
    fn every_mode_the_harness_accepts_is_deployable() {
        for mode in ["owner-only", "allowlist", "anyone", "nobody"] {
            let agent = payload_json(serde_json::json!({
                "respond_to": mode,
                "respond_to_allowlist": [pubkey('a')],
            }));
            let env = build(&agent)
                .unwrap_or_else(|e| panic!("{mode} is valid at the harness but was refused: {e}"));
            assert_eq!(env["BEEKEEPER_ACP_RESPOND_TO"], mode);
        }
    }

    // ---- the two spellings ------------------------------------------------

    /// An older desktop sends `BUZZ_*` keys. They must mean exactly what the
    /// current spelling means — including the tier order, which is applied
    /// after normalization so a legacy tier-2 key still beats a current
    /// tier-1 default.
    #[test]
    fn legacy_keys_from_an_older_desktop_mean_the_same_thing() {
        let agent = payload_json(serde_json::json!({
            "launch": {
                "policy_env": {"BUZZ_ACP_MODEL": "sonnet", "BEEKEEPER_ACP_AGENTS": "4"},
                "env": {"BUZZ_ACP_AGENTS": "8"},
                "owner_pubkey": "beef"
            }
        }));
        let env = build(&agent).unwrap();
        assert_eq!(env["BEEKEEPER_ACP_MODEL"], "sonnet");
        assert_eq!(env["BEEKEEPER_ACP_AGENTS"], "8", "tier 2 lost to tier 1");
        assert_eq!(env["BUZZ_ACP_AGENTS"], "8", "the mirror disagrees");
    }

    /// Within one received map, the current spelling wins a collision.
    #[test]
    fn the_current_spelling_wins_inside_one_received_map() {
        let agent = payload_json(serde_json::json!({
            "launch": {
                "env": {"BUZZ_ACP_MODEL": "stale", "BEEKEEPER_ACP_MODEL": "current"},
                "owner_pubkey": "beef"
            }
        }));
        let env = build(&agent).unwrap();
        assert_eq!(env["BEEKEEPER_ACP_MODEL"], "current");
        assert_eq!(env["BUZZ_ACP_MODEL"], "current");
    }

    /// A desktop predating the `launch` block, with legacy keys, still works.
    #[test]
    fn legacy_env_vars_with_legacy_keys_are_normalized() {
        let agent = payload_json(serde_json::json!({"env_vars": {"BUZZ_ACP_MODEL": "m"}}));
        let env = build(&agent).unwrap();
        assert_eq!(env["BEEKEEPER_ACP_MODEL"], "m");
    }

    /// A digest-pinned image built before the rename reads only `BUZZ_*`, so
    /// every current name must arrive under both spellings with one value.
    #[test]
    fn the_pod_env_carries_both_spellings_of_every_name() {
        let agent = payload_json(serde_json::json!({
            "launch": {
                "command": "goose",
                "args": ["acp"],
                "policy_env": {"BEEKEEPER_ACP_MODEL": "sonnet", "GOOSE_MODE": "auto"},
                "owner_pubkey": "beef"
            }
        }));
        let env = build(&agent).unwrap();
        let current: Vec<&String> = env.keys().filter(|k| k.starts_with("BEEKEEPER_")).collect();
        assert!(
            current.len() >= 8,
            "too few names to mean anything: {current:?}"
        );
        for key in current {
            let twin = beekeeper_core::env_compat::legacy_twin(key).unwrap();
            assert_eq!(
                env.get(&twin),
                Some(&env[key]),
                "{twin} missing or different"
            );
        }
        assert!(env.contains_key("NOSTR_PRIVATE_KEY"));
        assert!(!env
            .keys()
            .any(|k| k == "BUZZ_NOSTR_PRIVATE_KEY" || k == "BUZZ_GOOSE_MODE"));
    }

    /// The presence refusal cannot be dodged by the legacy spelling, in any
    /// case.
    #[test]
    fn refuses_presence_suppression_under_either_spelling() {
        for key in [
            "BUZZ_ACP_NO_PRESENCE",
            "buzz_acp_no_presence",
            "beekeeper_acp_no_presence",
        ] {
            for tier in ["env", "policy_env"] {
                let agent = payload_json(serde_json::json!({
                    "launch": {tier: {key: "1"}, "owner_pubkey": "beef"}
                }));
                let err = build(&agent).unwrap_err();
                assert!(
                    err.contains("BEEKEEPER_ACP_NO_PRESENCE"),
                    "{key} in {tier}: {err}"
                );
            }
        }
    }

    /// The pod ABI: the bundled agent and MCP server reach the pod under the
    /// `buzz-*` names every image ships, whichever name the desktop sent.
    #[test]
    fn bundled_commands_reach_the_pod_under_the_names_every_image_ships() {
        for (sent, expected) in [
            ("beekeeper-agent", "buzz-agent"),
            ("buzz-agent", "buzz-agent"),
            ("goose", "goose"),
            ("/opt/bin/beekeeper-agent", "/opt/bin/beekeeper-agent"),
        ] {
            let agent = payload_json(serde_json::json!({
                "launch": {"command": sent, "owner_pubkey": "beef"}
            }));
            let env = build(&agent).unwrap();
            assert_eq!(env["BEEKEEPER_ACP_AGENT_COMMAND"], expected, "{sent}");
            assert_eq!(env["BUZZ_ACP_AGENT_COMMAND"], expected, "{sent}");
            assert_eq!(env["BEEKEEPER_ACP_MCP_COMMAND"], "buzz-dev-mcp");
            assert_eq!(env["BUZZ_ACP_MCP_COMMAND"], "buzz-dev-mcp");
        }
        assert_eq!(pod_command_name("beekeeper-acp"), "buzz-acp");
        assert_eq!(pod_command_name("beekeeper-dev-mcp"), "buzz-dev-mcp");
    }

    /// The reconciler's per-attempt restamp must move both spellings, or an
    /// old image's harness reports the first attempt's generation forever.
    #[test]
    fn restamping_the_generation_moves_both_spellings() {
        let mut env = build(&payload_json(serde_json::json!({}))).unwrap();
        assert_eq!(env["BUZZ_MANAGED_AGENT_START_NONCE"], "gen0001");
        stamp_generation(&mut env, "gen0002");
        assert_eq!(env[START_NONCE_KEY], "gen0002");
        assert_eq!(env["BUZZ_MANAGED_AGENT_START_NONCE"], "gen0002");

        // Without mirrors, the restamp does not invent one.
        let mut bare = BTreeMap::new();
        stamp_generation(&mut bare, "g");
        assert_eq!(bare.len(), 1);
        assert_eq!(bare[START_NONCE_KEY], "g");
    }

    /// The pre-rename fixture is the full desktop payload as an older desktop
    /// recorded it, `BUZZ_ACP_*` keys and all. It must resolve to exactly the
    /// pod environment the current payload does.
    #[test]
    fn the_pre_rename_fixture_builds_the_same_pod_env() {
        let agent_of = |raw: &str| -> AgentPayload {
            let request: crate::wire::DeployRequest = serde_json::from_str(raw).unwrap();
            request.agent
        };
        let current = agent_of(include_str!(
            "../tests/fixtures/provider-wire/deploy-full-launch.request.json"
        ));
        let legacy = agent_of(include_str!(
            "../tests/fixtures/provider-wire/deploy-legacy-launch.request.json"
        ));
        assert!(
            legacy
                .launch
                .as_ref()
                .unwrap()
                .policy_env
                .keys()
                .any(|k| k.starts_with("BUZZ_")),
            "the legacy fixture no longer carries legacy keys"
        );
        let built = build(&legacy).unwrap();
        assert_eq!(built, build(&current).unwrap());
        assert_eq!(built["BEEKEEPER_ACP_MODEL"], "gpt-5");
        assert_eq!(built["BUZZ_ACP_MODEL"], "gpt-5");
    }
}
