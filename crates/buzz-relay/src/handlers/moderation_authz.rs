//! Community moderation authorization (Phase 1 contract).
//!
//! One capability seam for every moderation decision, per
//! `PLANS/COMMUNITY_MODERATION_PLAN.md` §0.1: roles are community
//! `owner`/`admin` (from tenant-scoped `relay_members`) plus existing
//! channel-level owner/admin. There is no Moderator tier in v1 — but all
//! authorization routes through [`authorize_moderation_action`] so adding one
//! later is a policy change, not a rewrite.
//!
//! ## Tenant invariant
//! Authority never crosses the tenant fence: the actor's role is read from
//! `relay_members` / `channel_members` under `tenant.community()` only, and
//! callers must have already resolved `target` inside the same tenant.
//!
//! Lane ownership: L2 (Mari). Signatures below are the contract.

use std::sync::Arc;

use buzz_core::channel::ChannelType;
use buzz_core::tenant::TenantContext;
use uuid::Uuid;

use crate::state::AppState;

/// A moderation capability being exercised.
///
/// V1 capability grid (plan §4 Gap A): community owner/admin hold all of
/// these community-wide; channel owner/admin hold `DeleteMessage`/`Kick`
/// within their channel only; members hold none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModerationAction {
    /// Delete any message (kind:9005 path).
    DeleteMessage,
    /// Remove/kick a user from a channel (kind:9001 path).
    Kick,
    /// Ban a user from the community (community owner/admin only).
    Ban,
    /// Lift a community ban.
    Unban,
    /// Time-box a user's writes (community owner/admin only).
    Timeout,
    /// Clear a timeout early.
    Untimeout,
    /// Resolve/dismiss/escalate reports in the moderation queue.
    ResolveReport,
    /// Read the moderation queue and audit log.
    ViewQueue,
}

/// What the action is aimed at (already tenant-resolved by the caller).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModerationTarget<'a> {
    /// An event (32-byte id) in `channel_id`'s community.
    Event(&'a [u8]),
    /// A member pubkey in this community.
    Pubkey(&'a [u8]),
    /// No specific target (queue/audit reads).
    None,
}

/// Why an authorization succeeded — recorded in the audit row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModerationAuthority {
    /// Actor is community `owner` in `relay_members`.
    CommunityOwner,
    /// Actor is community `admin` in `relay_members`.
    CommunityAdmin,
    /// Actor is channel owner/admin of the target's channel.
    ChannelRole,
}

/// Decide whether `actor` may perform `action` on `target`.
///
/// - Community `owner`/`admin` (tenant-scoped `relay_members.role`) are
///   authorized for every [`ModerationAction`] in any channel of their
///   community — this is the bridge `validate_admin_event` is missing today.
/// - Channel owner/admin keep their existing channel-local authority for
///   `DeleteMessage`/`Kick` (via `channel_id`).
/// - Guard rails (plan): an admin cannot ban/timeout the community owner or
///   a fellow admin; only the owner can action an admin.
/// - **DM carve-out**: no role grants `DeleteMessage`/`Kick` inside a
///   `channel_type = 'dm'` channel. See [`decide_authority`].
///
/// Returns the matched authority for the audit row, or `Err` with a
/// client-safe denial message.
pub async fn authorize_moderation_action(
    tenant: &TenantContext,
    state: &Arc<AppState>,
    actor_pubkey: &[u8],
    channel_id: Option<Uuid>,
    target: ModerationTarget<'_>,
    action: ModerationAction,
) -> anyhow::Result<ModerationAuthority> {
    let community = tenant.community();

    // DM carve-out, resolved before any role is read so the cheapest denial is
    // also the first one. Only the channel-local actions can land inside a DM;
    // the community-wide actions (ban/timeout/queue reads) are not channel
    // scoped, so they skip the lookup entirely. A DB fault propagates and the
    // caller denies — fail closed.
    let channel_is_dm = match (action, channel_id) {
        (ModerationAction::DeleteMessage | ModerationAction::Kick, Some(channel_id)) => {
            state
                .db
                .get_channel(community, channel_id)
                .await?
                .channel_type
                == ChannelType::Dm.as_str()
        }
        _ => false,
    };
    if channel_is_dm {
        // No role changes this outcome, so the role lookups are skipped. The
        // decision itself still comes from `decide_authority` — one policy.
        return decide_authority(None, None, None, true, action);
    }

    // Community role: `relay_members` stores pubkeys as 64-char hex, fenced to
    // `community` in the query itself. This is the primary authority — owner and
    // admin can moderate any channel in their community.
    let actor_role = state
        .db
        .get_relay_member(community, &hex::encode(actor_pubkey))
        .await?
        .map(|m| m.role);

    // The target's community role is read only for the admin guard rail — i.e.
    // an admin actioning a pubkey with ban/timeout — so the owner and
    // channel-role paths stay at a single query.
    let target_role = match (actor_role.as_deref(), action, target) {
        (Some("admin"), ModerationAction::Ban | ModerationAction::Timeout, target) => {
            match target {
                ModerationTarget::Pubkey(pk) => state
                    .db
                    .get_relay_member(community, &hex::encode(pk))
                    .await?
                    .map(|m| m.role),
                _ => None,
            }
        }
        _ => None,
    };

    // The channel role is read only when community authority does not apply and
    // the action is channel-local (DeleteMessage/Kick within `channel_id`).
    let channel_role = match (actor_role.as_deref(), action, channel_id) {
        (Some("owner") | Some("admin"), _, _) => None,
        (_, ModerationAction::DeleteMessage | ModerationAction::Kick, Some(channel_id)) => {
            state
                .db
                .get_member_role(community, channel_id, actor_pubkey)
                .await?
        }
        _ => None,
    };

    decide_authority(
        actor_role.as_deref(),
        target_role.as_deref(),
        channel_role.as_deref(),
        false,
        action,
    )
}

/// Pure authorization decision from resolved roles — the policy, factored out
/// of the I/O so it is exhaustively unit-testable.
///
/// - `actor_role` / `target_role`: community `relay_members` role, if any.
/// - `channel_role`: the actor's channel role, resolved by the caller only when
///   community authority does not apply and the action is channel-local.
/// - `channel_is_dm`: the target channel is `channel_type = 'dm'`.
fn decide_authority(
    actor_role: Option<&str>,
    target_role: Option<&str>,
    channel_role: Option<&str>,
    channel_is_dm: bool,
    action: ModerationAction,
) -> anyhow::Result<ModerationAuthority> {
    // A direct message is not a moderated space. Community owner/admin
    // authority is community-*wide*, not community-*total*: it stops at the
    // door of a private conversation between members, and delegated authority
    // over someone else's DM is not a capability this product grants at all.
    // So the carve-out is by channel type, ahead of every role — a channel
    // owner/admin of a DM would be refused too. That case is not reachable
    // through the product (DM participants are inserted with `role = 'member'`,
    // `buzz-db/src/dm.rs`), and were it ever minted by a migration or a direct
    // write it would still be the wrong grant.
    //
    // What this does NOT touch: a participant deleting their own DM message.
    // That is the author fast-path in `validate_admin_event`, upstream of this
    // seam (`side_effects.rs`), and the kind:5 self-delete path — neither
    // consults `authorize_moderation_action`.
    if channel_is_dm
        && matches!(
            action,
            ModerationAction::DeleteMessage | ModerationAction::Kick
        )
    {
        anyhow::bail!(
            "direct messages cannot be moderated; only a participant can delete their own message"
        );
    }

    match actor_role {
        // Owner holds every capability, community-wide, with no guard rail.
        Some("owner") => Ok(ModerationAuthority::CommunityOwner),
        // Admin holds every capability, but cannot ban/timeout the owner or a
        // fellow admin — only the owner may action an admin. The guard trips only
        // on a target *role* of owner/admin: a target with no `relay_members` row
        // (a drive-by spammer who already left) is bannable. Unban/Untimeout lift
        // a restriction and are intentionally unguarded at this role seam. The
        // command handler separately rejects a banned actor on every transport,
        // so the reachable case is an unrestricted admin lifting another admin's
        // restriction; that remains benign, audited, and owner-reversible.
        Some("admin") => {
            if matches!(action, ModerationAction::Ban | ModerationAction::Timeout)
                && matches!(target_role, Some("owner") | Some("admin"))
            {
                anyhow::bail!("an admin cannot ban or time out a community owner or fellow admin");
            }
            Ok(ModerationAuthority::CommunityAdmin)
        }
        // Not a community owner/admin: channel owner/admin keep channel-local
        // authority for DeleteMessage/Kick only.
        _ => match (action, channel_role) {
            (
                ModerationAction::DeleteMessage | ModerationAction::Kick,
                Some("owner") | Some("admin"),
            ) => Ok(ModerationAuthority::ChannelRole),
            _ => anyhow::bail!("moderator access required"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every community-wide action a community owner can take. Channel-local
    /// actions (DeleteMessage/Kick) are included — the owner holds them too.
    const ALL_ACTIONS: [ModerationAction; 8] = [
        ModerationAction::DeleteMessage,
        ModerationAction::Kick,
        ModerationAction::Ban,
        ModerationAction::Unban,
        ModerationAction::Timeout,
        ModerationAction::Untimeout,
        ModerationAction::ResolveReport,
        ModerationAction::ViewQueue,
    ];

    fn ok(r: anyhow::Result<ModerationAuthority>) -> ModerationAuthority {
        r.expect("expected authorization")
    }

    /// The role tables below all describe a *non-DM* channel; the DM carve-out
    /// has its own tests, which call [`decide_authority`] with `true`.
    fn decide(
        actor_role: Option<&str>,
        target_role: Option<&str>,
        channel_role: Option<&str>,
        action: ModerationAction,
    ) -> anyhow::Result<ModerationAuthority> {
        decide_authority(actor_role, target_role, channel_role, false, action)
    }

    #[test]
    fn community_owner_authorized_for_everything() {
        for action in ALL_ACTIONS {
            // Even against another owner/admin target: the owner has no guard rail.
            assert_eq!(
                ok(decide(Some("owner"), Some("admin"), None, action)),
                ModerationAuthority::CommunityOwner,
                "owner must be authorized for {action:?}"
            );
        }
    }

    #[test]
    fn community_admin_authorized_against_non_privileged_targets() {
        for action in ALL_ACTIONS {
            // Target is a plain member (or unknown) — admin holds every capability.
            assert_eq!(
                ok(decide(Some("admin"), Some("member"), None, action)),
                ModerationAuthority::CommunityAdmin,
                "admin must be authorized for {action:?} against a member"
            );
            assert_eq!(
                ok(decide(Some("admin"), None, None, action)),
                ModerationAuthority::CommunityAdmin,
                "admin must be authorized for {action:?} against a non-member"
            );
        }
    }

    #[test]
    fn admin_cannot_ban_or_timeout_owner_or_fellow_admin() {
        for target in ["owner", "admin"] {
            for action in [ModerationAction::Ban, ModerationAction::Timeout] {
                assert!(
                    decide(Some("admin"), Some(target), None, action).is_err(),
                    "admin must not {action:?} a community {target}"
                );
            }
        }
    }

    #[test]
    fn admin_can_ban_or_timeout_a_non_member_target() {
        // A target with no `relay_members` row (e.g. a drive-by spammer who
        // already left) must still be bannable — the guard trips on a privileged
        // *role*, never on a missing row.
        for action in [ModerationAction::Ban, ModerationAction::Timeout] {
            assert_eq!(
                ok(decide(Some("admin"), None, None, action)),
                ModerationAuthority::CommunityAdmin,
                "admin must be able to {action:?} a non-member target"
            );
            // A plain member target is likewise fair game.
            assert_eq!(
                ok(decide(Some("admin"), Some("member"), None, action)),
                ModerationAuthority::CommunityAdmin,
                "admin must be able to {action:?} a plain member"
            );
        }
    }

    #[test]
    fn admin_guard_rail_is_scoped_to_ban_and_timeout() {
        // Reversals and non-restriction actions against an admin target are allowed —
        // the guard rail protects against *applying* a restriction, not lifting one.
        for action in [
            ModerationAction::Unban,
            ModerationAction::Untimeout,
            ModerationAction::DeleteMessage,
            ModerationAction::Kick,
            ModerationAction::ResolveReport,
            ModerationAction::ViewQueue,
        ] {
            assert_eq!(
                ok(decide(Some("admin"), Some("admin"), None, action)),
                ModerationAuthority::CommunityAdmin,
                "admin must be authorized for {action:?} even against an admin target"
            );
        }
    }

    #[test]
    fn channel_role_covers_only_delete_and_kick() {
        for role in ["owner", "admin"] {
            for action in [ModerationAction::DeleteMessage, ModerationAction::Kick] {
                assert_eq!(
                    ok(decide(None, None, Some(role), action)),
                    ModerationAuthority::ChannelRole,
                    "channel {role} must be authorized for {action:?}"
                );
            }
            // No community authority: channel role does NOT grant community actions.
            for action in [
                ModerationAction::Ban,
                ModerationAction::Timeout,
                ModerationAction::Unban,
                ModerationAction::Untimeout,
                ModerationAction::ResolveReport,
                ModerationAction::ViewQueue,
            ] {
                assert!(
                    decide(None, None, Some(role), action).is_err(),
                    "channel {role} must NOT be authorized for community action {action:?}"
                );
            }
        }
    }

    /// The kind:9005 delete seam: `validate_admin_event` routes non-author
    /// deletes here, so the community-role rows below are exactly the cases the
    /// old `channel_members`-only check refused.
    #[test]
    fn delete_message_seam_covers_community_and_channel_roles() {
        // Community owner/admin with NO channel membership: authorized.
        for role in ["owner", "admin"] {
            let authority = ok(decide(
                Some(role),
                None,
                None,
                ModerationAction::DeleteMessage,
            ));
            assert_eq!(
                authority,
                match role {
                    "owner" => ModerationAuthority::CommunityOwner,
                    _ => ModerationAuthority::CommunityAdmin,
                },
                "community {role} must delete messages in a channel they never joined"
            );
        }
        // Plain community member with no channel role: refused.
        assert!(
            decide(Some("member"), None, None, ModerationAction::DeleteMessage).is_err(),
            "a plain community member must not delete another user's message"
        );
        // Channel owner/admin with no community role: still authorized (this is
        // the case the pre-existing check covered — the seam is strictly wider).
        for role in ["owner", "admin"] {
            assert_eq!(
                ok(decide(
                    None,
                    None,
                    Some(role),
                    ModerationAction::DeleteMessage
                )),
                ModerationAuthority::ChannelRole,
                "channel {role} must keep channel-local delete authority"
            );
        }
        // Plain channel member, and a community member who is also only a plain
        // channel member: refused on both.
        assert!(
            decide(None, None, Some("member"), ModerationAction::DeleteMessage).is_err(),
            "a plain channel member must not delete another user's message"
        );
        assert!(
            decide(
                Some("member"),
                None,
                Some("member"),
                ModerationAction::DeleteMessage
            )
            .is_err(),
            "community member + channel member must not delete another user's message"
        );
    }

    /// The DM carve-out. Before the 9005 widening, the non-author delete branch
    /// required an owner/admin row in `channel_members` for that channel, and
    /// DM participants are all inserted with `role = 'member'` — so a community
    /// admin who was not a participant was refused. The widening removed that
    /// accidental guard; this restores it deliberately, by channel type.
    #[test]
    fn no_role_can_moderate_inside_a_dm() {
        for action in [ModerationAction::DeleteMessage, ModerationAction::Kick] {
            for actor in [Some("owner"), Some("admin"), Some("member"), None] {
                assert!(
                    decide_authority(actor, None, None, true, action).is_err(),
                    "community {actor:?} must not {action:?} inside a DM"
                );
                // Even holding a channel owner/admin row (not reachable through
                // the product, but not worth trusting) grants nothing in a DM.
                for channel_role in ["owner", "admin", "member"] {
                    assert!(
                        decide_authority(actor, None, Some(channel_role), true, action).is_err(),
                        "community {actor:?} + channel {channel_role} must not {action:?} in a DM"
                    );
                }
            }
        }
    }

    /// The carve-out is scoped to the channel-local actions. Community-wide
    /// enforcement (ban/timeout/queue) is not channel-scoped, so a DM in the
    /// picture must not disarm it — the caller does not even resolve a channel
    /// type for those.
    #[test]
    fn dm_carve_out_does_not_disarm_community_wide_actions() {
        for action in [
            ModerationAction::Ban,
            ModerationAction::Unban,
            ModerationAction::Timeout,
            ModerationAction::Untimeout,
            ModerationAction::ResolveReport,
            ModerationAction::ViewQueue,
        ] {
            assert_eq!(
                ok(decide_authority(Some("owner"), None, None, true, action)),
                ModerationAuthority::CommunityOwner,
                "owner must keep {action:?} regardless of channel type"
            );
        }
    }

    #[test]
    fn plain_channel_member_and_stranger_are_denied() {
        for action in ALL_ACTIONS {
            assert!(
                decide(None, None, Some("member"), action).is_err(),
                "channel member must be denied {action:?}"
            );
            assert!(
                decide(None, None, None, action).is_err(),
                "user with no role must be denied {action:?}"
            );
        }
    }
}
