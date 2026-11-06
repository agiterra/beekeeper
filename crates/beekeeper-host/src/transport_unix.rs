//! Unix control transport and peer authentication.
use tokio::net::{UnixListener, UnixStream};
/// Whether a connected peer may drive this host.
///
/// Pure so the rule is provable: **only the same uid**. A `None` peer uid — a
/// platform that cannot tell us, or a syscall that failed — is refused, not
/// waved through. Failing open on an op that can stop a signing process is not
/// a trade worth making, and a refusal here is legible where a silent
/// acceptance is not.
pub fn peer_is_owner(peer_uid: Option<u32>, our_uid: u32) -> bool {
    peer_uid == Some(our_uid)
}

/// The connected peer's uid, or `None` when the platform will not say.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
fn peer_uid(stream: &UnixStream) -> Option<u32> {
    use std::os::fd::AsFd;
    nix::unistd::getpeereid(stream.as_fd())
        .ok()
        .map(|(uid, _gid)| uid.as_raw())
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> Option<u32> {
    use std::os::fd::AsFd;
    nix::sys::socket::getsockopt(&stream.as_fd(), nix::sys::socket::sockopt::PeerCredentials)
        .ok()
        .map(|credentials| credentials.uid())
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "linux"
)))]
fn peer_uid(_stream: &UnixStream) -> Option<u32> {
    None
}

/// The longest a Unix socket path may be, in bytes.
///
/// `sun_path` is 104 bytes on macOS and 108 on Linux; 100 is the safe floor
/// and the check exists to turn an opaque `EINVAL` into a sentence naming the
/// override that fixes it.
const MAX_SOCKET_PATH_BYTES: usize = 100;

/// Bind the control socket, or fail with something an operator can act on.
///
/// **Bound before supervision starts, and fatal if it fails.** A host with no
/// control socket is indistinguishable from no host at all: nothing could ask
/// it for its status and nothing could stop it. Continuing without one would
/// be the dishonest-status bug in its purest form — a process that is running
/// and looks, to every client, exactly like a process that was never
/// installed.
///
/// A stale socket from a previous run blocks `bind`. Removing it is safe
/// *because a live host of this instance would be holding the state-directory
/// claim*, which the takeover path checks before anything is started — the
/// socket file alone is not evidence of a running host, and treating it as
/// such would leave a crashed host's socket blocking its replacement forever.
pub fn bind(path: &std::path::Path) -> Result<UnixListener, String> {
    check_socket_path(path)?;
    // Create the parent when it is missing, owner-only — but **never change
    // the permissions of a directory that already exists.** `BEEKEEPER_HOST_SOCK`
    // may point anywhere, including a shared directory like `/tmp`, and
    // chmod-ing an operator's directory to 0700 because a socket happens to
    // live in it is not this process's business. On this machine the attempt
    // merely failed with EPERM; running as root it would have succeeded, and
    // broken whatever else used that directory.
    //
    // The socket file's own 0600 is the boundary that matters, and the host's
    // own directory is created restricted before this is ever called.
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            beekeeper_host_core::atomic_write::create_dir_all_restricted(parent)?;
        }
    }
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)
        .map_err(|error| format!("failed to bind {}: {error}", path.display()))?;
    restrict_socket_permissions(path);
    Ok(listener)
}

/// Refuse a socket path the kernel will reject, by name.
///
/// Pure so the limit is provable without a filesystem: the raw failure is
/// `EINVAL` with no indication of the cause, and an operator seeing it has no
/// way to guess that a long `$HOME` is the problem.
pub(crate) fn check_socket_path(path: &std::path::Path) -> Result<(), String> {
    let bytes = path.as_os_str().as_encoded_bytes().len();
    if bytes <= MAX_SOCKET_PATH_BYTES {
        return Ok(());
    }
    Err(format!(
        "the control socket path is {bytes} bytes, longer than the {MAX_SOCKET_PATH_BYTES} a Unix socket allows: {}. Set {} to a shorter path.",
        path.display(),
        beekeeper_host_core::layout::SOCKET_VAR
    ))
}

pub type Listener = UnixListener;
pub type Stream = UnixStream;
pub async fn connect(path: &std::path::Path) -> std::io::Result<Stream> {
    Stream::connect(path).await
}
pub async fn accept(listener: &Listener) -> std::io::Result<Stream> {
    listener.accept().await.map(|(stream, _)| stream)
}
pub fn authorized(stream: &Stream) -> bool {
    peer_is_owner(peer_uid(stream), nix::unistd::getuid().as_raw())
}
fn restrict_socket_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}
