//! Child environments across the `BUZZ_*` → `BEEKEEPER_*` rename.

/// Give every `BEEKEEPER_<X>` this command sets or removes its `BUZZ_<X>`
/// twin, so the spawned harness works whichever spelling it was built for.
///
/// Call it **last**, after every other env write: it reads what the command
/// holds at that moment.
///
/// - A variable set to a value is mirrored under the legacy name (while
///   `EMIT_LEGACY_ENV_NAMES` holds), so a harness built before the rename,
///   an old `bee` on `PATH`, still finds it.
/// - A variable removed has its legacy twin removed too, always. Otherwise a
///   stale inherited `BUZZ_AUTH_TAG` would survive the clear and the child
///   would adopt it as `BEEKEEPER_AUTH_TAG`.
///
/// A legacy name the command already sets or removes explicitly is left as
/// the caller chose it.
pub(crate) fn apply_legacy_env_names(command: &mut std::process::Command) {
    use beekeeper_core_pkg::env_compat::{legacy_twin, EMIT_LEGACY_ENV_NAMES};
    let explicit: Vec<(String, Option<std::ffi::OsString>)> = command
        .get_envs()
        .filter_map(|(key, value)| Some((key.to_str()?.to_owned(), value.map(Into::into))))
        .collect();
    let named = |name: &str| explicit.iter().any(|(key, _)| key == name);
    let mut set = Vec::new();
    let mut remove = Vec::new();
    for (key, value) in &explicit {
        let Some(twin) = legacy_twin(key).filter(|twin| !named(twin)) else {
            continue;
        };
        match value {
            Some(value) if EMIT_LEGACY_ENV_NAMES => set.push((twin, value.clone())),
            Some(_) => {}
            None => remove.push(twin),
        }
    }
    for (key, value) in set {
        command.env(key, value);
    }
    for key in remove {
        command.env_remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::apply_legacy_env_names;

    fn env_of(cmd: &std::process::Command, key: &str) -> Option<Option<String>> {
        cmd.get_envs()
            .find(|(found, _)| *found == std::ffi::OsStr::new(key))
            .map(|(_, value)| value.map(|v| v.to_string_lossy().into_owned()))
    }

    #[test]
    fn legacy_names_mirror_sets_and_follow_removals() {
        let mut cmd = std::process::Command::new("true");
        cmd.env("BEEKEEPER_RELAY_URL", "wss://relay")
            .env_remove("BEEKEEPER_AUTH_TAG")
            .env("BEEKEEPER_ACP_MODEL", "new")
            .env("BUZZ_ACP_MODEL", "explicit")
            .env("PATH", "/bin");

        apply_legacy_env_names(&mut cmd);

        assert_eq!(
            env_of(&cmd, "BUZZ_RELAY_URL"),
            Some(Some("wss://relay".into()))
        );
        assert_eq!(
            env_of(&cmd, "BUZZ_AUTH_TAG"),
            Some(None),
            "a removed name's legacy twin must be removed too"
        );
        assert_eq!(
            env_of(&cmd, "BUZZ_ACP_MODEL"),
            Some(Some("explicit".into())),
            "an explicit legacy entry is the caller's choice"
        );
        assert_eq!(env_of(&cmd, "BUZZ_PATH"), None);
    }
}
