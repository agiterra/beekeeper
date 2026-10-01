//! Installing the host as a system LaunchDaemon, for a headless Mac.
//!
//! A LaunchAgent lives in a user's `gui/$UID` domain, which exists only while
//! that user is logged in at the console. On a Mac mini in a cupboard nobody
//! is, so the agent never starts after a reboot and stops at every logout.
//! A LaunchDaemon in the `system` domain is loaded at boot whoever is logged
//! in, and `UserName` makes launchd drop to the persona's account before it
//! runs anything — so the host, its socket and its children are all that
//! user's, exactly as under a LaunchAgent. Only the registration is root's.
//!
//! Three things differ from the LaunchAgent on purpose:
//!
//! - **`KeepAlive` is `{SuccessfulExit: false}`, not `false`.** The agent's
//!   `false` exists so a deleted app fails once per login instead of looping;
//!   a daemon has no login to wait for, so a crash would otherwise leave the
//!   machine without a host until somebody noticed. A clean exit — which is
//!   what the host does on SIGTERM — stays down, so stopping it is still
//!   observable. `ThrottleInterval` bounds the loop a missing binary causes.
//! - **The environment is written out.** launchd gives a daemon no login
//!   shell, so `HOME`, `USER`, `LOGNAME` and `PATH` are set from the password
//!   database here rather than trusted to arrive. Never from the invoking
//!   shell: under `sudo`, `$HOME` may be root's or the operator's, and either
//!   would put the host's state in the wrong tree.
//! - **The log exists before launchd opens it**, owned by the persona. Which
//!   uid launchd opens `StandardOutPath` as is not documented; creating the
//!   file first means the answer cannot leave a root-owned log in the
//!   persona's state directory.
//!
//! Every privileged effect — `chown` and `launchctl` — is gated on
//! [`System::is_live`]: the real `/Library/LaunchDaemons`, and the real
//! effective uid being root. A test that names a temporary directory and
//! *claims* root exercises every refusal and every file it would write, and
//! cannot reach the machine's launchd. That is the `home_is_ours` lesson
//! applied before the bug rather than after it.

use std::path::{Path, PathBuf};

use beekeeper_host_core::atomic_write::atomic_write_with_mode;
use beekeeper_host_core::layout::{self, Instance, INSTANCE_VAR};

use super::{Domain, Registration, Service};

/// Where launchd reads system daemons from.
pub const DAEMON_DIR: &str = "/Library/LaunchDaemons";

/// The `PATH` a daemon runs with: the system directories and nothing a user
/// could write to. The host names every binary it runs by absolute path.
const DAEMON_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

/// Seconds launchd waits before restarting a daemon that exited, and so the
/// ceiling on how fast a missing binary can spin.
const THROTTLE_INTERVAL: u64 = 30;

/// The account a daemon runs as, read from the password database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// The short user name, as launchd's `UserName` takes it.
    pub name: String,
    /// Its uid.
    pub uid: u32,
    /// Its primary gid.
    pub gid: u32,
    /// Its home directory, from the password database — never `$HOME`.
    pub home: PathBuf,
}

/// Where a daemon install writes, and whether it may act on the machine.
pub(crate) struct System<'a> {
    /// The directory the plist goes in.
    pub(crate) dir: &'a Path,
    /// Whether the caller is root. A test may claim this; [`Self::is_live`]
    /// does not believe it.
    pub(crate) root: bool,
}

impl System<'static> {
    /// The machine's own launchd, as this process actually is.
    pub(crate) fn live() -> Self {
        Self {
            dir: Path::new(DAEMON_DIR),
            root: nix::unistd::geteuid().is_root(),
        }
    }
}

impl System<'_> {
    /// Whether this is the real `system` domain, written by a real root —
    /// the only case in which ownership changes and `launchctl` may run.
    fn is_live(&self) -> bool {
        self.dir == Path::new(DAEMON_DIR) && nix::unistd::geteuid().is_root()
    }
}

/// Refuse `--system` where it means nothing.
///
/// A Linux system unit is a different file in a different place with
/// different semantics, and it is not written by this code. A server there
/// runs the systemd *user* unit with lingering, which already survives logout.
pub fn supported() -> Result<(), String> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        Err(
            "`--system` installs a macOS LaunchDaemon and is not available on this platform. \
             On Linux, `bee host install` writes a systemd user unit; run `loginctl \
             enable-linger <user>` so it survives logout (docs/agent-host.md § On a server)."
                .to_string(),
        )
    }
}

/// Refuse a user name that cannot be one component of a launchd label.
///
/// The name ends up in the label, the plist's file name, and the XML. `.`
/// would make `daemon.x.dev` ambiguous between user `x` on the dev instance
/// and user `x.dev` on production; `/` would leave `/Library/LaunchDaemons`.
/// Root is refused by name here and by uid in [`resolve_account`]: the host
/// must run as the persona it serves, never as the superuser.
pub fn validate_user_name(name: &str) -> Result<(), String> {
    let well_formed = !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !well_formed {
        return Err(format!(
            "{name:?} cannot name a daemon's user: use the account's short name, which may \
             contain only letters, digits, `_` and `-`, and may not start with `-`"
        ));
    }
    if name == "root" {
        return Err(
            "the agent host must not run as root: name the persona's own account with `--user`"
                .to_string(),
        );
    }
    Ok(())
}

/// Look `name` up in the password database.
pub fn resolve_account(name: &str) -> Result<Account, String> {
    validate_user_name(name)?;
    let user = nix::unistd::User::from_name(name)
        .map_err(|error| format!("could not look up the user {name:?}: {error}"))?
        .ok_or_else(|| format!("there is no user named {name:?} on this machine"))?;
    if user.uid.is_root() {
        return Err(format!(
            "{name:?} is uid 0; the agent host must not run as root"
        ));
    }
    Ok(Account {
        name: user.name,
        uid: user.uid.as_raw(),
        gid: user.gid.as_raw(),
        home: user.dir,
    })
}

/// The name of the account this process runs as, when it has one.
pub(crate) fn current_user_name() -> Option<String> {
    nix::unistd::User::from_uid(nix::unistd::getuid())
        .ok()
        .flatten()
        .map(|user| user.name)
}

/// `io.agiterra.beekeeper.host.daemon.<user>[.dev]`.
///
/// Per user, because one Mac can host several personas, each with its own
/// host; per instance for the same reason the agent labels are. The `.daemon`
/// segment means no daemon label can equal an agent label, so the two can
/// never boot each other out.
pub fn label(user: &str, instance: Instance) -> String {
    let stem = format!(
        "{}.daemon.{user}",
        super::service_name(Service::AgentHost, Instance::Production)
    );
    match instance {
        Instance::Production => stem,
        Instance::Dev => format!("{stem}.dev"),
    }
}

/// The plist for `user` and `instance` inside `dir`.
pub fn plist_path(dir: &Path, user: &str, instance: Instance) -> PathBuf {
    dir.join(format!("{}.plist", label(user, instance)))
}

/// The arguments that ask launchd whether a system daemon is loaded.
///
/// `print`, not `list`: `launchctl list` answers about the caller's own
/// domain only, and an unprivileged caller's domain is not `system`.
/// `launchctl print system/<label>` needs no root, exits 0 when the job is
/// loaded and 113 when it is not (both confirmed on macOS 26).
pub(crate) fn print_args(label: &str) -> [String; 2] {
    ["print".to_string(), format!("system/{label}")]
}

/// Whether launchd has `user`'s daemon loaded. `None` when it could not say.
pub fn loaded(user: &str, instance: Instance) -> Option<bool> {
    let code = std::process::Command::new("launchctl")
        .args(print_args(&label(user, instance)))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?
        .code()?;
    match code {
        0 => Some(true),
        113 => Some(false),
        _ => None,
    }
}

/// What a daemon registration in `dir` says, or `None` when there is none.
pub(crate) fn registration(dir: &Path, user: &str, instance: Instance) -> Option<Registration> {
    let path = plist_path(dir, user, instance);
    if !path.exists() {
        return None;
    }
    // World-readable by construction, so an unprivileged `status` can read
    // the program it names.
    let program = std::fs::read_to_string(&path)
        .ok()
        .and_then(|content| super::program_from_plist(&content));
    let mut warnings = Vec::new();
    match &program {
        Some(program) if !program.exists() => warnings.push(format!(
            "the system daemon names {}, which does not exist — remove it with `sudo bee host \
             uninstall --system --user {user}` or reinstall it",
            program.display()
        )),
        None => warnings.push(format!(
            "{} exists but does not name a program this version understands",
            path.display()
        )),
        _ => {}
    }
    Some(Registration {
        installed: true,
        path,
        program,
        warnings,
        domain: Domain::System,
    })
}

/// The command line that re-runs this one under `sudo`.
///
/// The executable by absolute path, because `sudo`'s `secure_path` usually
/// does not include wherever `bee` was installed.
pub fn rerun_command() -> String {
    let exe = std::env::current_exe()
        .map(|exe| exe.display().to_string())
        .unwrap_or_else(|_| "bee".to_string());
    std::iter::once(exe)
        .chain(std::env::args().skip(1))
        .map(|arg| shell_quote(&arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./=:@%+,".contains(c));
    if plain {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

fn needs_root(action: &str, rerun: &str) -> String {
    format!("{action} a system daemon needs root, and this is not running as root. Run:\n  sudo {rerun}")
}

/// Render the daemon's plist. Writes nothing; needs no privilege.
pub fn plist_contents(
    account: &Account,
    instance: Instance,
    program: &Path,
) -> Result<String, String> {
    let label = label(&account.name, instance);
    let log = layout::host_log_path(&account.home, instance);
    let program = xml_safe(program)?;
    let home = xml_safe(&account.home)?;
    let log = xml_safe(&log)?;
    let user = &account.name;
    // Hand-written for the reason the LaunchAgent is: a fixed shape a person
    // reads. `ProcessType` is `Standard` rather than the agent's `Background`
    // because nothing in a server's foreground competes with it; it is one
    // line so it can follow the maintainer's call.
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{label}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{program}</string>{argv}
	</array>
	<key>UserName</key>
	<string>{user}</string>
	<key>WorkingDirectory</key>
	<string>{home}</string>
	<key>EnvironmentVariables</key>
	<dict>
		<key>HOME</key>
		<string>{home}</string>
		<key>USER</key>
		<string>{user}</string>
		<key>LOGNAME</key>
		<string>{user}</string>
		<key>PATH</key>
		<string>{DAEMON_PATH}</string>
		<key>{INSTANCE_VAR}</key>
		<string>{instance_value}</string>
	</dict>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<dict>
		<key>SuccessfulExit</key>
		<false/>
	</dict>
	<key>ThrottleInterval</key>
	<integer>{THROTTLE_INTERVAL}</integer>
	<key>ExitTimeOut</key>
	<integer>{exit_timeout}</integer>
	<key>ProcessType</key>
	<string>Standard</string>
	<key>StandardOutPath</key>
	<string>{log}</string>
	<key>StandardErrorPath</key>
	<string>{log}</string>
</dict>
</plist>
"#,
        argv = Service::AgentHost
            .args()
            .iter()
            .map(|arg| format!("\n\t\t<string>{arg}</string>"))
            .collect::<String>(),
        instance_value = instance.namespace_value(),
        exit_timeout = super::stop_timeout_secs(),
    ))
}

/// A path as plist text, refusing what would need escaping.
///
/// Refused rather than escaped because `program_from_registration` reads the
/// text back verbatim: an escaped `&amp;` would round-trip as a path that does
/// not exist, and `status` would then report a working daemon as broken.
fn xml_safe(path: &Path) -> Result<&str, String> {
    let text = path
        .to_str()
        .ok_or_else(|| format!("{} is not valid UTF-8", path.display()))?;
    if text.contains(['<', '>', '&']) {
        return Err(format!(
            "{text} contains a character a plist would need escaped (`<`, `>` or `&`); move it \
             somewhere without one"
        ));
    }
    Ok(text)
}

/// Install `program` as `user`'s agent host daemon, on this machine.
///
/// `rerun` is the command line to print after `sudo` when this is not root.
pub fn install(
    user: &str,
    instance: Instance,
    program: &Path,
    rerun: &str,
) -> Result<Registration, String> {
    supported()?;
    let account = resolve_account(user)?;
    install_into(&System::live(), &account, instance, program, rerun)
}

/// Remove `user`'s daemon. Safe when there is none.
///
/// Takes the name rather than resolving it: the account may already have been
/// deleted, and its daemon must still be removable.
pub fn uninstall(user: &str, instance: Instance, rerun: &str) -> Result<(), String> {
    supported()?;
    validate_user_name(user)?;
    uninstall_from(&System::live(), user, instance, rerun)
}

/// Render the plist an install would write, for `--print`.
pub fn render(user: &str, instance: Instance, program: &Path) -> Result<String, String> {
    supported()?;
    let account = resolve_account(user)?;
    super::check_program(Service::AgentHost, program)?;
    plist_contents(&account, instance, program)
}

pub(crate) fn install_into(
    system: &System<'_>,
    account: &Account,
    instance: Instance,
    program: &Path,
    rerun: &str,
) -> Result<Registration, String> {
    super::check_program(Service::AgentHost, program)?;
    let contents = plist_contents(account, instance, program)?;
    // Before anything that reads the persona's tree: as anyone but root those
    // reads can fail for permissions and would be reported as the wrong fact.
    if !system.root {
        return Err(needs_root("installing", rerun));
    }
    let config = layout::host_config_path(&account.home, instance);
    if !config.exists() {
        return Err(format!(
            "{}'s agent host is not commissioned: {} does not exist. Commission it first — \
             Beekeeper does this on a Mac with a screen; on a headless one, write host.json and \
             provider-key by hand (docs/agent-host.md § On a server) — and check it with \
             `beekeeper-host check` run as {}.",
            account.name,
            config.display(),
            account.name
        ));
    }
    let agent = super::launch_agent_path(Service::AgentHost, &account.home, instance);
    if agent.exists() {
        return Err(format!(
            "{} already has a login LaunchAgent for the agent host ({}), and two hosts would \
             race for one control socket. Remove it first: as {}, `bee host uninstall`; or as \
             root, `launchctl bootout gui/{}/{}` and delete that file.",
            account.name,
            agent.display(),
            account.name,
            account.uid,
            super::service_name(Service::AgentHost, instance)
        ));
    }
    check_executable_by(account, program)?;
    prepare_log(system, account, instance)?;

    let path = plist_path(system.dir, &account.name, instance);
    atomic_write_with_mode(&path, contents.as_bytes(), 0o644)?;
    if system.is_live() {
        // launchd refuses a daemon plist that is not root's.
        std::os::unix::fs::lchown(&path, Some(0), Some(0))
            .map_err(|error| format!("failed to chown {} to root: {error}", path.display()))?;
        // The same unwinding as the LaunchAgent: a plist launchd will not load
        // must not stay behind reading as installed.
        if let Err(error) = activate(&label(&account.name, instance), &path) {
            let _ = std::fs::remove_file(&path);
            return Err(error);
        }
    }
    registration(system.dir, &account.name, instance)
        .ok_or_else(|| format!("wrote {} but cannot read it back", path.display()))
}

pub(crate) fn uninstall_from(
    system: &System<'_>,
    user: &str,
    instance: Instance,
    rerun: &str,
) -> Result<(), String> {
    if !system.root {
        return Err(needs_root("removing", rerun));
    }
    let path = plist_path(system.dir, user, instance);
    if system.is_live() {
        // Not loaded is the answer we want, so its error is ignored.
        super::run(
            "launchctl",
            &["bootout", &format!("system/{}", label(user, instance))],
        )
        .ok();
    }
    // No refusal file: that records a person's answer to the desktop's login
    // question, and a root operator removing a daemon has not been asked it.
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("failed to remove {}: {error}", path.display())),
    }
}

/// `enable`, `bootout`, `bootstrap` — in that order.
///
/// `enable` clears a `disabled` override a past `launchctl disable` left in
/// launchd's database, which would otherwise make the bootstrap succeed and
/// the job never run. `bootout` first so a reinstall replaces a loaded job.
fn activate(label: &str, path: &Path) -> Result<(), String> {
    let target = format!("system/{label}");
    super::run("launchctl", &["enable", &target]).ok();
    super::run("launchctl", &["bootout", &target]).ok();
    super::run(
        "launchctl",
        &["bootstrap", "system", &path.to_string_lossy()],
    )
    .map_err(|error| {
        format!(
            "wrote {} but launchctl refused to load it: {error}. Try `sudo launchctl bootstrap \
             system {}` and read `sudo launchctl print {target}`.",
            path.display(),
            path.display()
        )
    })
}

/// Refuse a program `account` could not run, judged from mode bits.
///
/// Best-effort, and it errs towards refusing: it reads owner, primary group
/// and other bits along the resolved path, and does not see supplementary
/// groups, ACLs, or TCC — a binary under another user's `~/Downloads` can pass
/// here and still be blocked at launch. What it does catch is the common
/// mistake: pointing a persona's daemon at a build inside the operator's own
/// `0700` home, which would fail at every boot with nothing but a log line.
fn check_executable_by(account: &Account, program: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;

    let resolved = std::fs::canonicalize(program)
        .map_err(|error| format!("cannot resolve {}: {error}", program.display()))?;
    // The bits that apply are the first class that matches, as the kernel
    // decides: owner, then group, then other — never a union.
    let permits = |meta: &std::fs::Metadata, want: u32| {
        let shift = if meta.uid() == account.uid {
            6
        } else if meta.gid() == account.gid {
            3
        } else {
            0
        };
        (meta.mode() >> shift) & want == want
    };
    let meta = std::fs::metadata(&resolved)
        .map_err(|error| format!("cannot read {}: {error}", resolved.display()))?;
    if !permits(&meta, 0o5) {
        return Err(format!(
            "{} cannot read and execute {}; install the binary somewhere every user can run it \
             (`just install-bee`), or name one with `--program`",
            account.name,
            resolved.display()
        ));
    }
    for dir in resolved.ancestors().skip(1) {
        let meta = std::fs::metadata(dir)
            .map_err(|error| format!("cannot read {}: {error}", dir.display()))?;
        if !permits(&meta, 0o1) {
            return Err(format!(
                "{} cannot reach {} because it cannot enter {}; install the binary somewhere \
                 every user can run it, or name one with `--program`",
                account.name,
                resolved.display(),
                dir.display()
            ));
        }
    }
    Ok(())
}

/// Make the host's log exist, owned by the persona, before launchd opens it.
///
/// Root working inside a tree a user controls is the classic way to be
/// tricked into writing somewhere else, so every step that could follow a
/// link does not: missing directories are created only under a parent the
/// persona owns, the host directory is opened `O_NOFOLLOW` and its owner read
/// from the open descriptor, and the log is opened relative to that
/// descriptor, `O_NOFOLLOW`, and `chown`ed through it. A swapped link can
/// make this refuse; it cannot make it touch a file the persona could not
/// have touched itself.
fn prepare_log(system: &System<'_>, account: &Account, instance: Instance) -> Result<(), String> {
    use nix::errno::Errno;
    use nix::fcntl::{open, openat, OFlag};
    use nix::sys::stat::Mode;
    use std::os::unix::fs::MetadataExt;

    let live = system.is_live();
    let host_dir = layout::host_dir(&account.home, instance);
    let relative = host_dir.strip_prefix(&account.home).map_err(|_| {
        format!(
            "{} is not under {}",
            host_dir.display(),
            account.home.display()
        )
    })?;

    // Reached only when the commissioning gate is relaxed — commissioning
    // writes host.json into this directory, so today it always exists.
    let mut current = account.home.clone();
    for component in relative.components() {
        let next = current.join(component);
        match std::fs::symlink_metadata(&next) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let parent = std::fs::metadata(&current)
                    .map_err(|error| format!("cannot read {}: {error}", current.display()))?;
                if parent.uid() != account.uid {
                    return Err(format!(
                        "{} is not owned by {}, so the host's state directory will not be \
                         created inside it",
                        current.display(),
                        account.name
                    ));
                }
                std::fs::create_dir(&next)
                    .map_err(|error| format!("failed to create {}: {error}", next.display()))?;
                if next == host_dir {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&next, std::fs::Permissions::from_mode(0o700))
                        .map_err(|error| {
                            format!("failed to restrict {}: {error}", next.display())
                        })?;
                }
                if live {
                    std::os::unix::fs::lchown(&next, Some(account.uid), Some(account.gid))
                        .map_err(|error| format!("failed to chown {}: {error}", next.display()))?;
                }
            }
            Err(error) => return Err(format!("cannot read {}: {error}", next.display())),
        }
        current = next;
    }
    let dir = open(
        &host_dir,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| format!("cannot open {}: {error}", host_dir.display()))?;
    let dir = std::fs::File::from(dir);
    let owner = dir
        .metadata()
        .map_err(|error| format!("cannot read {}: {error}", host_dir.display()))?
        .uid();
    if owner != account.uid {
        return Err(format!(
            "{} is not owned by {} (uid {owner}), so its log will not be created there",
            host_dir.display(),
            account.name
        ));
    }

    const LOG: &str = "host.log";
    // `O_NONBLOCK` so a FIFO planted at the name fails instead of hanging.
    let flags = OFlag::O_WRONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC | OFlag::O_NONBLOCK;
    let log = match openat(
        &dir,
        LOG,
        flags | OFlag::O_CREAT | OFlag::O_EXCL,
        Mode::from_bits_truncate(0o600),
    ) {
        Ok(fd) => fd,
        Err(Errno::EEXIST) => {
            openat(&dir, LOG, flags | OFlag::O_APPEND, Mode::empty()).map_err(|error| {
                format!(
                    "cannot open {}: {error} — it must be a regular file, not a link",
                    host_dir.join(LOG).display()
                )
            })?
        }
        Err(error) => {
            return Err(format!(
                "cannot create {}: {error}",
                host_dir.join(LOG).display()
            ))
        }
    };
    let log = std::fs::File::from(log);
    let meta = log
        .metadata()
        .map_err(|error| format!("cannot read {}: {error}", host_dir.join(LOG).display()))?;
    if !meta.is_file() {
        return Err(format!(
            "{} is not a regular file",
            host_dir.join(LOG).display()
        ));
    }
    if live {
        std::os::unix::fs::fchown(&log, Some(account.uid), Some(account.gid)).map_err(|error| {
            format!("failed to chown {}: {error}", host_dir.join(LOG).display())
        })?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "launchd_daemon_tests.rs"]
mod tests;
