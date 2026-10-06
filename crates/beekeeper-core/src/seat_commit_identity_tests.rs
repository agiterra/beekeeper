//! What a seat's commit identity must be, and what it must never be.

use super::{beekeeper_local_email, seat_commit_identity, BEEKEEPER_LOCAL_DOMAIN};

const SEAT: &str = "3d3b71690f2e4f5c8a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f6071";

#[test]
fn the_email_is_the_key_not_the_person() {
    assert_eq!(beekeeper_local_email(SEAT), "3d3b7169@beekeeper.local");
    // The same eight characters the app's own commits already use, so one key
    // has one author across every repository this machine writes.
    assert!(beekeeper_local_email(SEAT).ends_with(BEEKEEPER_LOCAL_DOMAIN));
}

#[test]
fn a_short_or_padded_key_still_yields_an_address_rather_than_a_panic() {
    assert_eq!(beekeeper_local_email("abc"), "abc@beekeeper.local");
    assert_eq!(beekeeper_local_email(""), "@beekeeper.local");
    assert_eq!(
        beekeeper_local_email("  3d3b7169ff  "),
        "3d3b7169@beekeeper.local"
    );
}

#[test]
fn the_name_places_the_seat_in_its_project() {
    let identity = seat_commit_identity(SEAT, "builder", Some("kettle-control")).expect("derived");
    assert_eq!(identity.name, "builder · kettle-control");
    assert_eq!(identity.email, "3d3b7169@beekeeper.local");
}

#[test]
fn a_seat_with_no_nameable_project_is_still_named() {
    let identity = seat_commit_identity(SEAT, "verifier", None).expect("derived");
    assert_eq!(identity.name, "verifier seat");
    let blank = seat_commit_identity(SEAT, "verifier", Some("   ")).expect("derived");
    assert_eq!(blank.name, "verifier seat");
}

/// One key, one author name: a display-cased role must not produce a second
/// spelling of the same seat in `git log`.
#[test]
fn a_display_cased_role_is_normalised() {
    let identity = seat_commit_identity(SEAT, "Refuter", Some("kettle-control")).expect("derived");
    assert_eq!(identity.name, "refuter · kettle-control");
}

#[test]
fn a_seat_with_no_nameable_role_is_still_named() {
    let identity = seat_commit_identity(SEAT, "  ", Some("kettle-control")).expect("derived");
    assert_eq!(identity.name, "agent · kettle-control");
    // Never empty: an empty `user.name` is the state that produced the defect.
    assert!(!identity.name.trim().is_empty());
}

/// A name reaches `git config` as one argv value. A newline or a control
/// character in it would be written into the config file verbatim, which is
/// how a role word becomes a second config line.
#[test]
fn a_role_or_project_carrying_newlines_is_collapsed_to_one_line() {
    let identity = seat_commit_identity(
        SEAT,
        "builder\n[core]\n\tbare = true",
        Some("kettle\r\ncontrol\u{7}"),
    )
    .expect("derived");
    assert!(
        !identity.name.contains(['\n', '\r', '\u{7}']),
        "{:?} carries a control character",
        identity.name
    );
    assert_eq!(identity.name, "builder [core] bare = true · kettle control");
}

/// A seat whose key cannot be named has no author to derive. Guessing one
/// attributes its commits to nothing, which is worse than refusing.
#[test]
fn a_key_that_is_not_64_lowercase_hex_is_refused() {
    for bad in [
        "",
        "3d3b7169",
        &SEAT.to_uppercase(),
        &format!("{SEAT}0"),
        &"z".repeat(64),
    ] {
        let error = seat_commit_identity(bad, "builder", Some("kettle-control"))
            .expect_err("a malformed key must be refused");
        assert!(error.contains("64 lowercase hex"), "{error}");
    }
}

/// The operator's own address is a person's identity; a seat committing under
/// it is a forged attribution. Nothing this function returns can be one.
#[test]
fn no_derived_identity_can_be_a_persons_address() {
    let identity = seat_commit_identity(SEAT, "builder", Some("kettle-control")).expect("derived");
    assert!(identity.email.ends_with("@beekeeper.local"));
    assert_eq!(identity.email, beekeeper_local_email(SEAT));
}
