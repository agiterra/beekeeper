fn main() {
    git_credential_nostr::legacy_env::adopt_legacy_env("git-credential-nostr");
    std::process::exit(git_credential_nostr::run());
}
