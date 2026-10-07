fn main() {
    // Before anything else reads the environment or starts a thread: an older
    // parent (a desktop, host or Kubernetes backend built before the rename)
    // still hands this process `BUZZ_*` names.
    beekeeper_core::env_compat::adopt_legacy_env("sprig");
    if let Err(e) = dispatch() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

/// Which program a Sprig invocation runs, decided by its argv0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Acp,
    Agent,
    Sprig,
    /// The developer MCP server and every multicall name it handles itself.
    DevMcp,
}

/// Map argv0's file name to a personality.
///
/// Both spellings of each name are accepted. Images built before the rename
/// carry only the `buzz-*` links, and the Kubernetes backend still sends the
/// `buzz-*` command names to pods, because pod images are digest-pinned
/// (`crates/beekeeper-backend-kubernetes/src/env.rs`, `pod_command_name`).
/// Retire the `buzz-*` arms once every supported sprig image ships the
/// `beekeeper-*` names and the backend sends them.
fn personality(argv0: &str) -> Personality {
    let cmd = std::path::Path::new(argv0)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match cmd.as_str() {
        "beekeeper-acp" | "buzz-acp" => Personality::Acp,
        "beekeeper-agent" | "buzz-agent" => Personality::Agent,
        "sprig" => Personality::Sprig,
        // `beekeeper-dev-mcp` and `buzz-dev-mcp` land here, as do rg, tree,
        // bee, git-credential-nostr and git-sign-nostr.
        _ => Personality::DevMcp,
    }
}

fn dispatch() -> Result<(), String> {
    let argv0 = std::env::args().next().unwrap_or_default();

    match personality(&argv0) {
        Personality::Acp => beekeeper_acp::run().map_err(|e| e.to_string()),
        Personality::Agent => beekeeper_agent::run().map_err(|e| e.to_string()),
        Personality::Sprig => match std::env::args().nth(1).as_deref() {
            Some("-V") | Some("--version") => {
                println!("sprig {}", env!("CARGO_PKG_VERSION"));
                Ok(())
            }
            Some("-h") | Some("--help") | None => {
                print_usage();
                if std::env::args().len() <= 1 {
                    Err("error: invoke Sprig via a personality symlink".into())
                } else {
                    Ok(())
                }
            }
            Some(other) => {
                print_usage();
                Err(format!(
                    "error: unknown Sprig option or personality: {other}"
                ))
            }
        },
        // beekeeper-dev-mcp also handles its own multicall names.
        Personality::DevMcp => beekeeper_dev_mcp::run().map_err(|e| e.to_string()),
    }
}

fn print_usage() {
    println!(
        "Sprig — all-in-one Beekeeper ACP harness, agent, and developer MCP\n\n\
Sprig is a multicall binary. Invoke it through one of the personality names:\n\n\
  beekeeper-acp       ACP harness\n  beekeeper-agent     ACP-compliant agent\n  beekeeper-dev-mcp   Developer MCP server\n\n\
The names from before the rename (buzz-acp, buzz-agent, buzz-dev-mcp) still work.\n\
Developer MCP helper names are also supported: rg, tree, bee, git-credential-nostr, git-sign-nostr.\n\n\
Installers can create links with:\n  ln -s sprig beekeeper-acp\n  ln -s sprig beekeeper-agent\n  ln -s sprig beekeeper-dev-mcp"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_spellings_of_each_personality_dispatch_alike() {
        for (new, old, expected) in [
            ("beekeeper-acp", "buzz-acp", Personality::Acp),
            ("beekeeper-agent", "buzz-agent", Personality::Agent),
            ("beekeeper-dev-mcp", "buzz-dev-mcp", Personality::DevMcp),
        ] {
            assert_eq!(personality(new), expected, "{new}");
            assert_eq!(personality(old), expected, "{old}");
            assert_eq!(
                personality(&format!("/usr/local/bin/{old}")),
                expected,
                "a full path to {old}"
            );
            assert_eq!(
                personality(&new.to_ascii_uppercase()),
                expected,
                "case-insensitive {new}"
            );
        }
    }

    #[test]
    fn sprig_itself_and_helper_names_dispatch_as_before() {
        assert_eq!(personality("/usr/local/bin/sprig"), Personality::Sprig);
        for helper in [
            "rg",
            "tree",
            "bee",
            "git-credential-nostr",
            "git-sign-nostr",
            "",
        ] {
            assert_eq!(personality(helper), Personality::DevMcp, "{helper}");
        }
    }
}
