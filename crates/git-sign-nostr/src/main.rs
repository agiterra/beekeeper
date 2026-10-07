fn main() {
    git_sign_nostr::legacy_env::adopt_legacy_env("git-sign-nostr");
    std::process::exit(git_sign_nostr::run());
}
