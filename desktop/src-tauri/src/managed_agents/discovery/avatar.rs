//! The Beekeeper Agent avatar, bundled rather than fetched: a 128px JPEG of
//! `crates/buzz-agent/sprout-agent.png` (`buzz-agent-avatar.jpg`), inlined as a
//! base64 data URL. It becomes the agent's kind:0 `picture`, so it must render
//! in any client without reaching a host we do not control, and it is kept
//! small (~10 KB) because it travels inside that event.

pub(super) const BUZZ_AGENT_AVATAR_URL: &str =
    include_str!("buzz-agent-avatar.data-url").trim_ascii();

#[cfg(test)]
mod tests {
    use super::BUZZ_AGENT_AVATAR_URL;

    /// It must be a data URL of exactly the JPEG committed beside it, so the
    /// two cannot drift apart.
    #[test]
    fn buzz_agent_avatar_is_the_bundled_jpeg() {
        use base64::Engine as _;

        let payload = BUZZ_AGENT_AVATAR_URL
            .strip_prefix("data:image/jpeg;base64,")
            .expect("bundled avatar must be a base64 JPEG data URL");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .expect("bundled avatar payload must be valid base64");
        assert_eq!(decoded, include_bytes!("buzz-agent-avatar.jpg"));
        assert!(
            BUZZ_AGENT_AVATAR_URL.len() < 16 * 1024,
            "the avatar travels in a kind:0 event; keep it small"
        );
    }
}
