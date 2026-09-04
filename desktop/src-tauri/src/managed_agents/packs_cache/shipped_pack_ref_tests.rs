//! Recognising an installed pack that is one of the packs this build ships.
//!
//! Split out of `packs_cache.rs` for the repository file-size ratchet; the
//! code under test lives in the parent module.

use super::*;

#[test]
fn an_installed_pack_that_is_the_shipped_pack_is_named_as_one() {
    // The `packRef` half of finding 53, measured on run 5: nine role
    // agents on that machine, every `persona_team_dir` pointing at
    // `<checkout>/personas/roles/<role>` — which is what
    // `shipped_packs_dir()` answers on a development build. `plan_seat_pack`
    // took the installed arm, published `packRef: null`, and the seat chip
    // read "no pack staged" over a pack that was staged and that the app's
    // own version pins.
    let tmp = tempfile::tempdir().unwrap();
    let shipped = tmp.path().join("personas/roles");
    std::fs::create_dir_all(shipped.join("builder")).unwrap();

    let pack_ref = shipped_pack_ref_for_dir(
        Some(&shipped),
        &shipped.join("builder"),
        "builder",
        "0.5.16",
    )
    .expect("the shipped pack is named as the shipped pack");

    assert_eq!(pack_ref.repo, PACK_REF_SHIPPED_REPO);
    assert_eq!(pack_ref.sha, "0.5.16");
    assert_eq!(pack_ref.role, "builder");
    assert_eq!(pack_ref.path, "personas/roles/builder");
}

#[test]
fn a_pack_from_anywhere_else_still_has_nothing_vouching_for_it() {
    // Recognition, not a guess: the answer is `Some` only for
    // `<shipped>/<role>` itself. A pack the operator installed from their
    // own folder is named by nothing, and saying otherwise would put a
    // repository on the wire that the bytes did not come from.
    let tmp = tempfile::tempdir().unwrap();
    let shipped = tmp.path().join("personas/roles");
    std::fs::create_dir_all(shipped.join("builder")).unwrap();
    let elsewhere = tmp.path().join("my-packs/builder");
    std::fs::create_dir_all(&elsewhere).unwrap();

    assert!(shipped_pack_ref_for_dir(Some(&shipped), &elsewhere, "builder", "0.5.16").is_none());
    // The right directory under the wrong role is not this role's pack.
    assert!(shipped_pack_ref_for_dir(
        Some(&shipped),
        &shipped.join("builder"),
        "architect",
        "0.5.16"
    )
    .is_none());
    // A build that ships no packs vouches for nothing.
    assert!(
        shipped_pack_ref_for_dir(None, &shipped.join("builder"), "builder", "0.5.16").is_none()
    );
    // A seat with no role names no pack.
    assert!(
        shipped_pack_ref_for_dir(Some(&shipped), &shipped.join("builder"), "  ", "0.5.16")
            .is_none()
    );
}

#[test]
fn the_same_directory_by_another_route_is_the_same_directory() {
    // The installer records the folder the operator picked. A route
    // through `..` or a symlink names the same bytes, and a seat staged
    // from it is staged from the shipped pack.
    let tmp = tempfile::tempdir().unwrap();
    let shipped = tmp.path().join("personas/roles");
    std::fs::create_dir_all(shipped.join("builder")).unwrap();
    let roundabout = tmp.path().join("personas/roles/../roles/builder");

    assert!(shipped_pack_ref_for_dir(Some(&shipped), &roundabout, "builder", "0.5.16").is_some());
}
