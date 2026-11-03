//! `beekeeper-host census`: the read-only start census, from this bundle.
//!
//! Runs beside whatever Host may be live. It binds no socket, takes no lock,
//! supervises nothing and registers nothing. The sibling provider binary does the
//! reading, given the key by the route a start uses: through its environment and
//! nowhere else. Nothing here prints the key, the auth tag or the files it compares.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use beekeeper_core::build_info::{build_info, UNKNOWN_COMMIT};
use beekeeper_host_core::layout::{self, Instance};
use serde_json::{json, Value};

use crate::spawn::{provider_command, PROVIDER_BINARY};

/// Complete. Rows may be present; `EXIT_INCOMPLETE` means the list cannot be trusted.
const EXIT_INCOMPLETE: i32 = 20;
/// Refused before any query.
const EXIT_REFUSED: i32 = 30;
const DEADLINE: Duration = Duration::from_secs(900);

fn refuse(code: &str, message: &str) -> i32 {
    println!(
        "{}",
        json!({"schema":"host-start-census/v1","exit":EXIT_REFUSED,"reason":code,"message":message})
    );
    EXIT_REFUSED
}

fn contents(path: &Path) -> Result<Option<Vec<u8>>, std::io::Error> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Whether a child's document may be passed on as it stands.
pub(crate) fn accept(
    stdout: &str,
    code: Option<i32>,
    provider: &str,
    host_commit: &str,
) -> Result<Value, (i32, &'static str)> {
    let document: Value = serde_json::from_str(stdout.trim())
        .map_err(|_| (EXIT_INCOMPLETE, "CHILD_OUTPUT_UNREADABLE"))?;
    if document["exit"].as_i64() != code.map(i64::from) {
        return Err((EXIT_INCOMPLETE, "CHILD_EXIT_DIFFERS"));
    }
    // Refused before it read anything: it has no identity to compare.
    if document["exit"] == EXIT_REFUSED {
        return Ok(document);
    }
    if document["providerPubkey"] != provider {
        return Err((EXIT_INCOMPLETE, "CHILD_IDENTITY_DIFFERS"));
    }
    // Two binaries that cannot be shown to be one build do not describe a start.
    if host_commit == UNKNOWN_COMMIT || document["commit"] != host_commit {
        return Err((EXIT_REFUSED, "BUILD_COMMIT_MISMATCH"));
    }
    Ok(document)
}

/// The caller's shell must not choose what counts as expired. Both spellings.
const HORIZON_VARS: [&str; 2] = [
    "BEEKEEPER_CSP_COMMAND_HORIZON_SECS",
    "BUZZ_CSP_COMMAND_HORIZON_SECS",
];

pub fn run(home: &Path, instance: Instance, expect_provider: Option<&str>) -> i32 {
    if HORIZON_VARS
        .iter()
        .any(|name| std::env::var_os(name).is_some())
    {
        return refuse(
            "CENSUS_HORIZON_OVERRIDE",
            "this shell sets the command horizon, which a start under the Host would not see",
        );
    }
    let Some(expected) = expect_provider else {
        return refuse(
            "EXPECT_PROVIDER_REQUIRED",
            "name the provider this census is for, with --expect-provider",
        );
    };
    let commissioned = match crate::commission::commission(home, instance) {
        Ok(commissioned) => commissioned,
        Err(_) => {
            return refuse(
                "NOT_COMMISSIONED",
                "this Host's config, record or key could not be resolved",
            )
        }
    };
    let config = &commissioned.config;
    let provider = commissioned.record.provider_pubkey.clone();
    let host_commit = build_info().commit_display();
    if expected != provider {
        return refuse(
            "WRONG_PROVIDER",
            "the commissioned provider is not the expected one",
        );
    }
    // The census describes the build about to start, which is this bundle's.
    if config.provider_command.is_some() {
        return refuse(
            "CENSUS_PROVIDER_OVERRIDE",
            "host.json names a provider binary, so this bundle's would not be the one to start",
        );
    }
    let binary = match std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(PROVIDER_BINARY)))
    {
        Some(binary) if binary.is_file() => binary,
        _ => {
            return refuse(
                "PROVIDER_NOT_BESIDE_HOST",
                "no provider binary beside this host",
            )
        }
    };
    let config_path = layout::host_config_path(home, instance);
    let store_path = config.record_store_path();
    let read = || Ok::<_, std::io::Error>((contents(&config_path)?, contents(&store_path)?));
    let Ok(before) = read() else {
        return refuse(
            "NOT_COMMISSIONED",
            "this Host's config or record could not be read",
        );
    };
    let mut command: Command = match provider_command(
        &binary,
        config,
        &commissioned.record,
        &commissioned.key.nsec,
    ) {
        Ok(command) => command,
        Err(_) => {
            return refuse(
                "NOT_COMMISSIONED",
                "the provider's environment could not be built",
            )
        }
    };
    command
        .arg("census")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            return refuse(
                "PROVIDER_NOT_BESIDE_HOST",
                "the provider binary would not start",
            )
        }
    };
    let mut pipe = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        if let Some(pipe) = pipe.as_mut() {
            let _ = pipe.read_to_string(&mut out);
        }
        out
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() < DEADLINE => {
                std::thread::sleep(Duration::from_millis(200))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let stdout = reader.join().unwrap_or_default();
    let unstable = !matches!(read(), Ok(after) if after == before);
    let outcome = match status {
        None => Err((EXIT_INCOMPLETE, "CENSUS_TIMED_OUT")),
        Some(_) if unstable => Err((EXIT_INCOMPLETE, "SNAPSHOT_UNSTABLE")),
        Some(status) => accept(&stdout, status.code(), &provider, &host_commit),
    };
    match outcome {
        Ok(mut document) => {
            document["instance"] = json!(instance.namespace_value());
            document["hostCommit"] = json!(host_commit);
            println!("{document}");
            document["exit"]
                .as_i64()
                .unwrap_or(i64::from(EXIT_INCOMPLETE)) as i32
        }
        Err((exit, reason)) => {
            println!(
                "{}",
                json!({"schema":"host-start-census/v1","instance":instance.namespace_value(),"hostCommit":host_commit,"providerPubkey":provider,"exit":exit,"reason":reason})
            );
            exit
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_passes_only_for_this_provider_build_and_exit() {
        let doc = r#"{"providerPubkey":"aa","commit":"c1","exit":10,"commands":[]}"#;
        assert_eq!(accept(doc, Some(10), "aa", "c1").unwrap()["exit"], 10);
        assert_eq!(
            accept(doc, Some(10), "bb", "c1").unwrap_err(),
            (EXIT_INCOMPLETE, "CHILD_IDENTITY_DIFFERS")
        );
        assert_eq!(
            accept(doc, Some(0), "aa", "c1").unwrap_err(),
            (EXIT_INCOMPLETE, "CHILD_EXIT_DIFFERS")
        );
        assert_eq!(
            accept("not json", Some(0), "aa", "c1").unwrap_err(),
            (EXIT_INCOMPLETE, "CHILD_OUTPUT_UNREADABLE")
        );
        assert_eq!(
            accept(doc, None, "aa", "c1").unwrap_err(),
            (EXIT_INCOMPLETE, "CHILD_EXIT_DIFFERS")
        );
        assert_eq!(
            accept(doc, Some(10), "aa", "c2").unwrap_err(),
            (EXIT_REFUSED, "BUILD_COMMIT_MISMATCH")
        );
        // A build that cannot name itself proves nothing, even matching its sibling.
        let unknown = r#"{"providerPubkey":"aa","commit":"unknown","exit":0}"#;
        assert_eq!(
            accept(unknown, Some(0), "aa", "unknown").unwrap_err(),
            (EXIT_REFUSED, "BUILD_COMMIT_MISMATCH")
        );
    }
}
