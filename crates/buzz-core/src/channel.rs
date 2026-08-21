//! Channel and membership enums shared across crates.
//!
//! These live in `buzz-core` (zero I/O deps) so both the SDK (client-side)
//! and the DB layer (server-side) can use the same types without pulling in
//! sqlx/tokio.

use std::fmt;
use std::str::FromStr;

/// Returns the canonical display name for a channel.
///
/// Channel names are rendered with a leading `#` by clients, so surrounding
/// whitespace and user-supplied hash prefixes are removed here to keep the
/// stored name prefix-free.
pub fn canonical_channel_name(name: &str) -> &str {
    name.trim_start_matches(|c: char| c == '#' || c.is_whitespace())
        .trim_end()
}

/// Whether a channel is publicly visible or invite-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelVisibility {
    /// Searchable; anyone can join without an invite.
    Open,
    /// Hidden; requires an invite to join.
    Private,
}

impl ChannelVisibility {
    /// Canonical string representation (matches DB enum and Nostr tags).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Private => "private",
        }
    }
}

impl fmt::Display for ChannelVisibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ChannelVisibility {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open" => Ok(Self::Open),
            "private" => Ok(Self::Private),
            other => Err(format!("unknown channel visibility: {other:?}")),
        }
    }
}

/// The functional type of a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelType {
    /// Linear message stream (the default).
    Stream,
    /// Threaded forum-style discussion.
    Forum,
    /// Direct message conversation.
    Dm,
    /// Internal workflow execution channel.
    Workflow,
    /// Hidden per-project transport channel (e.g. coding-session events).
    ///
    /// Identified by type rather than display name so a user-named channel can
    /// never be mistaken for one. Project members are admitted by the relay
    /// through the project ACL instead of explicit channel membership.
    Transport,
}

impl ChannelType {
    /// Canonical string representation (matches DB enum and Nostr tags).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stream => "stream",
            Self::Forum => "forum",
            Self::Dm => "dm",
            Self::Workflow => "workflow",
            Self::Transport => "transport",
        }
    }
}

impl fmt::Display for ChannelType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ChannelType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stream" => Ok(Self::Stream),
            "forum" => Ok(Self::Forum),
            "dm" => Ok(Self::Dm),
            "workflow" => Ok(Self::Workflow),
            "transport" => Ok(Self::Transport),
            other => Err(format!("unknown channel type: {other:?}")),
        }
    }
}

/// A member's role within a channel.
///
/// The hierarchy for permission checks is: Owner > Admin > Member > Guest.
/// Bot is a **separate designation** — it is not part of the linear hierarchy.
/// Use [`MemberRole::permission_level`] for numeric comparisons in authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberRole {
    /// Full control — can manage members and delete the channel.
    Owner,
    /// Can manage members and channel settings.
    Admin,
    /// Standard participant.
    Member,
    /// Read-only external participant.
    Guest,
    /// Automated agent or integration (not in the role hierarchy).
    Bot,
}

impl MemberRole {
    /// Canonical string representation (matches DB enum and Nostr tags).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Member => "member",
            Self::Guest => "guest",
            Self::Bot => "bot",
        }
    }

    /// Elevated roles that only existing owners/admins may grant.
    pub fn is_elevated(&self) -> bool {
        matches!(self, Self::Owner | Self::Admin)
    }

    /// Numeric permission level for authorization comparisons.
    ///
    /// Higher = more privileged. Bot returns 0 (must use explicit grants).
    /// Use `role.permission_level() >= required.permission_level()` for checks.
    pub fn permission_level(self) -> u8 {
        match self {
            Self::Owner => 4,
            Self::Admin => 3,
            Self::Member => 2,
            Self::Guest => 1,
            Self::Bot => 0,
        }
    }

    /// Returns true if this role meets or exceeds the required role's permission level.
    ///
    /// Bot never meets any requirement (returns false for all non-Bot requirements).
    pub fn has_at_least(self, required: MemberRole) -> bool {
        self.permission_level() >= required.permission_level()
    }
}

impl fmt::Display for MemberRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MemberRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "owner" => Ok(Self::Owner),
            "admin" => Ok(Self::Admin),
            "member" => Ok(Self::Member),
            "guest" => Ok(Self::Guest),
            "bot" => Ok(Self::Bot),
            other => Err(format!("unknown member role: {other:?}")),
        }
    }
}

/// A member's role within a project (NIP-MP Buzz access extension).
///
/// The hierarchy is Owner > Collaborator > Viewer. The project creator (the
/// kind:30621 address pubkey) is always an implicit Owner and never appears
/// on the roster. String values match [`crate::kind::PROJECT_ROLES`], the DB
/// `project_acl_members.role` column, and the role element of roster tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectRole {
    /// Full rights inside the project plus roster management.
    Owner,
    /// Read everything, write into project contents (channels, own sessions,
    /// repos); no roster management. The default for legacy role-less
    /// invites — pre-role members could already write.
    Collaborator,
    /// Read-only across the project and its contents.
    Viewer,
}

impl ProjectRole {
    /// Canonical string representation (matches DB values and Nostr tags).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Collaborator => "collaborator",
            Self::Viewer => "viewer",
        }
    }

    /// Whether this role may write into the project's contents (post in its
    /// channels, create sessions/repos, publish into its transports).
    pub fn can_write(self) -> bool {
        matches!(self, Self::Owner | Self::Collaborator)
    }

    /// Whether this role may manage the project roster (put/remove members).
    pub fn can_manage_roster(self) -> bool {
        matches!(self, Self::Owner)
    }
}

impl fmt::Display for ProjectRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProjectRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "owner" => Ok(Self::Owner),
            "collaborator" => Ok(Self::Collaborator),
            "viewer" => Ok(Self::Viewer),
            other => Err(format!("unknown project role: {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::canonical_channel_name;

    #[test]
    fn channel_names_trim_whitespace_and_drop_all_leading_hashes() {
        assert_eq!(canonical_channel_name("channel"), "channel");
        assert_eq!(canonical_channel_name("#channel"), "channel");
        assert_eq!(canonical_channel_name("###channel"), "channel");
        assert_eq!(canonical_channel_name("  ###channel  "), "channel");
        assert_eq!(canonical_channel_name("# channel"), "channel");
        assert_eq!(canonical_channel_name("### channel  "), "channel");
        assert_eq!(canonical_channel_name("  ###  "), "");
        assert_eq!(canonical_channel_name("# #"), "");
        assert_eq!(canonical_channel_name("### ###"), "");
        assert_eq!(canonical_channel_name("channel#topic"), "channel#topic");
    }
}
