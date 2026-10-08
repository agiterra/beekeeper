use super::*;

const NOW: u64 = 1_790_000_000;
const NONCE: &str = "00112233445566778899aabbccddeeff";

fn target(session: &str, generation: u64) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude".into(),
        instance_id: "inst-1".into(),
        session_id: session.into(),
        generation,
    }
}

fn channel() -> Uuid {
    Uuid::parse_str("6f1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e6f").expect("uuid")
}

fn request(owner: &Keys, session: &str, generation: u64) -> PreviewGrantRequest {
    PreviewGrantRequest {
        channel_id: channel(),
        target: target(session, generation),
        execution_id: format!("exec-{session}-{generation}"),
        audience: preview_grant_audience(&owner.public_key()),
        ttl_secs: 3600,
    }
}

struct Rig {
    provider: Keys,
    owner: Keys,
}

impl Rig {
    fn new() -> Self {
        Self {
            provider: Keys::generate(),
            owner: Keys::generate(),
        }
    }
    fn mint(&self, session: &str, generation: u64) -> String {
        mint_preview_grant(
            &self.provider,
            &request(&self.owner, session, generation),
            NOW,
        )
        .expect("mint")
    }
    fn audience(&self) -> String {
        preview_grant_audience(&self.owner.public_key())
    }
    fn verify(&self, token: &str, now: u64) -> Result<VerifiedPreviewGrant, PreviewGrantError> {
        verify_preview_grant(
            Some(token),
            &[self.provider.public_key()],
            &self.audience(),
            now,
        )
    }
}

#[test]
fn round_trip_verifies_and_binds_the_session() {
    let rig = Rig::new();
    let token = rig.mint("S", 1);
    assert!(token.starts_with(PREVIEW_GRANT_TOKEN_PREFIX));
    let grant = rig.verify(&token, NOW + 10).expect("verify");
    assert_eq!(grant.claims().channel_id, channel());
    assert_eq!(grant.claims().target, target("S", 1));
    assert_eq!(grant.claims().issuer, rig.provider.public_key().to_hex());
    assert_eq!(grant.claims().nonce.len(), PREVIEW_GRANT_NONCE_HEX_LEN);
    assert_eq!(grant.binding().target, Some(target("S", 1)));
    grant.check_binding(&grant.binding()).expect("own binding");
}

#[test]
fn nonces_are_fresh() {
    let rig = Rig::new();
    let a = decode_preview_grant_unverified(&rig.mint("S", 1)).expect("a");
    let b = decode_preview_grant_unverified(&rig.mint("S", 1)).expect("b");
    assert_ne!(a.nonce, b.nonce);
}

#[test]
fn missing_grant_is_preview_no_grant() {
    let rig = Rig::new();
    for token in [None, Some(""), Some("   ")] {
        let err = verify_preview_grant(token, &[rig.provider.public_key()], &rig.audience(), NOW)
            .expect_err("missing");
        assert_eq!(err.code(), "preview_no_grant");
    }
}

#[test]
fn wrong_session_is_refused() {
    let rig = Rig::new();
    let s = rig.verify(&rig.mint("S", 1), NOW).expect("S");
    let s_prime = rig.verify(&rig.mint("S-prime", 1), NOW).expect("S'");
    let err = s_prime
        .check_binding(&s.binding())
        .expect_err("cross-session");
    assert_eq!(err.code(), "preview_wrong_session");

    let other_channel = PreviewSessionBinding {
        channel_id: Uuid::nil(),
        target: None,
    };
    assert_eq!(
        s.check_binding(&other_channel).expect_err("channel").code(),
        "preview_wrong_session"
    );
}

#[test]
fn person_opened_preview_accepts_any_grant_for_its_channel() {
    let rig = Rig::new();
    let grant = rig.verify(&rig.mint("S", 3), NOW).expect("grant");
    let person = PreviewSessionBinding {
        channel_id: channel(),
        target: None,
    };
    grant.check_binding(&person).expect("person-opened");
}

#[test]
fn newer_generation_drives_older_generation_does_not() {
    let rig = Rig::new();
    let gen2 = rig.verify(&rig.mint("S", 2), NOW).expect("gen2");
    let gen3 = rig.verify(&rig.mint("S", 3), NOW).expect("gen3");
    gen3.check_binding(&gen2.binding())
        .expect("restore keeps driving");
    assert_eq!(
        gen2.check_binding(&gen3.binding())
            .expect_err("stale")
            .code(),
        "preview_wrong_session"
    );
}

#[test]
fn expired_and_future_grants_are_refused() {
    let rig = Rig::new();
    let token = rig.mint("S", 1);
    assert_eq!(
        rig.verify(&token, NOW + 3600).expect_err("expired").code(),
        "preview_grant_expired"
    );
    rig.verify(&token, NOW + 3599).expect("last second");
    assert_eq!(
        rig.verify(&token, NOW - PREVIEW_GRANT_MAX_FUTURE_SKEW_SECS - 1)
            .expect_err("future")
            .code(),
        "preview_grant_not_yet_valid"
    );
    rig.verify(&token, NOW - PREVIEW_GRANT_MAX_FUTURE_SKEW_SECS)
        .expect("within skew");
}

#[test]
fn untrusted_issuer_is_refused() {
    let rig = Rig::new();
    let stranger = Keys::generate();
    let token = mint_preview_grant(&stranger, &request(&rig.owner, "S", 1), NOW).expect("mint");
    assert_eq!(
        rig.verify(&token, NOW).expect_err("issuer").code(),
        "preview_wrong_issuer"
    );
}

#[test]
fn wrong_audience_is_refused() {
    let rig = Rig::new();
    let token = rig.mint("S", 1);
    let other = preview_grant_audience(&Keys::generate().public_key());
    let err = verify_preview_grant(Some(&token), &[rig.provider.public_key()], &other, NOW)
        .expect_err("audience");
    assert_eq!(err.code(), "preview_wrong_audience");
}

#[test]
fn tampered_claims_fail_the_signature() {
    let rig = Rig::new();
    let token = rig.mint("S", 1);
    let body = token
        .strip_prefix(PREVIEW_GRANT_TOKEN_PREFIX)
        .expect("prefix");
    let (claims_b64, sig) = body.split_once('.').expect("dot");
    let mut claims: PreviewGrantClaims =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims_b64).expect("b64")).expect("json");
    claims.target.session_id = "S-prime".into();
    let forged = format!(
        "{PREVIEW_GRANT_TOKEN_PREFIX}{}.{sig}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("ser"))
    );
    assert_eq!(
        rig.verify(&forged, NOW).expect_err("forged").code(),
        "preview_grant_bad_signature"
    );
}

#[test]
fn non_canonical_and_garbage_tokens_are_malformed() {
    let rig = Rig::new();
    let token = rig.mint("S", 1);
    let body = token
        .strip_prefix(PREVIEW_GRANT_TOKEN_PREFIX)
        .expect("prefix");
    let (claims_b64, sig) = body.split_once('.').expect("dot");
    let bytes = URL_SAFE_NO_PAD.decode(claims_b64).expect("b64");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    let pretty = serde_json::to_vec_pretty(&value).expect("pretty");
    let reencoded = format!(
        "{PREVIEW_GRANT_TOKEN_PREFIX}{}.{sig}",
        URL_SAFE_NO_PAD.encode(pretty)
    );
    let long = "x".repeat(MAX_PREVIEW_GRANT_TOKEN_BYTES + 1);
    let upper_sig = format!(
        "{PREVIEW_GRANT_TOKEN_PREFIX}{claims_b64}.{}",
        sig.to_ascii_uppercase()
    );
    for bad in [
        reencoded.as_str(),
        "bkpg1.",
        "bkpg2.abc.def",
        "bkpg1.!!!.00",
        upper_sig.as_str(),
        long.as_str(),
    ] {
        assert_eq!(
            rig.verify(bad, NOW).expect_err(bad).code(),
            "preview_grant_malformed",
            "{bad}"
        );
    }
}

#[test]
fn unknown_claim_fields_are_malformed() {
    let rig = Rig::new();
    let token = rig.mint("S", 1);
    let body = token
        .strip_prefix(PREVIEW_GRANT_TOKEN_PREFIX)
        .expect("prefix");
    let (claims_b64, sig) = body.split_once('.').expect("dot");
    let mut value: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims_b64).expect("b64")).expect("json");
    value["socketPath"] = serde_json::json!("/tmp/x.sock");
    let token = format!(
        "{PREVIEW_GRANT_TOKEN_PREFIX}{}.{sig}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&value).expect("ser"))
    );
    assert_eq!(
        rig.verify(&token, NOW).expect_err("unknown").code(),
        "preview_grant_malformed"
    );
}

#[test]
fn mint_refuses_bad_requests() {
    let rig = Rig::new();
    let mut req = request(&rig.owner, "S", 1);
    req.ttl_secs = PREVIEW_GRANT_MAX_TTL_SECS + 1;
    assert_eq!(
        mint_preview_grant(&rig.provider, &req, NOW)
            .expect_err("ttl")
            .code(),
        "preview_grant_invalid"
    );
    let mut req = request(&rig.owner, "S", 0);
    req.ttl_secs = 60;
    assert!(mint_preview_grant(&rig.provider, &req, NOW).is_err());
    let mut req = request(&rig.owner, "S", 1);
    req.audience = "somewhere-else".into();
    assert!(mint_preview_grant(&rig.provider, &req, NOW).is_err());
    let req = request(&rig.owner, "S", 1);
    assert!(mint_preview_grant_with_nonce(&rig.provider, &req, NOW, "short").is_err());
}

#[test]
fn claims_carry_no_paths_ports_or_urls() {
    let rig = Rig::new();
    let claims = decode_preview_grant_unverified(&rig.mint("S", 1)).expect("decode");
    let value = serde_json::to_value(&claims).expect("value");
    let mut keys: Vec<&str> = value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "audience",
            "channelId",
            "executionId",
            "expiresAt",
            "issuedAt",
            "issuer",
            "nonce",
            "schema",
            "target"
        ]
    );
    let mut without_schema = value.clone();
    without_schema["schema"] = serde_json::Value::Null;
    let text = without_schema.to_string();
    for needle in ["/", "sock", "http", "127.0.0.1", "localhost"] {
        assert!(!text.contains(needle), "{needle} in {text}");
    }
}

#[test]
fn error_codes_are_stable() {
    let codes: Vec<&str> = [
        PreviewGrantError::Missing,
        PreviewGrantError::Malformed(String::new()),
        PreviewGrantError::Invalid(String::new()),
        PreviewGrantError::WrongIssuer,
        PreviewGrantError::BadSignature,
        PreviewGrantError::WrongAudience,
        PreviewGrantError::Expired,
        PreviewGrantError::NotYetValid,
        PreviewGrantError::WrongSession(String::new()),
    ]
    .iter()
    .map(PreviewGrantError::code)
    .collect();
    assert_eq!(
        codes,
        [
            "preview_no_grant",
            "preview_grant_malformed",
            "preview_grant_invalid",
            "preview_wrong_issuer",
            "preview_grant_bad_signature",
            "preview_wrong_audience",
            "preview_grant_expired",
            "preview_grant_not_yet_valid",
            "preview_wrong_session",
        ]
    );
}

#[test]
fn fixed_nonce_mint_is_deterministic_in_claims() {
    let rig = Rig::new();
    let req = request(&rig.owner, "S", 1);
    let token = mint_preview_grant_with_nonce(&rig.provider, &req, NOW, NONCE).expect("mint");
    let claims = rig.verify(&token, NOW).expect("verify");
    assert_eq!(claims.claims().nonce, NONCE);
    let again = mint_preview_grant_with_nonce(&rig.provider, &req, NOW, NONCE).expect("mint");
    assert_eq!(
        token.rsplit_once('.').expect("dot").0,
        again.rsplit_once('.').expect("dot").0
    );
}
