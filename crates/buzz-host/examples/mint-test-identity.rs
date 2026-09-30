//! Mint a throwaway provider identity for the host acceptance script.
//!
//! An example rather than a test helper: `scripts/host-acceptance.sh` needs a
//! keypair from outside the test harness, and the alternative — hard-coding
//! one in a shell script — puts a real-looking nsec in the repository.
fn main() {
    use nostr::ToBech32;
    let keys = nostr::Keys::generate();
    println!(
        "{}",
        serde_json::json!({
            "nsec": keys.secret_key().to_bech32().expect("nsec"),
            "pubkey": keys.public_key().to_hex(),
        })
    );
}
