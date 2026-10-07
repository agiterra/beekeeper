//! The relay-mesh env sniff across the `BUZZ_*` → `BEEKEEPER_*` rename.

use super::*;

fn mesh_env_with_provider_keys(providers: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut env = BTreeMap::from([
        (
            "OPENAI_COMPAT_BASE_URL".to_string(),
            "http://127.0.0.1:9337/v1".to_string(),
        ),
        ("OPENAI_COMPAT_MODEL".to_string(), "Qwen3".to_string()),
        (
            "OPENAI_COMPAT_API_KEY".to_string(),
            RELAY_MESH_API_KEY_PLACEHOLDER.to_string(),
        ),
    ]);
    for (key, value) in providers {
        env.insert((*key).to_string(), (*value).to_string());
    }
    env
}

/// A record written between #971 and the `BUZZ_*` → `BEEKEEPER_*` rename
/// carries `BUZZ_AGENT_PROVIDER`. It still resolves mesh.
#[test]
fn legacy_env_sniff_reads_the_buzz_provider_key() {
    let mut rec = record(None, None, None, None);
    rec.env_vars = mesh_env_with_provider_keys(&[("BUZZ_AGENT_PROVIDER", "openai")]);
    assert_eq!(
        resolve_effective_relay_mesh_model_id(&rec, &[], &global(None, None)).as_deref(),
        Some("Qwen3"),
    );
}

/// Lookup order is BEEKEEPER_, then BUZZ_, then SPROUT_: the newest spelling
/// present states current intent.
#[test]
fn legacy_env_sniff_provider_key_precedence_is_newest_first() {
    type Case<'a> = (&'a [(&'a str, &'a str)], Option<&'a str>);
    let cases: &[Case<'_>] = &[
        (
            &[
                ("BEEKEEPER_AGENT_PROVIDER", "anthropic"),
                ("BUZZ_AGENT_PROVIDER", "openai"),
            ],
            None,
        ),
        (
            &[
                ("BUZZ_AGENT_PROVIDER", "anthropic"),
                ("SPROUT_AGENT_PROVIDER", "openai"),
            ],
            None,
        ),
        (
            &[
                ("BUZZ_AGENT_PROVIDER", "openai"),
                ("SPROUT_AGENT_PROVIDER", "anthropic"),
            ],
            Some("Qwen3"),
        ),
    ];
    for (providers, expected) in cases {
        let mut rec = record(None, None, None, None);
        rec.env_vars = mesh_env_with_provider_keys(providers);
        assert_eq!(
            resolve_effective_relay_mesh_model_id(&rec, &[], &global(None, None)).as_deref(),
            *expected,
            "{providers:?}"
        );
    }
}
