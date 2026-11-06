//! Local Windows named pipes. No network clients and no default permissive DACL.
use std::{io, path::Path};

use interprocess::{
    local_socket::{tokio::prelude::*, GenericNamespaced, ListenerOptions, ToNsName},
    os::windows::{local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor},
};
use sha2::{Digest, Sha256};
use widestring::U16CString;

pub type Listener = interprocess::local_socket::tokio::Listener;
pub type Stream = interprocess::local_socket::tokio::Stream;

fn name(path: &Path) -> String {
    // The configured socket path is an endpoint identifier, never a file removed
    // by this transport. Distinct disposable homes get distinct pipe names.
    format!(
        "beekeeper-host-{:x}",
        Sha256::digest(path.as_os_str().as_encoded_bytes())
    )
}

pub fn bind(path: &Path) -> Result<Listener, String> {
    // Resolve the token's user, not its default owner (which can be the
    // Administrators group for an elevated token).
    let sid = win_custody::current_user_sid_string().map_err(|e| e.to_string())?;
    let sddl =
        U16CString::from_str(format!("O:{sid}D:P(A;;GA;;;{sid})")).map_err(|e| e.to_string())?;
    let descriptor = SecurityDescriptor::deserialize(&sddl).map_err(|e| e.to_string())?;
    ListenerOptions::new()
        .name(
            name(path)
                .to_ns_name::<GenericNamespaced>()
                .map_err(|e| e.to_string())?,
        )
        .security_descriptor(descriptor)
        .create_tokio()
        .map_err(|e| format!("failed to bind owner-only control pipe: {e}"))
}

pub async fn connect(path: &Path) -> io::Result<Stream> {
    Stream::connect(name(path).to_ns_name::<GenericNamespaced>()?).await
}

pub async fn accept(listener: &Listener) -> io::Result<Stream> {
    listener.accept().await
}

pub fn authorized(_stream: &Stream) -> bool {
    // Access was checked by the kernel against the explicitly supplied DACL.
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn local_pipe_roundtrip_and_competing_bind_refusal() {
        let dir = tempfile::tempdir().expect("home");
        let path = dir.path().join("host.sock");
        let listener = bind(&path).expect("owner-only bind");
        assert!(bind(&path).is_err(), "a second server cannot join the pipe");
        let serving = tokio::spawn(async move {
            let mut stream = accept(&listener).await.expect("accept");
            let mut request = [0; 4];
            stream.read_exact(&mut request).await.expect("read");
            assert_eq!(&request, b"ping");
            stream.write_all(b"pong").await.expect("write");
        });
        let mut stream = connect(&path).await.expect("same-user connect");
        stream.write_all(b"ping").await.expect("write");
        let mut response = [0; 4];
        stream.read_exact(&mut response).await.expect("read");
        assert_eq!(&response, b"pong");
        serving.await.expect("server");
    }

    #[test]
    fn disposable_homes_have_distinct_endpoints() {
        assert_ne!(
            name(Path::new(r"C:\one\host.sock")),
            name(Path::new(r"C:\two\host.sock"))
        );
    }
}
