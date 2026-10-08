//! The per-session **preview grant** (SV-33 S1/S2): a bearer token the
//! coding-session provider mints for every execution it spawns or restores,
//! and the desktop's session broker verifies before it lets that execution
//! drive a local Browser preview.
//!
//! The grant is how the broker knows *which session* a `bee preview` call
//! speaks for: the CLI takes no session argument, so the session comes only
//! from here. It is Schnorr-signed by the provider key the desktop already
//! trusts for that provider's transcripts, addressed to this machine's
//! desktop broker, short-lived, and carries a random nonce.
//!
//! Isolation, not security: the token stops one session's agent from
//! accidentally driving another session's preview. It is a bearer token,
//! read from the execution's environment ([`PREVIEW_GRANT_ENV`]), and it is
//! never published. It carries no host path, port, URL or socket location,
//! so nothing in it is worth leaking even if a transcript echoes it.
//!
//! Token form: `bkpg1.<base64url(claims JSON)>.<128 lowercase hex sig>`. The
//! signature covers `sha256(SIGNING_DOMAIN || claims bytes)`; the claims bytes
//! must be the canonical serialization of [`PreviewGrantClaims`] (fields in
//! declaration order, no whitespace), so one grant has exactly one encoding.

use std::str::FromStr;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use nostr::secp256k1::schnorr::Signature;
use nostr::secp256k1::Message;
use nostr::{Keys, PublicKey, SECP256K1};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::coding_session_command::{
    CodingSessionTarget, MAX_IDENTIFIER_BYTES, MAX_SAFE_GENERATION,
};

/// The only supported preview-grant claims schema.
pub const PREVIEW_GRANT_SCHEMA: &str = "beekeeper-preview-grant/v1";
/// Prefix of every encoded preview-grant token.
pub const PREVIEW_GRANT_TOKEN_PREFIX: &str = "bkpg1.";
/// Environment variable the provider pushes into every execution (seated,
/// unseated and restored) carrying the encoded grant.
pub const PREVIEW_GRANT_ENV: &str = "BEEKEEPER_PREVIEW_GRANT";
/// Environment variable naming the desktop broker socket. The same name the
/// desktop and `bee session` already honour; the provider pushes this
/// machine's app socket (dev or prod) alongside the grant.
pub const SESSION_BROKER_SOCK_ENV: &str = "BUZZ_SESSION_BROKER_SOCK";
/// Audience prefix: a grant names the desktop broker of the identity it was
/// minted for, as `desktop-broker:<64 lowercase hex pubkey>`.
pub const PREVIEW_GRANT_AUDIENCE_PREFIX: &str = "desktop-broker:";
/// Lifetime the provider gives a grant unless told otherwise. Restores and
/// respawns mint a fresh grant, so this only bounds one uninterrupted run.
pub const PREVIEW_GRANT_DEFAULT_TTL_SECS: u64 = 7 * 24 * 60 * 60;
/// Longest lifetime a grant may claim; longer ones are refused at mint and
/// at verify.
pub const PREVIEW_GRANT_MAX_TTL_SECS: u64 = 7 * 24 * 60 * 60;
/// How far a grant's `issuedAt` may lead the verifier's clock.
pub const PREVIEW_GRANT_MAX_FUTURE_SKEW_SECS: u64 = 60;
/// Upper bound on an encoded token, checked before any decoding.
pub const MAX_PREVIEW_GRANT_TOKEN_BYTES: usize = 4 * 1024;
/// Length of the hex nonce (16 random bytes).
pub const PREVIEW_GRANT_NONCE_HEX_LEN: usize = 32;

/// Domain separator hashed in front of the claims bytes before signing, so a
/// preview-grant signature can never be replayed as any other signature.
const SIGNING_DOMAIN: &[u8] = b"beekeeper-preview-grant/v1\n";

/// Every way a grant is refused, each with a stable wire code
/// ([`PreviewGrantError::code`]) the broker returns and the CLI prints.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PreviewGrantError {
    /// No grant was presented (env var unset or empty).
    #[error("no preview grant was presented")]
    Missing,
    /// The token could not be decoded or is not canonical.
    #[error("preview grant is malformed: {0}")]
    Malformed(String),
    /// The claims decoded but violate the schema's bounds.
    #[error("preview grant is invalid: {0}")]
    Invalid(String),
    /// The issuer is not a provider this desktop trusts.
    #[error("preview grant was issued by a provider this app does not trust")]
    WrongIssuer,
    /// The signature does not verify against the claimed issuer.
    #[error("preview grant signature does not verify")]
    BadSignature,
    /// The grant is addressed to a different desktop broker.
    #[error("preview grant is addressed to a different desktop")]
    WrongAudience,
    /// `now` is at or past `expiresAt`.
    #[error("preview grant has expired")]
    Expired,
    /// `issuedAt` leads the verifier's clock by more than the allowed skew.
    #[error("preview grant is not valid yet")]
    NotYetValid,
    /// The grant is for a different session than the preview it would drive.
    #[error("preview grant belongs to a different session: {0}")]
    WrongSession(String),
}

impl PreviewGrantError {
    /// The stable wire code for this refusal.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Missing => "preview_no_grant",
            Self::Malformed(_) => "preview_grant_malformed",
            Self::Invalid(_) => "preview_grant_invalid",
            Self::WrongIssuer => "preview_wrong_issuer",
            Self::BadSignature => "preview_grant_bad_signature",
            Self::WrongAudience => "preview_wrong_audience",
            Self::Expired => "preview_grant_expired",
            Self::NotYetValid => "preview_grant_not_yet_valid",
            Self::WrongSession(_) => "preview_wrong_session",
        }
    }
}

/// The signed content of a preview grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewGrantClaims {
    /// Must equal [`PREVIEW_GRANT_SCHEMA`].
    pub schema: String,
    /// The session's umbrella channel — the key the desktop's Browser surface
    /// and broker index previews by.
    pub channel_id: Uuid,
    /// The exact provider session generation the grant was minted for.
    pub target: CodingSessionTarget,
    /// The provider's execution id for this spawn/restore.
    pub execution_id: String,
    /// The minting provider's pubkey, 64 lowercase hex.
    pub issuer: String,
    /// `desktop-broker:<owner pubkey hex>`; see [`preview_grant_audience`].
    pub audience: String,
    /// Unix seconds at mint.
    pub issued_at: u64,
    /// Unix seconds after which the grant is refused.
    pub expires_at: u64,
    /// 32 lowercase hex characters of fresh randomness.
    pub nonce: String,
}

/// What the provider supplies to mint a grant; issuer and nonce are derived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewGrantRequest {
    /// The session's umbrella channel.
    pub channel_id: Uuid,
    /// The exact session generation being spawned or restored.
    pub target: CodingSessionTarget,
    /// The execution id of this spawn/restore.
    pub execution_id: String,
    /// The audience string, built with [`preview_grant_audience`].
    pub audience: String,
    /// Lifetime in seconds, at most [`PREVIEW_GRANT_MAX_TTL_SECS`].
    pub ttl_secs: u64,
}

/// What a preview is bound to on the desktop side. A preview the agent opened
/// is bound to its grant's channel and target; one the person opened from the
/// Browser surface is bound to its channel only (`target: None`), and any
/// grant for that channel may drive it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewSessionBinding {
    /// The umbrella channel the preview belongs to.
    pub channel_id: Uuid,
    /// The session generation that opened it, when an agent did.
    pub target: Option<CodingSessionTarget>,
}

/// A grant that passed signature, issuer, audience and time checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPreviewGrant {
    claims: PreviewGrantClaims,
}

impl VerifiedPreviewGrant {
    /// The verified claims.
    pub fn claims(&self) -> &PreviewGrantClaims {
        &self.claims
    }

    /// The binding a preview opened under this grant records.
    pub fn binding(&self) -> PreviewSessionBinding {
        PreviewSessionBinding {
            channel_id: self.claims.channel_id,
            target: Some(self.claims.target.clone()),
        }
    }

    /// Refuse with [`PreviewGrantError::WrongSession`] unless this grant may
    /// drive a preview bound to `binding`: same channel, and — when the
    /// preview was opened by an agent — the same driver, instance and
    /// session, at the same or a newer generation (a restore keeps driving the
    /// preview its earlier generation opened; an older generation does not).
    pub fn check_binding(&self, binding: &PreviewSessionBinding) -> Result<(), PreviewGrantError> {
        if self.claims.channel_id != binding.channel_id {
            return Err(PreviewGrantError::WrongSession(
                "the preview belongs to another session's channel".into(),
            ));
        }
        let Some(bound) = binding.target.as_ref() else {
            return Ok(());
        };
        let mine = &self.claims.target;
        if mine.driver != bound.driver
            || mine.instance_id != bound.instance_id
            || mine.session_id != bound.session_id
        {
            return Err(PreviewGrantError::WrongSession(
                "the preview was opened by another session".into(),
            ));
        }
        if mine.generation < bound.generation {
            return Err(PreviewGrantError::WrongSession(
                "the grant is for an older generation of this session".into(),
            ));
        }
        Ok(())
    }
}

/// The audience string for the desktop broker of `owner` — the identity the
/// desktop app on this machine signs in as.
pub fn preview_grant_audience(owner: &PublicKey) -> String {
    format!("{PREVIEW_GRANT_AUDIENCE_PREFIX}{}", owner.to_hex())
}

/// Mint an encoded grant signed by `provider`, with a fresh random nonce.
pub fn mint_preview_grant(
    provider: &Keys,
    request: &PreviewGrantRequest,
    now: u64,
) -> Result<String, PreviewGrantError> {
    let mut nonce = [0u8; PREVIEW_GRANT_NONCE_HEX_LEN / 2];
    rand::fill(&mut nonce);
    mint_preview_grant_with_nonce(provider, request, now, &hex::encode(nonce))
}

/// Mint an encoded grant with a caller-supplied nonce (deterministic tests).
pub fn mint_preview_grant_with_nonce(
    provider: &Keys,
    request: &PreviewGrantRequest,
    now: u64,
    nonce: &str,
) -> Result<String, PreviewGrantError> {
    if request.ttl_secs == 0 || request.ttl_secs > PREVIEW_GRANT_MAX_TTL_SECS {
        return Err(PreviewGrantError::Invalid(format!(
            "ttl must be 1..={PREVIEW_GRANT_MAX_TTL_SECS} seconds"
        )));
    }
    let expires_at = now
        .checked_add(request.ttl_secs)
        .ok_or_else(|| PreviewGrantError::Invalid("expiry overflows".into()))?;
    let claims = PreviewGrantClaims {
        schema: PREVIEW_GRANT_SCHEMA.to_owned(),
        channel_id: request.channel_id,
        target: request.target.clone(),
        execution_id: request.execution_id.clone(),
        issuer: provider.public_key().to_hex(),
        audience: request.audience.clone(),
        issued_at: now,
        expires_at,
        nonce: nonce.to_owned(),
    };
    validate_claims(&claims)?;
    let bytes = canonical_claims_bytes(&claims)?;
    let signature = provider.sign_schnorr(&signing_message(&bytes));
    Ok(format!(
        "{PREVIEW_GRANT_TOKEN_PREFIX}{}.{}",
        URL_SAFE_NO_PAD.encode(&bytes),
        signature
    ))
}

/// Decode a token's claims **without** verifying it. For display and
/// diagnostics only (e.g. `bee preview status` naming the session it would
/// speak for); never an authorization decision.
pub fn decode_preview_grant_unverified(
    token: &str,
) -> Result<PreviewGrantClaims, PreviewGrantError> {
    decode(token).map(|(claims, _, _)| claims)
}

/// Verify `token` for the broker whose audience is `audience`, accepting only
/// grants issued by one of `trusted_issuers`, at unix time `now`.
///
/// Checks run in a fixed order so each refusal has one code: presence,
/// encoding and canonical form, schema bounds, issuer trust, signature,
/// audience, then time. Session binding is a separate step,
/// [`VerifiedPreviewGrant::check_binding`], because it needs the preview.
pub fn verify_preview_grant(
    token: Option<&str>,
    trusted_issuers: &[PublicKey],
    audience: &str,
    now: u64,
) -> Result<VerifiedPreviewGrant, PreviewGrantError> {
    let token = match token.map(str::trim) {
        None | Some("") => return Err(PreviewGrantError::Missing),
        Some(token) => token,
    };
    let (claims, bytes, signature) = decode(token)?;
    validate_claims(&claims)?;
    let issuer = PublicKey::from_hex(&claims.issuer)
        .map_err(|_| PreviewGrantError::Invalid("issuer is not a pubkey".into()))?;
    if !trusted_issuers.contains(&issuer) {
        return Err(PreviewGrantError::WrongIssuer);
    }
    let xonly = issuer
        .xonly()
        .map_err(|_| PreviewGrantError::Invalid("issuer is not a curve point".into()))?;
    SECP256K1
        .verify_schnorr(&signature, &signing_message(&bytes), &xonly)
        .map_err(|_| PreviewGrantError::BadSignature)?;
    if claims.audience != audience {
        return Err(PreviewGrantError::WrongAudience);
    }
    if claims.issued_at > now.saturating_add(PREVIEW_GRANT_MAX_FUTURE_SKEW_SECS) {
        return Err(PreviewGrantError::NotYetValid);
    }
    if now >= claims.expires_at {
        return Err(PreviewGrantError::Expired);
    }
    Ok(VerifiedPreviewGrant { claims })
}

fn decode(token: &str) -> Result<(PreviewGrantClaims, Vec<u8>, Signature), PreviewGrantError> {
    let malformed = |why: &str| PreviewGrantError::Malformed(why.to_owned());
    if token.len() > MAX_PREVIEW_GRANT_TOKEN_BYTES {
        return Err(malformed("token is too long"));
    }
    let body = token
        .strip_prefix(PREVIEW_GRANT_TOKEN_PREFIX)
        .ok_or_else(|| malformed("not a bkpg1 token"))?;
    let (claims_b64, sig_hex) = body
        .split_once('.')
        .ok_or_else(|| malformed("missing signature"))?;
    if sig_hex.len() != 128
        || !sig_hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(malformed("signature is not 128 lowercase hex"));
    }
    let signature = Signature::from_str(sig_hex).map_err(|_| malformed("bad signature bytes"))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(claims_b64)
        .map_err(|_| malformed("claims are not base64url"))?;
    let claims: PreviewGrantClaims =
        serde_json::from_slice(&bytes).map_err(|e| PreviewGrantError::Malformed(e.to_string()))?;
    if canonical_claims_bytes(&claims)? != bytes {
        return Err(malformed("claims are not canonically encoded"));
    }
    Ok((claims, bytes, signature))
}

fn canonical_claims_bytes(claims: &PreviewGrantClaims) -> Result<Vec<u8>, PreviewGrantError> {
    serde_json::to_vec(claims).map_err(|e| PreviewGrantError::Invalid(e.to_string()))
}

fn signing_message(claims_bytes: &[u8]) -> Message {
    let mut hasher = Sha256::new();
    hasher.update(SIGNING_DOMAIN);
    hasher.update(claims_bytes);
    Message::from_digest(hasher.finalize().into())
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn validate_identifier(value: &str, field: &str) -> Result<(), PreviewGrantError> {
    if value.trim().is_empty() {
        return Err(PreviewGrantError::Invalid(format!("{field} is empty")));
    }
    if value.len() > MAX_IDENTIFIER_BYTES {
        return Err(PreviewGrantError::Invalid(format!(
            "{field} exceeds {MAX_IDENTIFIER_BYTES} bytes"
        )));
    }
    Ok(())
}

fn validate_claims(claims: &PreviewGrantClaims) -> Result<(), PreviewGrantError> {
    let invalid = |why: &str| PreviewGrantError::Invalid(why.to_owned());
    if claims.schema != PREVIEW_GRANT_SCHEMA {
        return Err(invalid("unsupported schema"));
    }
    validate_identifier(&claims.target.driver, "target.driver")?;
    validate_identifier(&claims.target.instance_id, "target.instanceId")?;
    validate_identifier(&claims.target.session_id, "target.sessionId")?;
    if claims.target.generation == 0 || claims.target.generation > MAX_SAFE_GENERATION {
        return Err(invalid("target.generation must be a positive safe integer"));
    }
    validate_identifier(&claims.execution_id, "executionId")?;
    if !is_lower_hex(&claims.issuer, 64) {
        return Err(invalid("issuer must be 64 lowercase hex"));
    }
    let audience_key = claims
        .audience
        .strip_prefix(PREVIEW_GRANT_AUDIENCE_PREFIX)
        .ok_or_else(|| invalid("audience must name a desktop broker"))?;
    if !is_lower_hex(audience_key, 64) {
        return Err(invalid("audience pubkey must be 64 lowercase hex"));
    }
    if !is_lower_hex(&claims.nonce, PREVIEW_GRANT_NONCE_HEX_LEN) {
        return Err(invalid("nonce must be 32 lowercase hex"));
    }
    if claims.expires_at <= claims.issued_at
        || claims.expires_at - claims.issued_at > PREVIEW_GRANT_MAX_TTL_SECS
    {
        return Err(invalid("lifetime must be positive and within the maximum"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "preview_grant_tests.rs"]
mod tests;
