//! Read the NIP-OA auth tag from `BEEKEEPER_AUTH_TAG_FILE`, else
//! `BEEKEEPER_AUTH_TAG`.
//!
//! A deliberately small local copy of
//! `beekeeper_core::auth_tag_source::AuthTagSource::current`: this crate does
//! not otherwise depend on `beekeeper-core`, and one read is not worth the
//! dependency. Keep the behaviour identical to that function — the file wins
//! when it is set, readable and non-empty (surrounding whitespace, such as a
//! trailing newline, is ignored); an unreadable or empty file falls back to the
//! environment value with a warning on stderr naming the path and the error,
//! never the tag. With the file variable unset or empty nothing changes.
//!
//! This binary is started fresh for every git operation, so reading once per
//! process is reading at every use: a tag the host renews in the file reaches
//! the next push or signature without restarting anything.

use std::path::Path;

/// The environment variable naming a file that carries the auth tag JSON.
pub const AUTH_TAG_FILE_VAR: &str = "BEEKEEPER_AUTH_TAG_FILE";

/// The environment variable carrying the auth tag JSON itself.
pub const AUTH_TAG_VAR: &str = "BEEKEEPER_AUTH_TAG";

/// The auth tag JSON from this process's environment, as of now. Prints the
/// fallback warning, if any, on stderr.
pub fn env_auth_tag() -> Option<String> {
    let file = std::env::var_os(AUTH_TAG_FILE_VAR);
    let (value, warning) = resolve(
        file.as_deref().map(Path::new),
        std::env::var(AUTH_TAG_VAR).ok(),
        |path| std::fs::read_to_string(path),
    );
    if let Some(warning) = warning {
        eprintln!("warning: {warning}");
    }
    value
}

/// The pure part of [`env_auth_tag`]: the value and the warning to print.
pub fn resolve<R>(
    file: Option<&Path>,
    env_value: Option<String>,
    read: R,
) -> (Option<String>, Option<String>)
where
    R: FnOnce(&Path) -> std::io::Result<String>,
{
    let Some(path) = file.filter(|path| !path.as_os_str().is_empty()) else {
        return (env_value, None);
    };
    let fallback = match env_value.as_deref() {
        Some(value) if !value.is_empty() => format!("using {AUTH_TAG_VAR}"),
        _ => format!("{AUTH_TAG_VAR} is not set either, so no auth tag is used"),
    };
    match read(path) {
        Ok(contents) if !contents.trim().is_empty() => (Some(contents.trim().to_owned()), None),
        Ok(_) => (
            env_value,
            Some(format!(
                "{AUTH_TAG_FILE_VAR}={} is empty; {fallback}",
                path.display()
            )),
        ),
        Err(error) => (
            env_value,
            Some(format!(
                "{AUTH_TAG_FILE_VAR}={} could not be read ({error}); {fallback}",
                path.display()
            )),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::resolve;
    use std::io;
    use std::path::Path;

    const TAG: &str = r#"["auth","aa","","ss"]"#;

    #[test]
    fn the_file_wins_and_a_missing_or_empty_file_falls_back_with_a_warning() {
        let file = Some(Path::new("/tag"));
        let (value, warning) = resolve(file, Some("env".into()), |_| Ok(format!("{TAG}\n")));
        assert_eq!((value.as_deref(), warning), (Some(TAG), None));

        let (value, warning) = resolve(file, Some("env".into()), |_| {
            Err(io::Error::new(io::ErrorKind::NotFound, "gone"))
        });
        assert_eq!(value.as_deref(), Some("env"));
        let warning = warning.expect("warning");
        assert!(
            warning.contains("/tag") && warning.contains("gone"),
            "{warning}"
        );

        let (value, warning) = resolve(file, None, |_| Ok("\n".into()));
        assert_eq!(value, None);
        assert!(warning.expect("warning").contains("is empty"));
    }

    #[test]
    fn without_a_file_the_environment_value_is_returned_unchanged() {
        for env in [None, Some(String::new()), Some(TAG.to_owned())] {
            assert_eq!(
                resolve(None, env.clone(), |_| unreachable!()),
                (env.clone(), None)
            );
            assert_eq!(
                resolve(Some(Path::new("")), env.clone(), |_| unreachable!()),
                (env, None)
            );
        }
    }
}
