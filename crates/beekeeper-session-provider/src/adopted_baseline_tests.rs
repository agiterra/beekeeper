//! The real shape: 23 prior records, 3 open, 13 fenced before the adoption.
use crate::managed_enrollment::has_current_stop_fence;
use crate::state::SessionRecord;
use crate::Provider;
use nostr::Keys;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use uuid::Uuid;

const ADOPTED_AT: &str = "2026-11-03T23:48:04+00:00";

fn at(offset: i64) -> SystemTime {
    let base: SystemTime = chrono::DateTime::parse_from_rfc3339(ADOPTED_AT)
        .expect("fixture time")
        .into();
    if offset < 0 {
        base - Duration::from_secs(offset.unsigned_abs())
    } else {
        base + Duration::from_secs(offset.unsigned_abs())
    }
}

fn record(session_id: &str, actor: &str, closed: bool) -> SessionRecord {
    serde_json::from_value(json!({
        "sessionId": session_id, "generation": 1, "channelId": Uuid::new_v4(),
        "commandId": format!("c-{session_id}"), "generationCommandId": format!("c-{session_id}"),
        "providerInstanceRef": "claude-primary", "runtime": "claude", "driver": "claude",
        "cwd": "/fixture", "actor": actor, "founderPubkey": actor, "grantedOperators": [],
        "grantedViewers": [], "nativeHistoryStarted": false, "authoritySeq": 0, "createdAtMs": 1,
        "nextSeq": 1, "nextLeaseSequence": 1, "closed": closed,
    }))
    .expect("the record's real shape")
}

struct World {
    _dir: tempfile::TempDir,
    provider: Provider,
    actor: String,
    owner: Keys,
    prior: Vec<String>,
}

impl World {
    fn state_dir(&self) -> PathBuf {
        self.provider.config.state_dir.clone()
    }
    fn fence_path(&self, session: &str) -> PathBuf {
        self.state_dir()
            .join(format!("stop-quiescence-{session}.json"))
    }
    fn fence(&self, session: &str, written: SystemTime) {
        std::fs::write(
            self.fence_path(session),
            b"{\"commandId\":\"settled-umbrella\"}",
        )
        .expect("fence");
        std::fs::File::options()
            .write(true)
            .open(self.fence_path(session))
            .expect("fence")
            .set_modified(written)
            .expect("mtime");
    }
    fn adoption(&self, value: &Value) {
        std::fs::write(
            self.state_dir()
                .join("actors")
                .join(&self.actor)
                .join("adoption.json"),
            serde_json::to_vec(value).expect("json"),
        )
        .expect("adoption");
    }
    fn good_adoption(&self) -> Value {
        json!({"schema": "beekeeper-adopted-identity/v1", "actor": self.actor,
            "owner": self.owner.public_key().to_hex(), "priorSessions": self.prior,
            "adoptedAt": ADOPTED_AT})
    }
    /// Both actor-wide readers must agree; one saying free is the hole.
    fn refuses(&self) -> bool {
        let scan = has_current_stop_fence(&self.provider.config, &self.provider.state, &self.actor);
        let unproved = self.provider.actor_has_unproved_stop(Some(&self.actor));
        assert_eq!(scan, unproved, "the two scans must read one boundary");
        scan
    }
}

fn world() -> World {
    let dir = tempfile::tempdir().expect("fixture");
    let mut provider = crate::tests::provider(&dir.path().join("state"), None);
    let (agent, owner) = (Keys::generate(), Keys::generate());
    let actor = agent.public_key().to_hex();
    let actor_dir = provider.config.state_dir.join("actors").join(&actor);
    std::fs::create_dir_all(&actor_dir).expect("custody");
    std::fs::write(
        actor_dir.join("auth-tag.json"),
        beekeeper_sdk::nip_oa::compute_auth_tag(&owner, &agent.public_key(), "").expect("tag"),
    )
    .expect("tag");
    let prior: Vec<String> = (0..23).map(|_| Uuid::new_v4().to_string()).collect();
    for (index, id) in prior.iter().enumerate() {
        provider
            .state
            .insert_session(record(id, &actor, index >= 3))
            .expect("record");
    }
    let held = World {
        _dir: dir,
        provider,
        actor,
        owner,
        prior,
    };
    for id in &held.prior[3..16] {
        held.fence(id, at(-20 * 86_400));
    }
    held.adoption(&held.good_adoption());
    held
}

#[test]
fn the_fences_an_adoption_found_do_not_stop_its_new_work_and_stay_in_place() {
    let held = world();
    let digest = |w: &World| -> Vec<(PathBuf, Vec<u8>, SystemTime)> {
        w.prior[3..16]
            .iter()
            .map(|id| {
                let path = w.fence_path(id);
                let bytes = std::fs::read(&path).expect("fence stays");
                let modified = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .expect("mtime");
                (path, bytes, modified)
            })
            .collect()
    };
    let before = digest(&held);
    assert!(!held.refuses());
    assert_eq!(before, digest(&held), "reading changes no fence");
}

#[test]
fn a_fence_on_the_new_session_still_refuses() {
    let mut held = world();
    let new = Uuid::new_v4().to_string();
    let row = record(&new, &held.actor, true);
    held.provider.state.insert_session(row).expect("record");
    assert!(!held.refuses());
    held.fence(&new, at(60));
    assert!(held.refuses());
}

#[test]
fn a_fence_on_a_named_record_written_after_the_adoption_refuses() {
    let held = world();
    held.fence(&held.prior[3], at(60));
    assert!(held.refuses());
}

#[test]
fn an_open_named_record_with_a_fence_refuses() {
    let held = world();
    held.fence(&held.prior[0], at(-60));
    assert!(held.refuses());
}

#[test]
fn a_named_record_this_host_has_run_counts_again() {
    let mut held = world();
    let id = held.prior[3].clone();
    let target = crate::CodingSessionTarget {
        driver: "claude".into(),
        instance_id: held.provider.config.instance_id.clone(),
        session_id: id.clone(),
        generation: 1,
    };
    held.provider
        .state
        .update_session(&id, |row| {
            row.process_witness = Some(crate::process_witness::ExecutionProcessWitness {
                target,
                attempted_target: None,
                group_id: None,
                quiesced: false,
            })
        })
        .expect("witness");
    assert!(held.refuses());
}

#[test]
fn a_missing_baseline_refuses() {
    let held = world();
    std::fs::remove_file(
        held.state_dir()
            .join("actors")
            .join(&held.actor)
            .join("adoption.json"),
    )
    .expect("removed");
    assert!(held.refuses());
}

#[test]
fn a_malformed_baseline_refuses() {
    let held = world();
    std::fs::write(
        held.state_dir()
            .join("actors")
            .join(&held.actor)
            .join("adoption.json"),
        b"{not json",
    )
    .expect("written");
    assert!(held.refuses());
}

#[test]
fn a_baseline_naming_another_actor_refuses() {
    let held = world();
    let mut value = held.good_adoption();
    value["actor"] = json!(Keys::generate().public_key().to_hex());
    held.adoption(&value);
    assert!(held.refuses());
}

#[test]
fn a_baseline_naming_another_owner_refuses() {
    let held = world();
    let mut value = held.good_adoption();
    value["owner"] = json!(Keys::generate().public_key().to_hex());
    held.adoption(&value);
    assert!(held.refuses());
}

#[test]
fn a_baseline_with_no_time_refuses() {
    let held = world();
    let mut value = held.good_adoption();
    value.as_object_mut().expect("object").remove("adoptedAt");
    held.adoption(&value);
    assert!(held.refuses());
}

#[test]
fn an_attestation_that_will_not_verify_refuses() {
    let held = world();
    std::fs::write(
        held.state_dir()
            .join("actors")
            .join(&held.actor)
            .join("auth-tag.json"),
        b"not an attestation",
    )
    .expect("written");
    assert!(held.refuses());
}

#[test]
fn a_fence_that_is_a_symlink_refuses() {
    let held = world();
    let elsewhere = held.state_dir().join("elsewhere.json");
    std::fs::write(&elsewhere, b"{}").expect("target");
    let path = held.fence_path(&held.prior[3]);
    std::fs::remove_file(&path).expect("removed");
    std::os::unix::fs::symlink(&elsewhere, &path).expect("link");
    assert!(held.refuses());
}

#[test]
fn one_actors_baseline_excuses_no_other_actors_record() {
    let mut held = world();
    let other = Keys::generate().public_key().to_hex();
    let shared = held.prior[20].clone();
    let row = record(&format!("{shared}-other"), &other, true);
    held.provider.state.insert_session(row).expect("record");
    held.fence(&format!("{shared}-other"), at(-60));
    assert!(has_current_stop_fence(
        &held.provider.config,
        &held.provider.state,
        &other
    ));
    assert!(held.provider.actor_has_unproved_stop(Some(&other)));
    assert!(
        !held.refuses(),
        "the adopted actor's own scan is unaffected"
    );
}

#[test]
fn no_actor_reads_no_baseline() {
    let held = world();
    assert!(!held.provider.actor_has_unproved_stop(None));
}
