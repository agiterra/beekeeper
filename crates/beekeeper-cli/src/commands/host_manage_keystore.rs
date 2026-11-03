//! The desktop's key store, as `bee host manage adopt-identity` reads it: one entry,
//! once, and only the one key it was asked for. Read-only. The Host daemon links
//! none of this. Errors carry no detail from the store.

use beekeeper_agent_management::adopt_identity::{AgentKeySource, KeySourceFailure};
use beekeeper_host_core::layout::Instance;
use zeroize::{Zeroize, Zeroizing};

/// The account the desktop keeps all its secrets under, as one JSON map.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const BLOB_ACCOUNT: &str = "secrets";

/// The one service this instance's desktop build uses. Nothing else is tried.
fn service_for(instance: Instance) -> &'static str {
    match instance {
        Instance::Dev => "beekeeper-desktop-dev",
        Instance::Production => "beekeeper-desktop",
    }
}

/// This instance's desktop key store.
pub(crate) struct DesktopKeyStore {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    service: &'static str,
}

impl DesktopKeyStore {
    pub(crate) fn for_instance(instance: Instance) -> Self {
        Self {
            service: service_for(instance),
        }
    }
}

/// Take exactly one actor's key out of the store's map, and clear the rest.
fn extract_agent_key(
    blob: &mut String,
    actor: &str,
) -> Result<Option<Zeroizing<String>>, KeySourceFailure> {
    let parsed = serde_json::from_str::<std::collections::HashMap<String, String>>(blob);
    blob.zeroize();
    let mut map = parsed.map_err(|_| KeySourceFailure::Malformed)?;
    let key = map.remove(&format!("agent:{actor}")).map(Zeroizing::new);
    for value in map.values_mut() {
        value.zeroize();
    }
    Ok(key)
}

impl AgentKeySource for DesktopKeyStore {
    #[cfg(target_os = "macos")]
    fn agent_key(&self, actor: &str) -> Result<Option<Zeroizing<String>>, KeySourceFailure> {
        let entry = keyring::Entry::new(self.service, BLOB_ACCOUNT)
            .map_err(|_| KeySourceFailure::Unavailable)?;
        match entry.get_password() {
            Ok(mut blob) => extract_agent_key(&mut blob, actor),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(KeySourceFailure::Unavailable),
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn agent_key(&self, _actor: &str) -> Result<Option<Zeroizing<String>>, KeySourceFailure> {
        Err(KeySourceFailure::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENTINEL: &str = "nsec1sentinelsentinelsentinel";

    #[test]
    fn only_the_named_key_comes_out_and_the_input_is_cleared() {
        let actor = "ab".repeat(32);
        let mut blob = format!(
            r#"{{"agent:{actor}":"{SENTINEL}","identity":"other-secret","agent:{}":"third"}}"#,
            "cd".repeat(32)
        );
        let key = extract_agent_key(&mut blob, &actor).unwrap().unwrap();
        assert_eq!(key.as_str(), SENTINEL);
        assert!(blob.is_empty() || !blob.contains(SENTINEL));
    }

    #[test]
    fn an_absent_key_is_none_and_never_the_human_identity() {
        let mut blob = r#"{"identity":"human-secret"}"#.to_string();
        assert!(extract_agent_key(&mut blob, &"ab".repeat(32))
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_malformed_store_says_so_and_quotes_nothing() {
        let mut blob = format!("not json {SENTINEL}");
        let error = extract_agent_key(&mut blob, &"ab".repeat(32)).unwrap_err();
        assert_eq!(error, KeySourceFailure::Malformed);
        assert!(!format!("{error:?}").contains(SENTINEL));
    }

    #[test]
    fn each_instance_has_exactly_one_service() {
        assert_eq!(service_for(Instance::Dev), "beekeeper-desktop-dev");
        assert_eq!(service_for(Instance::Production), "beekeeper-desktop");
    }
}
