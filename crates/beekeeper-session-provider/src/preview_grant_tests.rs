use super::*;

use beekeeper_core::preview_grant::{
    decode_preview_grant_unverified, verify_preview_grant, PreviewGrantError, PreviewSessionBinding,
};

const NOW: u64 = 1_790_000_000;

fn target(generation: u64) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude".into(),
        instance_id: "inst-1".into(),
        session_id: "S".into(),
        generation,
    }
}

fn channel() -> Uuid {
    Uuid::parse_str("6f1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e6f").expect("uuid")
}

fn auth_tag(owner: &PublicKey) -> nostr::Tag {
    nostr::Tag::parse(["auth", &owner.to_hex(), "", &"ab".repeat(64)]).expect("tag")
}

fn mint(provider: &Keys, owner: &Keys, generation: u64) -> [(String, String); 2] {
    preview_env(
        provider,
        attested_owner(Some(&auth_tag(&owner.public_key()))),
        Some(PathBuf::from(
            "/home/u/.local/state/buzz/session-broker.sock",
        )),
        channel(),
        &target(generation),
        "create-1",
        NOW,
    )
    .expect("mints")
}

#[test]
fn a_grant_verifies_for_the_owners_desktop_and_binds_its_session() {
    let provider = Keys::generate();
    let owner = Keys::generate();
    let [(grant_name, token), (sock_name, sock)] = mint(&provider, &owner, 2);
    assert_eq!(grant_name, PREVIEW_GRANT_ENV);
    assert_eq!(sock_name, SESSION_BROKER_SOCK_ENV);
    assert_eq!(sock, "/home/u/.local/state/buzz/session-broker.sock");

    let audience = preview_grant_audience(&owner.public_key());
    let verified = verify_preview_grant(Some(&token), &[provider.public_key()], &audience, NOW + 1)
        .expect("verifies");
    let claims = verified.claims();
    assert_eq!(claims.channel_id, channel());
    assert_eq!(claims.target, target(2));
    assert_eq!(claims.execution_id, "create-1");
    assert_eq!(claims.issuer, provider.public_key().to_hex());
    assert_eq!(
        claims.expires_at - claims.issued_at,
        PREVIEW_GRANT_DEFAULT_TTL_SECS
    );
    // No host path reaches the token.
    assert!(!format!("{claims:?}").contains("session-broker"));

    // Another session's preview on the same channel is refused by name.
    let foreign = PreviewSessionBinding {
        channel_id: channel(),
        target: Some(CodingSessionTarget {
            session_id: "S-prime".into(),
            ..target(1)
        }),
    };
    assert_eq!(
        verified.check_binding(&foreign).map_err(|e| e.code()),
        Err("preview_wrong_session")
    );
    let other_channel = PreviewSessionBinding {
        channel_id: Uuid::nil(),
        target: None,
    };
    assert_eq!(
        verified.check_binding(&other_channel).map_err(|e| e.code()),
        Err("preview_wrong_session")
    );
    assert!(verified.check_binding(&verified.binding()).is_ok());
}

#[test]
fn a_grant_is_refused_by_another_identitys_desktop_and_after_it_expires() {
    let provider = Keys::generate();
    let owner = Keys::generate();
    let [(_, token), _] = mint(&provider, &owner, 1);
    let stranger = preview_grant_audience(&Keys::generate().public_key());
    assert_eq!(
        verify_preview_grant(Some(&token), &[provider.public_key()], &stranger, NOW),
        Err(PreviewGrantError::WrongAudience)
    );
    let audience = preview_grant_audience(&owner.public_key());
    assert_eq!(
        verify_preview_grant(
            Some(&token),
            &[provider.public_key()],
            &audience,
            NOW + PREVIEW_GRANT_DEFAULT_TTL_SECS
        ),
        Err(PreviewGrantError::Expired)
    );
    assert_eq!(
        verify_preview_grant(Some(&token), &[owner.public_key()], &audience, NOW),
        Err(PreviewGrantError::WrongIssuer)
    );
}

#[test]
fn renewal_is_a_fresh_mint_and_a_newer_generation_still_drives() {
    let provider = Keys::generate();
    let owner = Keys::generate();
    let [(_, first), _] = mint(&provider, &owner, 1);
    let [(_, again), _] = mint(&provider, &owner, 1);
    assert_ne!(first, again, "every spawn mints a new nonce");
    let [(_, resumed), _] = mint(&provider, &owner, 2);
    let audience = preview_grant_audience(&owner.public_key());
    let issuers = [provider.public_key()];
    let older = verify_preview_grant(Some(&first), &issuers, &audience, NOW).expect("older");
    let newer = verify_preview_grant(Some(&resumed), &issuers, &audience, NOW).expect("newer");
    assert!(newer.check_binding(&older.binding()).is_ok());
    assert_eq!(
        older.check_binding(&newer.binding()).map_err(|e| e.code()),
        Err("preview_wrong_session")
    );
}

#[test]
fn no_attestation_or_no_socket_pushes_nothing() {
    let provider = Keys::generate();
    let sock = Some(PathBuf::from("/s.sock"));
    assert_eq!(
        preview_env(&provider, None, sock, channel(), &target(1), "c", NOW),
        Err(PreviewEnvSkip::NoOwner)
    );
    let owner = Some(Keys::generate().public_key());
    assert_eq!(
        preview_env(&provider, owner, None, channel(), &target(1), "c", NOW),
        Err(PreviewEnvSkip::NoSocket)
    );
    assert_eq!(
        preview_env(
            &provider,
            owner,
            Some(PathBuf::from("/s.sock")),
            channel(),
            &target(1),
            "",
            NOW
        ),
        Err(PreviewEnvSkip::Mint("preview_grant_invalid".into()))
    );
}

#[test]
fn the_owner_is_read_from_the_nip_oa_tag_only() {
    let owner = Keys::generate().public_key();
    assert_eq!(attested_owner(Some(&auth_tag(&owner))), Some(owner));
    assert_eq!(attested_owner(None), None);
    let wrong_name = nostr::Tag::parse(["p", &owner.to_hex()]).expect("tag");
    assert_eq!(attested_owner(Some(&wrong_name)), None);
    let not_a_key = nostr::Tag::parse(["auth", "owner", "", "sig"]).expect("tag");
    assert_eq!(attested_owner(Some(&not_a_key)), None);
}

#[test]
fn the_socket_follows_the_desktop_instance_rule() {
    let home = Some(PathBuf::from("/Users/u"));
    let support = Path::new("/Users/u/Library/Application Support");
    let state = |app: &str| {
        support
            .join(app)
            .join("session-provider")
            .join("ab".repeat(32))
    };
    let path =
        |dir: &str| PathBuf::from(format!("/Users/u/.local/state/{dir}/session-broker.sock"));

    for (app, expected) in [
        ("io.agiterra.beekeeper.app", "buzz"),
        ("io.agiterra.beekeeper.app.dev", "buzz-dev"),
        ("io.agiterra.beekeeper.app.dev.my-worktree", "buzz-dev"),
        ("io.agiterra.beekeeper.app.developer", "buzz"),
    ] {
        assert_eq!(
            broker_socket_path(None, home.clone(), None, &state(app)),
            Some(path(expected)),
            "{app}"
        );
    }
    // The state directory decides even when the host variable disagrees.
    assert_eq!(
        broker_socket_path(
            None,
            home.clone(),
            Some("dev".into()),
            &state("io.agiterra.beekeeper.app")
        ),
        Some(path("buzz"))
    );
    // A server-shaped state directory falls back to the host's instance.
    let server = Path::new("/var/lib/beekeeper/provider");
    assert_eq!(
        broker_socket_path(None, home.clone(), Some("dev".into()), server),
        Some(path("buzz-dev"))
    );
    assert_eq!(
        broker_socket_path(None, home.clone(), None, server),
        Some(path("buzz"))
    );
    // An explicit override wins; a relative one is not a socket path.
    assert_eq!(
        broker_socket_path(Some("/run/b.sock".into()), home.clone(), None, server),
        Some(PathBuf::from("/run/b.sock"))
    );
    assert_eq!(
        broker_socket_path(Some("b.sock".into()), home.clone(), None, server),
        None
    );
    assert_eq!(
        broker_socket_path(Some("".into()), home, None, server),
        Some(path("buzz"))
    );
    assert_eq!(broker_socket_path(None, None, None, server), None);
}

#[test]
fn the_token_decodes_to_the_session_it_was_minted_for() {
    let [(_, token), _] = mint(&Keys::generate(), &Keys::generate(), 3);
    let claims = decode_preview_grant_unverified(&token).expect("decodes");
    assert_eq!(claims.target.generation, 3);
    assert_eq!(claims.target.session_id, "S");
}
