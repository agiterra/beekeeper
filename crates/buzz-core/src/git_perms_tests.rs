//! Unit tests for [`super`] — split out of `git_perms.rs` so neither file
//! passes 1,000 lines. A child module (`#[path]`), so `use super::*` reads the
//! parent exactly as the inline `mod tests` it replaces did.

use super::*;

// ── project role → git role ──────────────────────────────────────────

#[test]
fn project_owner_pushes_as_owner_collaborator_as_member_viewer_never() {
    assert_eq!(
        git_role_for_project_role(ProjectRole::Owner),
        Some(MemberRole::Owner)
    );
    assert_eq!(
        git_role_for_project_role(ProjectRole::Collaborator),
        Some(MemberRole::Member)
    );
    // A viewer is read-only across the project. Mapping them to Guest
    // would look harmless but Guest is a *grant*, and it would satisfy
    // any `buzz-protect` rule written as `push:guest`.
    assert_eq!(git_role_for_project_role(ProjectRole::Viewer), None);
}

// ── additive grants: the maximum wins ────────────────────────────────

#[test]
fn max_git_role_never_demotes_either_path() {
    // Channel Admin + project Collaborator: admin survives.
    assert_eq!(
        max_git_role(MemberRole::Admin, MemberRole::Member),
        MemberRole::Admin
    );
    // Project Owner + channel Guest: owner survives, either order.
    assert_eq!(
        max_git_role(MemberRole::Guest, MemberRole::Owner),
        MemberRole::Owner
    );
    assert_eq!(
        max_git_role(MemberRole::Owner, MemberRole::Guest),
        MemberRole::Owner
    );
    // Equal roles are idempotent.
    assert_eq!(
        max_git_role(MemberRole::Member, MemberRole::Member),
        MemberRole::Member
    );
}

#[test]
fn un_normalized_bot_loses_to_every_real_role() {
    // Bot is outside the hierarchy. Callers promote it to Member before
    // ranking; if one forgets, the fail-closed direction is that Bot
    // never beats a real grant.
    for other in [
        MemberRole::Guest,
        MemberRole::Member,
        MemberRole::Admin,
        MemberRole::Owner,
    ] {
        assert_eq!(max_git_role(MemberRole::Bot, other), other);
        assert_eq!(max_git_role(other, MemberRole::Bot), other);
    }
}

#[test]
fn pattern_parse_valid() {
    let p = RefPattern::parse("refs/heads/main").unwrap();
    assert_eq!(p.segments.len(), 3);
    assert!(p.matches("refs/heads/main"));
    assert!(!p.matches("refs/heads/develop"));
}

#[test]
fn pattern_wildcard_matches_one_segment() {
    let p = RefPattern::parse("refs/heads/*").unwrap();
    assert!(p.matches("refs/heads/main"));
    assert!(p.matches("refs/heads/feature"));
    assert!(!p.matches("refs/heads/feature/sub"));
    assert!(!p.matches("refs/tags/v1"));
}

#[test]
fn pattern_multi_wildcard() {
    let p = RefPattern::parse("refs/*/release/*").unwrap();
    assert!(p.matches("refs/heads/release/v1"));
    assert!(!p.matches("refs/heads/release/v1/hotfix"));
}

#[test]
fn pattern_rejects_partial_glob() {
    assert!(matches!(
        RefPattern::parse("refs/tags/v*"),
        Err(PatternError::InvalidSegment(_))
    ));
}

#[test]
fn pattern_rejects_missing_refs_prefix() {
    assert!(matches!(
        RefPattern::parse("heads/main"),
        Err(PatternError::MissingRefsPrefix)
    ));
}

#[test]
fn pattern_rejects_empty() {
    assert!(matches!(RefPattern::parse(""), Err(PatternError::Empty)));
}

#[test]
fn pattern_rejects_too_many_wildcards() {
    assert!(matches!(
        RefPattern::parse("refs/*/*/*/*"),
        Err(PatternError::TooManyWildcards)
    ));
}

#[test]
fn pattern_recursive_wildcard_matches_nested() {
    let p = RefPattern::parse("refs/heads/**").unwrap();
    assert!(p.matches("refs/heads/main"));
    assert!(p.matches("refs/heads/feature/foo"));
    assert!(p.matches("refs/heads/feature/foo/bar"));
    assert!(!p.matches("refs/tags/v1"));
}

#[test]
fn pattern_recursive_wildcard_must_be_last() {
    assert!(matches!(
        RefPattern::parse("refs/**/heads"),
        Err(PatternError::InvalidSegment(_))
    ));
}

#[test]
fn pattern_recursive_requires_at_least_one_segment() {
    let p = RefPattern::parse("refs/heads/**").unwrap();
    // Must match at least one segment after prefix
    assert!(!p.matches("refs/heads"));
}

#[test]
fn classify_create() {
    let zero = "0000000000000000000000000000000000000000";
    assert_eq!(
        UpdateKind::classify(zero, "abc123abc123abc123abc123abc123abc123abcd", false),
        UpdateKind::Create
    );
}

#[test]
fn classify_delete() {
    let zero = "0000000000000000000000000000000000000000";
    assert_eq!(
        UpdateKind::classify("abc123abc123abc123abc123abc123abc123abcd", zero, false),
        UpdateKind::Delete
    );
}

#[test]
fn classify_fast_forward() {
    assert_eq!(
        UpdateKind::classify("aaa", "bbb", true),
        UpdateKind::FastForward
    );
}

#[test]
fn classify_non_fast_forward() {
    assert_eq!(
        UpdateKind::classify("aaa", "bbb", false),
        UpdateKind::NonFastForward
    );
}

#[test]
fn parse_protection_tag_basic() {
    let rule = parse_protection_tag(&["refs/heads/main", "push:admin", "no-force-push"]).unwrap();
    assert_eq!(rule.push_role, Some(MemberRole::Admin));
    assert!(rule.no_force_push);
    assert!(!rule.no_delete);
    assert!(!rule.require_patch);
}

#[test]
fn parse_protection_tag_all_rules() {
    let rule = parse_protection_tag(&[
        "refs/heads/main",
        "push:owner",
        "no-force-push",
        "no-delete",
        "require-patch",
    ])
    .unwrap();
    assert_eq!(rule.push_role, Some(MemberRole::Owner));
    assert!(rule.no_force_push);
    assert!(rule.no_delete);
    assert!(rule.require_patch);
}

#[test]
fn parse_protection_tag_unknown_rule_skipped() {
    // Forward-compatibility: unknown rules are silently skipped.
    let rule = parse_protection_tag(&["refs/heads/main", "yolo", "no-force-push"]).unwrap();
    // "yolo" was skipped, but "no-force-push" was still applied.
    assert!(rule.no_force_push);
    assert!(rule.push_role.is_none());
}

#[test]
fn parse_protection_tag_invalid_role() {
    assert!(matches!(
        parse_protection_tag(&["refs/heads/main", "push:superadmin"]),
        Err(RuleParseError::InvalidRole(_))
    ));
}

#[test]
fn parse_protection_tag_rejects_push_bot_and_guest() {
    // push:bot and push:guest are rejected — they're almost certainly user errors.
    assert!(matches!(
        parse_protection_tag(&["refs/heads/main", "push:bot"]),
        Err(RuleParseError::InvalidRole(_))
    ));
    assert!(matches!(
        parse_protection_tag(&["refs/heads/main", "push:guest"]),
        Err(RuleParseError::InvalidRole(_))
    ));
}

#[test]
fn effective_rules_union_strictest_role() {
    let rules = vec![
        parse_protection_tag(&["refs/heads/*", "push:member", "no-force-push"]).unwrap(),
        parse_protection_tag(&["refs/heads/main", "push:admin"]).unwrap(),
    ];
    let eff = EffectiveRules::for_ref("refs/heads/main", &rules);
    assert_eq!(eff.push_role, Some(MemberRole::Admin)); // strictest
    assert!(eff.no_force_push); // from the wildcard rule
    assert!(eff.has_explicit_match);
}

#[test]
fn effective_rules_no_match_uses_defaults() {
    let rules = vec![parse_protection_tag(&["refs/heads/main", "push:admin"]).unwrap()];
    let eff = EffectiveRules::for_ref("refs/heads/develop", &rules);
    assert!(!eff.has_explicit_match);
}

#[test]
fn evaluate_owner_passes_push_role() {
    let rules = vec![parse_protection_tag(&["refs/heads/main", "push:admin"]).unwrap()];
    let update = RefUpdate {
        ref_name: "refs/heads/main".to_string(),
        kind: UpdateKind::FastForward,
        old_oid: "a".repeat(40),
        new_oid: "b".repeat(40),
    };
    assert!(evaluate_ref_update(&update, MemberRole::Owner, &rules).is_ok());
}

#[test]
fn evaluate_member_denied_push_admin() {
    let rules = vec![parse_protection_tag(&["refs/heads/main", "push:admin"]).unwrap()];
    let update = RefUpdate {
        ref_name: "refs/heads/main".to_string(),
        kind: UpdateKind::FastForward,
        old_oid: "a".repeat(40),
        new_oid: "b".repeat(40),
    };
    assert!(evaluate_ref_update(&update, MemberRole::Member, &rules).is_err());
}

#[test]
fn evaluate_no_force_push_blocks_owner() {
    let rules =
        vec![parse_protection_tag(&["refs/heads/main", "push:member", "no-force-push"]).unwrap()];
    let update = RefUpdate {
        ref_name: "refs/heads/main".to_string(),
        kind: UpdateKind::NonFastForward,
        old_oid: "a".repeat(40),
        new_oid: "b".repeat(40),
    };
    // Owner is blocked by no-force-push!
    assert!(evaluate_ref_update(&update, MemberRole::Owner, &rules).is_err());
}

#[test]
fn evaluate_no_force_push_allows_fast_forward() {
    let rules =
        vec![parse_protection_tag(&["refs/heads/main", "push:member", "no-force-push"]).unwrap()];
    let update = RefUpdate {
        ref_name: "refs/heads/main".to_string(),
        kind: UpdateKind::FastForward,
        old_oid: "a".repeat(40),
        new_oid: "b".repeat(40),
    };
    // no-force-push should NOT block fast-forward pushes.
    assert!(evaluate_ref_update(&update, MemberRole::Member, &rules).is_ok());
}

#[test]
fn evaluate_no_delete_blocks_admin() {
    let rules =
        vec![parse_protection_tag(&["refs/heads/main", "push:member", "no-delete"]).unwrap()];
    let update = RefUpdate {
        ref_name: "refs/heads/main".to_string(),
        kind: UpdateKind::Delete,
        old_oid: "a".repeat(40),
        new_oid: "0".repeat(40),
    };
    assert!(evaluate_ref_update(&update, MemberRole::Admin, &rules).is_err());
}

#[test]
fn evaluate_require_patch_blocks_all() {
    let rules = vec![parse_protection_tag(&["refs/heads/main", "require-patch"]).unwrap()];
    let update = RefUpdate {
        ref_name: "refs/heads/main".to_string(),
        kind: UpdateKind::FastForward,
        old_oid: "a".repeat(40),
        new_oid: "b".repeat(40),
    };
    assert!(evaluate_ref_update(&update, MemberRole::Owner, &rules).is_err());
}

#[test]
fn evaluate_defaults_member_can_ff_branch() {
    let rules = vec![]; // No explicit rules
    let update = RefUpdate {
        ref_name: "refs/heads/feature".to_string(),
        kind: UpdateKind::FastForward,
        old_oid: "a".repeat(40),
        new_oid: "b".repeat(40),
    };
    assert!(evaluate_ref_update(&update, MemberRole::Member, &rules).is_ok());
}

#[test]
fn evaluate_defaults_member_cannot_force_push() {
    let rules = vec![]; // No explicit rules — defaults apply
    let update = RefUpdate {
        ref_name: "refs/heads/feature".to_string(),
        kind: UpdateKind::NonFastForward,
        old_oid: "a".repeat(40),
        new_oid: "b".repeat(40),
    };
    // Default: non-fast-forward requires Admin
    assert!(evaluate_ref_update(&update, MemberRole::Member, &rules).is_err());
}

#[test]
fn evaluate_defaults_guest_cannot_push() {
    let rules = vec![];
    let update = RefUpdate {
        ref_name: "refs/heads/feature".to_string(),
        kind: UpdateKind::FastForward,
        old_oid: "a".repeat(40),
        new_oid: "b".repeat(40),
    };
    assert!(evaluate_ref_update(&update, MemberRole::Guest, &rules).is_err());
}

#[test]
fn evaluate_bot_cannot_push_without_explicit_grant() {
    let rules = vec![];
    let update = RefUpdate {
        ref_name: "refs/heads/feature".to_string(),
        kind: UpdateKind::FastForward,
        old_oid: "a".repeat(40),
        new_oid: "b".repeat(40),
    };
    // Bot has permission_level 0 at the core evaluator level.
    // NOTE: The policy layer (policy.rs) promotes Bot → Member before calling
    // evaluate_push, so bots in a channel CAN push in practice. This test
    // verifies the raw evaluator behavior; the promotion is tested in policy.
    assert!(evaluate_ref_update(&update, MemberRole::Bot, &rules).is_err());
}

#[test]
fn evaluate_guest_denied_even_with_only_no_force_push_rule() {
    // Regression test: a rule that only sets no-force-push (no push:role)
    // should NOT let a Guest bypass the built-in default (Member required).
    let rules = vec![parse_protection_tag(&["refs/heads/main", "no-force-push"]).unwrap()];
    let update = RefUpdate {
        ref_name: "refs/heads/main".to_string(),
        kind: UpdateKind::FastForward,
        old_oid: "a".repeat(40),
        new_oid: "b".repeat(40),
    };
    // Guest should be denied — built-in default requires Member for FF push.
    assert!(evaluate_ref_update(&update, MemberRole::Guest, &rules).is_err());
    // Member should be allowed (meets default requirement).
    assert!(evaluate_ref_update(&update, MemberRole::Member, &rules).is_ok());
}

#[test]
fn evaluate_push_member_cannot_weaken_destructive_defaults() {
    // push:member should NOT allow Members to force-push or delete.
    // The built-in default for NFF/Delete is Admin — explicit push:member
    // can't weaken that for destructive operations.
    let rules = vec![parse_protection_tag(&["refs/heads/main", "push:member"]).unwrap()];
    // Member can FF push (non-destructive, explicit overrides default).
    let ff_update = RefUpdate {
        ref_name: "refs/heads/main".to_string(),
        kind: UpdateKind::FastForward,
        old_oid: "a".repeat(40),
        new_oid: "b".repeat(40),
    };
    assert!(evaluate_ref_update(&ff_update, MemberRole::Member, &rules).is_ok());

    // Member CANNOT force-push (destructive, default Admin still enforced).
    let nff_update = RefUpdate {
        ref_name: "refs/heads/main".to_string(),
        kind: UpdateKind::NonFastForward,
        old_oid: "a".repeat(40),
        new_oid: "b".repeat(40),
    };
    assert!(evaluate_ref_update(&nff_update, MemberRole::Member, &rules).is_err());

    // Admin CAN force-push (meets the default Admin requirement).
    assert!(evaluate_ref_update(&nff_update, MemberRole::Admin, &rules).is_ok());

    // Member CANNOT delete (destructive, default Admin still enforced).
    let del_update = RefUpdate {
        ref_name: "refs/heads/main".to_string(),
        kind: UpdateKind::Delete,
        old_oid: "a".repeat(40),
        new_oid: "0".repeat(40),
    };
    assert!(evaluate_ref_update(&del_update, MemberRole::Member, &rules).is_err());
}

#[test]
fn evaluate_push_multiple_refs_partial_deny() {
    let rules = vec![parse_protection_tag(&["refs/heads/main", "push:admin"]).unwrap()];
    let updates = vec![
        RefUpdate {
            ref_name: "refs/heads/feature".to_string(),
            kind: UpdateKind::FastForward,
            old_oid: "a".repeat(40),
            new_oid: "b".repeat(40),
        },
        RefUpdate {
            ref_name: "refs/heads/main".to_string(),
            kind: UpdateKind::FastForward,
            old_oid: "c".repeat(40),
            new_oid: "d".repeat(40),
        },
    ];
    // Member can push to feature but not main
    let result = evaluate_push(&updates, MemberRole::Member, &rules);
    assert!(result.is_err());
    let denials = result.unwrap_err();
    assert_eq!(denials.len(), 1);
    assert_eq!(denials[0].ref_name, "refs/heads/main");
}

// ── require-verdict (batch 3 lane L6) ────────────────────────────────────

/// The token parses into its own flag rather than falling into
/// `unknown_rules`, which is where a relay predating this lane leaves it.
#[test]
fn require_verdict_parses_into_its_own_flag() {
    let (rule, unknown) =
        parse_protection_tag_with_warnings(&["refs/heads/main", "require-verdict"])
            .expect("tag parses");
    assert!(rule.require_verdict, "require-verdict sets its own flag");
    assert!(
        unknown.is_empty(),
        "require-verdict is a known rule now, not a forward-compat unknown: {unknown:?}"
    );
}

/// Unioned across matching patterns exactly as `no-force-push` is: one
/// matching rule setting it is enough.
#[test]
fn require_verdict_unions_across_matching_patterns() {
    let rules = vec![
        parse_protection_tag(&["refs/heads/*", "no-force-push"]).expect("wildcard rule"),
        parse_protection_tag(&["refs/heads/main", "require-verdict"]).expect("exact rule"),
    ];
    let main = EffectiveRules::for_ref("refs/heads/main", &rules);
    assert!(main.require_verdict, "the exact rule's flag is unioned in");
    assert!(main.no_force_push, "the wildcard rule still applies");
    let topic = EffectiveRules::for_ref("refs/heads/topic", &rules);
    assert!(
        !topic.require_verdict,
        "a ref no require-verdict pattern matches is ungoverned"
    );
}

/// The flag is enforced by the relay, never by this pure evaluator: the
/// search it implies needs storage, so `evaluate_ref_update` must keep
/// returning `Ok` and leave the decision to `hook_policy_check`.
#[test]
fn evaluate_ref_update_never_enforces_require_verdict() {
    let rules = vec![parse_protection_tag(&["refs/heads/main", "require-verdict"]).expect("rule")];
    let update = RefUpdate {
        ref_name: "refs/heads/main".to_string(),
        kind: UpdateKind::FastForward,
        old_oid: "1".repeat(40),
        new_oid: "2".repeat(40),
    };
    assert!(
        evaluate_ref_update(&update, MemberRole::Member, &rules).is_ok(),
        "the pure evaluator stays pure; the relay runs the verdict search"
    );
}
