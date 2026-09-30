//! Where the agent host keeps its own things, and how two processes agree.
//!
//! One machine can run a production Beekeeper and a dev build at once, and the
//! dev/prod namespace split is what stops them adopting each other's detached
//! work or stealing each other's socket bind. The desktop app derives that
//! namespace from its Tauri app-data directory name; a launchd- or
//! systemd-started `beekeeper-host` has no such thing, so it is **told** which
//! instance it is (one registration per instance) rather than sniffing for it.
//!
//! Every path below is derived here and nowhere else. Two sides computing
//! "the socket" separately is how a desktop ends up reporting "the host is not
//! running" while the host sits listening one directory over — a failure that
//! reads as a missing feature rather than as a typo.

use std::path::{Path, PathBuf};

/// Which Beekeeper instance a host belongs to.
///
/// Deliberately a two-variant enum rather than a `&str` namespace: the two
/// namespaces are not extensible, and a typo in a string would silently mint a
/// third instance nobody is listening on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Instance {
    /// A signed release build: `~/.local/state/buzz`, nest `~/.beekeeper`.
    Production,
    /// A dev build: `~/.local/state/buzz-dev`, nest `~/.beekeeper-dev`.
    Dev,
}

/// The variable a launcher uses to tell the host which instance it serves.
///
/// Absent means [`Instance::Production`], because the overwhelmingly common
/// deployment — an installed app, or a server running one host — is production,
/// and a server operator should not have to set a variable to get the obvious
/// answer.
pub const INSTANCE_VAR: &str = "BEEKEEPER_HOST_INSTANCE";

/// The variable that overrides the control-socket path outright.
///
/// For containers and tests, where `$HOME` is not where state belongs.
pub const SOCKET_VAR: &str = "BEEKEEPER_HOST_SOCK";

/// The variable carrying the provider nsec directly (server/container path).
pub const PRIVATE_KEY_VAR: &str = "BEEKEEPER_HOST_PRIVATE_KEY";

/// The variable pointing at a file holding the provider nsec.
///
/// This is what lets a hardened deployment put the key on a secrets mount or a
/// tmpfs file with no code change, rather than in a `0600` file under `$HOME`.
pub const KEY_FILE_VAR: &str = "BEEKEEPER_HOST_KEY_FILE";

/// The variable every child of the host carries, holding the host's pid.
///
/// The desktop's untracked-harness sweep kills processes it does not recognise
/// (`managed_agents::runtime::sweep`). Before the host existed, "not mine"
/// and "nobody's" were the same set. This variable is how a host-owned child
/// says which they are, so a desktop boot cannot silently kill it.
pub const CHILD_MARKER_VAR: &str = "BEEKEEPER_HOST_CHILD";

impl Instance {
    /// The state-directory namespace: `buzz` or `buzz-dev`.
    pub fn namespace(self) -> &'static str {
        match self {
            Self::Production => "buzz",
            Self::Dev => "buzz-dev",
        }
    }

    /// The value [`INSTANCE_VAR`] carries for this instance.
    ///
    /// Distinct from [`Self::namespace`] on purpose, even though they are the
    /// same strings today: one is a directory name and the other is a wire
    /// value that `from_env` must accept. Tying them together would make a
    /// rename of either silently change the other.
    pub fn namespace_value(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Dev => "dev",
        }
    }

    /// The nest directory name this instance owns.
    pub fn nest_dir_name(self) -> &'static str {
        match self {
            Self::Production => ".beekeeper",
            Self::Dev => ".beekeeper-dev",
        }
    }

    /// The instance a nest directory name belongs to.
    ///
    /// The desktop knows its nest and not its namespace; this is the bridge,
    /// and it is here so the two sides cannot disagree about which name means
    /// dev.
    pub fn from_nest_dir_name(name: &str) -> Self {
        if name == Self::Dev.nest_dir_name() {
            Self::Dev
        } else {
            Self::Production
        }
    }

    /// The instance named by [`INSTANCE_VAR`], defaulting to production.
    ///
    /// An unrecognised value is **not** silently production: it is an operator
    /// typo in a unit file, and treating it as production would put a dev host
    /// on the production socket.
    pub fn from_env() -> Result<Self, String> {
        match std::env::var(INSTANCE_VAR) {
            Err(_) => Ok(Self::Production),
            Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
                "" | "production" | "prod" => Ok(Self::Production),
                "dev" | "development" => Ok(Self::Dev),
                other => Err(format!(
                    "{INSTANCE_VAR} must be \"production\" or \"dev\", not {other:?}"
                )),
            },
        }
    }
}

/// `~/.local/state/buzz[-dev]` — the per-instance state root.
///
/// Shared with the built-in shell sessions' detached hosts and the session
/// broker socket, which already live here.
pub fn state_root(home: &Path, instance: Instance) -> PathBuf {
    home.join(".local/state").join(instance.namespace())
}

/// `~/.local/state/buzz[-dev]/host` — everything the agent host itself owns.
///
/// Deliberately *not* the provider's state directory. That one stays under the
/// desktop's app-data tree, because five desktop modules write into it; this
/// one holds only the host's own socket, config, key fallback and log. Keeping
/// the two names distinct in code is how the socket stays out of a directory
/// other processes are also writing to.
pub fn host_dir(home: &Path, instance: Instance) -> PathBuf {
    state_root(home, instance).join("host")
}

/// The control socket this instance owns, ignoring any override.
pub fn default_host_socket_path(home: &Path, instance: Instance) -> PathBuf {
    host_dir(home, instance).join("host.sock")
}

/// The control socket to use. [`SOCKET_VAR`] wins when set.
///
/// Env-reading is kept in this one wrapper so the layout itself stays
/// provable: a test that asserts where things live must not be answerable by
/// whatever the surrounding shell exported.
pub fn host_socket_path(home: &Path, instance: Instance) -> PathBuf {
    match std::env::var_os(SOCKET_VAR) {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => default_host_socket_path(home, instance),
    }
}

/// The host's config file: which relay to serve, and where the provider's
/// state directory is.
pub fn host_config_path(home: &Path, instance: Instance) -> PathBuf {
    host_dir(home, instance).join("host.json")
}

/// The `0600` provider-key fallback — the only key route that works headless
/// on a machine with no keyring.
pub fn host_key_file_path(home: &Path, instance: Instance) -> PathBuf {
    host_dir(home, instance).join("provider-key")
}

/// The host's own log, distinct from the supervised provider's.
pub fn host_log_path(home: &Path, instance: Instance) -> PathBuf {
    host_dir(home, instance).join("host.log")
}

/// The file a running host writes into the provider's state directory to say
/// which host owns the lock.
///
/// A separate file rather than a second line in `provider.lock`: the
/// *provider* writes that one, as a bare pid, and every reader parses the whole
/// trimmed file as one integer. A second line would make an older reader's
/// `parse::<u32>()` fail and silently stop taking over a stale provider.
pub fn host_owner_file_name() -> &'static str {
    "host-owner.json"
}

/// Resolve `$HOME`, with the one error message worth reading.
pub fn home_dir() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "cannot resolve the home directory".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_and_production_never_share_a_socket() {
        let home = Path::new("/home/agent");
        assert_ne!(
            host_dir(home, Instance::Production),
            host_dir(home, Instance::Dev)
        );
        assert_eq!(
            host_dir(home, Instance::Production),
            Path::new("/home/agent/.local/state/buzz/host")
        );
        assert_eq!(
            host_dir(home, Instance::Dev),
            Path::new("/home/agent/.local/state/buzz-dev/host")
        );
    }

    /// Whatever `namespace_value` writes, `from_env` must read back — these
    /// are the two halves of one round trip through a launchd plist or a
    /// systemd unit, and a drift is a host on the wrong socket.
    #[test]
    fn the_instance_written_into_a_unit_file_reads_back() {
        for instance in [Instance::Production, Instance::Dev] {
            std::env::set_var(INSTANCE_VAR, instance.namespace_value());
            assert_eq!(Instance::from_env().expect("round trip"), instance);
        }
        std::env::set_var(INSTANCE_VAR, "nonsense");
        assert!(
            Instance::from_env().is_err(),
            "an operator typo must not be read as production"
        );
        std::env::remove_var(INSTANCE_VAR);
        assert_eq!(Instance::from_env().expect("default"), Instance::Production);
    }

    #[test]
    fn the_nest_name_is_the_bridge_between_the_two_vocabularies() {
        assert_eq!(
            Instance::from_nest_dir_name(".beekeeper-dev"),
            Instance::Dev
        );
        assert_eq!(
            Instance::from_nest_dir_name(".beekeeper"),
            Instance::Production
        );
        // Anything else is an installed app, i.e. production.
        assert_eq!(
            Instance::from_nest_dir_name("beekeeper"),
            Instance::Production
        );
    }

    #[test]
    fn the_host_directory_is_not_the_providers_state_directory() {
        // Restated as a test because conflating them is the mistake: the
        // provider's state dir is under the desktop's app-data tree and is
        // written by several desktop modules.
        let home = Path::new("/home/agent");
        let host = host_dir(home, Instance::Production);
        for path in [
            default_host_socket_path(home, Instance::Production),
            host_config_path(home, Instance::Production),
            host_key_file_path(home, Instance::Production),
            host_log_path(home, Instance::Production),
        ] {
            assert_eq!(path.parent(), Some(host.as_path()), "{path:?}");
        }
    }
}
