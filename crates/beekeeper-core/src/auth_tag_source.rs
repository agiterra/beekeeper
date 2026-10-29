//! Where a process finds its NIP-OA auth tag: `BEEKEEPER_AUTH_TAG_FILE`,
//! re-read at every use, else `BEEKEEPER_AUTH_TAG`.
//!
//! An owner-signed auth tag can carry a `created_at<T` condition, and the relay
//! checks `T` on every authenticated request. An environment variable is fixed
//! when a process starts, so a host that renews the tag cannot reach a process
//! that is already running: its next HTTP call, NIP-42 AUTH or reconnect after
//! `T` is refused. `BEEKEEPER_AUTH_TAG_FILE` names a file holding the same JSON
//! as `BEEKEEPER_AUTH_TAG` (an optional trailing newline is ignored). A consumer
//! that holds an [`AuthTagSource`] and calls [`AuthTagSource::current`] at the
//! point of use sees whatever the host last wrote there.
//!
//! Precedence: the file, when it is set, readable and non-empty, then the
//! environment value. A file that is set but unreadable or empty falls back to
//! the environment value with a warning on stderr naming the path and the error
//! — never the tag. With `BEEKEEPER_AUTH_TAG_FILE` unset (or empty) nothing
//! changes: the source is exactly `BEEKEEPER_AUTH_TAG`.
//!
//! The legacy spelling `BUZZ_AUTH_TAG_FILE` needs nothing here: binaries adopt
//! `BUZZ_<X>` as `BEEKEEPER_<X>` at startup ([`crate::env_compat`]). Every list
//! that clears or fences `BEEKEEPER_AUTH_TAG` must name the file pointer too, in
//! both spellings — the file wins over the environment value, so a pointer that
//! survived a clear would override the attestation the clear protected.
//!
//! The resolution is pure ([`AuthTagSource::resolve_with`]); only
//! [`AuthTagSource::current`] touches the filesystem, the same split as
//! [`crate::model_registry_source`].

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The environment variable carrying the auth tag JSON itself.
pub const AUTH_TAG_VAR: &str = "BEEKEEPER_AUTH_TAG";

/// The environment variable naming a file that carries the auth tag JSON.
pub const AUTH_TAG_FILE_VAR: &str = "BEEKEEPER_AUTH_TAG_FILE";

/// Where this process reads its auth tag from.
///
/// Cheap to clone. `Debug` never prints the tag.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct AuthTagSource {
    file: Option<PathBuf>,
    env_value: Option<String>,
}

impl fmt::Debug for AuthTagSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthTagSource")
            .field("file", &self.file)
            .field("env_value", &self.env_value.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl AuthTagSource {
    /// Snapshot both variables from this process's environment.
    ///
    /// The file's *contents* are not read here; [`Self::current`] reads them
    /// each time it is called.
    pub fn from_env() -> Self {
        Self::new(
            std::env::var_os(AUTH_TAG_FILE_VAR).map(PathBuf::from),
            std::env::var(AUTH_TAG_VAR).ok(),
        )
    }

    /// A source with an explicit file (an empty path counts as unset) and
    /// fallback value.
    pub fn new(file: Option<PathBuf>, env_value: Option<String>) -> Self {
        Self {
            file: file.filter(|path| !path.as_os_str().is_empty()),
            env_value,
        }
    }

    /// A source that always answers `value`: no file.
    pub fn fixed(value: Option<String>) -> Self {
        Self::new(None, value)
    }

    /// The file this source re-reads, if any.
    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    /// The environment value this source falls back to.
    pub fn env_value(&self) -> Option<&str> {
        self.env_value.as_deref()
    }

    /// The auth tag JSON as of now.
    ///
    /// Re-reads the file on every call. The environment value is returned
    /// unchanged — an empty `BEEKEEPER_AUTH_TAG` stays `Some("")`, as
    /// `std::env::var` gives it — so callers keep their existing empty-value
    /// handling. A fallback warning is printed once per distinct failure, so a
    /// missing file does not repeat on every request; it is printed again if
    /// the file recovers and then fails.
    pub fn current(&self) -> Option<String> {
        let (value, warning) = self.resolve_with(|path| std::fs::read_to_string(path));
        if self.file.is_some() {
            report(warning);
        }
        value
    }

    /// [`Self::current`] with the file read injected, returning the value and
    /// the warning (if any) instead of printing it.
    pub fn resolve_with<R>(&self, read: R) -> (Option<String>, Option<String>)
    where
        R: FnOnce(&Path) -> std::io::Result<String>,
    {
        let Some(path) = self.file.as_deref() else {
            return (self.env_value.clone(), None);
        };
        let fallback = match self.env_value.as_deref() {
            Some(value) if !value.is_empty() => format!("using {AUTH_TAG_VAR}"),
            _ => format!("{AUTH_TAG_VAR} is not set either, so no auth tag is used"),
        };
        match read(path) {
            Ok(contents) => {
                let trimmed = contents.trim();
                if trimmed.is_empty() {
                    (
                        self.env_value.clone(),
                        Some(format!(
                            "{AUTH_TAG_FILE_VAR}={} is empty; {fallback}",
                            path.display()
                        )),
                    )
                } else {
                    (Some(trimmed.to_owned()), None)
                }
            }
            Err(error) => (
                self.env_value.clone(),
                Some(format!(
                    "{AUTH_TAG_FILE_VAR}={} could not be read ({error}); {fallback}",
                    path.display()
                )),
            ),
        }
    }
}

/// The auth tag JSON this process should use right now: shorthand for
/// `AuthTagSource::from_env().current()`.
pub fn current_auth_tag() -> Option<String> {
    AuthTagSource::from_env().current()
}

/// The last fallback warning printed, so a persistent failure is reported once
/// rather than on every request.
static LAST_WARNING: Mutex<Option<String>> = Mutex::new(None);

fn report(warning: Option<String>) {
    let mut last = LAST_WARNING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match warning {
        Some(message) => {
            if last.as_deref() != Some(message.as_str()) {
                eprintln!("beekeeper: warning: {message}");
                *last = Some(message);
            }
        }
        None => *last = None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    const TAG_A: &str = r#"["auth","aa","created_at<100","ss"]"#;
    const TAG_B: &str = r#"["auth","aa","created_at<200","ss"]"#;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "beekeeper-auth-tag-source-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir.join("auth-tag.json")
    }

    #[test]
    fn the_file_wins_over_the_environment_value() {
        let source = AuthTagSource::new(Some("/tag".into()), Some(TAG_A.into()));
        let (value, warning) = source.resolve_with(|_| Ok(format!("{TAG_B}\n")));
        assert_eq!(value.as_deref(), Some(TAG_B), "trailing newline stripped");
        assert_eq!(warning, None);
    }

    #[test]
    fn a_rewritten_file_is_read_again_on_the_next_use() {
        let path = temp_file("rewrite");
        std::fs::write(&path, TAG_A).expect("write a");
        let source = AuthTagSource::new(Some(path.clone()), None);
        assert_eq!(source.current().as_deref(), Some(TAG_A));
        std::fs::write(&path, format!("{TAG_B}\n")).expect("write b");
        assert_eq!(source.current().as_deref(), Some(TAG_B));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_falls_back_to_the_environment_value_with_a_warning() {
        let source = AuthTagSource::new(Some("/no/such/tag".into()), Some(TAG_A.into()));
        let (value, warning) =
            source.resolve_with(|_| Err(io::Error::new(io::ErrorKind::NotFound, "gone")));
        assert_eq!(value.as_deref(), Some(TAG_A));
        let warning = warning.expect("a warning");
        assert!(warning.contains("/no/such/tag"), "{warning}");
        assert!(warning.contains("gone"), "{warning}");
        assert!(warning.contains("using BEEKEEPER_AUTH_TAG"), "{warning}");
        assert!(!warning.contains(TAG_A), "the tag must never be logged");

        // The real filesystem path, not only the injected reader.
        assert_eq!(source.current().as_deref(), Some(TAG_A));
    }

    #[test]
    fn an_empty_file_falls_back_to_the_environment_value_with_a_warning() {
        let source = AuthTagSource::new(Some("/tag".into()), Some(TAG_A.into()));
        let (value, warning) = source.resolve_with(|_| Ok(" \n".into()));
        assert_eq!(value.as_deref(), Some(TAG_A));
        assert!(warning.expect("a warning").contains("is empty"));

        let bare = AuthTagSource::new(Some("/tag".into()), None);
        let (value, warning) = bare.resolve_with(|_| Ok(String::new()));
        assert_eq!(value, None);
        assert!(warning.expect("a warning").contains("is not set either"));
    }

    #[test]
    fn without_a_file_the_source_is_exactly_the_environment_value() {
        for env in [None, Some(String::new()), Some(TAG_A.to_owned())] {
            let source = AuthTagSource::new(None, env.clone());
            let (value, warning) = source.resolve_with(|_| panic!("no file to read"));
            assert_eq!(value, env);
            assert_eq!(warning, None);
            assert_eq!(source.current(), env);
        }
        // An empty pointer is no pointer.
        let empty = AuthTagSource::new(Some(PathBuf::new()), Some(TAG_A.into()));
        assert_eq!(empty.file(), None);
        assert_eq!(empty.current().as_deref(), Some(TAG_A));
    }

    #[test]
    fn debug_never_prints_the_tag() {
        let source = AuthTagSource::new(Some("/tag".into()), Some(TAG_A.into()));
        let rendered = format!("{source:?}");
        assert!(!rendered.contains("created_at"), "{rendered}");
        assert!(rendered.contains("/tag"), "{rendered}");
    }
}
