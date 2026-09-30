//! Readiness facts about the **agent host**, split from
//! `team_readiness_tests.rs` so that file stays under the repository's
//! 1000-line ceiling.
//!
//! One property, and it is the load-bearing one: a host a launch cannot get an
//! answer from must block, never pass. The gate treats `unknown` facts as
//! passable, so anything softer than `Blocked` would seat a team against a
//! provider that may not exist while every surface downstream looked calm.

use super::tests::{signed_auth_tag, PROJECT_REF};
use super::*;
use std::collections::HashSet;

use crate::coding_sessions::workdir_store::CodingSessionWorkdirStore;
use crate::session_provider::runtimes::runtime_readiness_metadata;
use crate::session_provider::store::{
    CodingSessionProviderReadinessRecord, CodingSessionProviderReadinessStore,
};

/// A host that a launch cannot get an answer from must **block**, never pass.
///
/// This is the regression test for the whole comfortable-status failure mode.
/// The gate treats `unknown` facts as passable and `blocked` ones as not, so
/// an unreachable agent host reported as anything but blocked would seat a
/// team against a provider that may not exist — and every surface downstream
/// would look calm while doing it.
///
/// It also pins the *remedy* to the host's own words. "Use Prepare to start
/// it" is wrong for three of the four causes (not installed, no identity,
/// another host holding the lock), and a wrong remedy costs more than none.
#[test]
fn an_unreachable_agent_host_blocks_the_launch_and_carries_its_own_reason() {
    /// A host with a provisioned provider whose agent host says nothing.
    struct UnreachableHost {
        owner: String,
        provider_pubkey: String,
        reason: String,
    }

    impl ReadinessHost for UnreachableHost {
        fn owner_pubkey(&self) -> Result<String, String> {
            Ok(self.owner.clone())
        }
        fn workdirs(&self) -> Result<CodingSessionWorkdirStore, String> {
            Ok(CodingSessionWorkdirStore::default())
        }
        fn agents(
            &self,
            _expected_owner: Option<&str>,
        ) -> Result<Vec<ManagedAgentReadinessMetadata>, String> {
            Ok(Vec::new())
        }
        fn live_agent_pubkeys(&self) -> Result<HashSet<String>, String> {
            Ok(HashSet::new())
        }
        fn runtimes(&self) -> Vec<StrictRuntimeDiagnostic> {
            runtime_readiness_metadata()
        }
        fn provider_store(
            &self,
            _expected_owner: Option<&str>,
        ) -> Result<CodingSessionProviderReadinessStore, String> {
            let mut store = CodingSessionProviderReadinessStore::default();
            store.providers.insert(
                "wss://relay.example".to_string(),
                CodingSessionProviderReadinessRecord {
                    provider_pubkey: self.provider_pubkey.clone(),
                    instance_id: self.provider_pubkey[..16].to_string(),
                    auth_tag_present: true,
                    auth_tag_owner: Some(self.owner.clone()),
                    auth_tag_invalid: false,
                    auth_tag_owner_mismatch: false,
                    created_at: "2026-09-30T00:00:00Z".to_string(),
                    relay_url: "wss://relay.example".to_string(),
                },
            );
            Ok(store)
        }
        fn relay_url(&self) -> String {
            "wss://relay.example".to_string()
        }
        fn provider_process(&self, _pubkey: &str) -> CodingSessionProviderProcessState {
            CodingSessionProviderProcessState::HostUnreachable {
                reason: self.reason.clone(),
            }
        }
    }

    let (owner, provider_pubkey, _tag) = signed_auth_tag("");
    let reason = "the agent host is not running (nothing is listening on \
                  /home/agent/.local/state/buzz/host/host.sock)";
    let response = gather(
        &UnreachableHost {
            owner,
            provider_pubkey,
            reason: reason.to_string(),
        },
        PROJECT_REF.into(),
        None,
        None,
        &ProjectPackSourceProbe::NotProbed,
    );

    let fact = response
        .facts
        .iter()
        .find(|fact| fact.code == "PROVIDER_HOST_UNREACHABLE")
        .expect("an unreachable agent host must produce a named fact");
    assert_eq!(
        fact.state,
        TeamReadinessFactState::Blocked,
        "never Ready and never unknown: the gate lets unknowns through"
    );
    assert_eq!(
        fact.remedy.as_deref(),
        Some(reason),
        "the remedy is the host's own words, not a generic 'use Prepare'"
    );
    assert_eq!(response.provider.process, "host_unreachable");

    // And it reaches the response's own summary, which is what the launch
    // gate and the UI actually read.
    assert!(
        !response
            .unknown_codes
            .contains(&"PROVIDER_HOST_UNREACHABLE".to_string()),
        "an unreachable host is not an unknown: {:?}",
        response.unknown_codes
    );
    assert!(response
        .facts
        .iter()
        .any(|fact| fact.code == "PROVIDER_HOST_UNREACHABLE"
            && fact.state == TeamReadinessFactState::Blocked));
}
