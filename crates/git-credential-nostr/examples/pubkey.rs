//! Print the hex public key for a Nostr key file.
//!
//! Operator tooling for provisioning machine identities (e.g. the forge's
//! mirror-bridge key), where `bee git status` is not installed. The key file
//! holds an `nsec1...` or 64-char hex secret, same as `nostr.keyfile`.
//!
//! ```sh
//! cargo run -p git-credential-nostr --example pubkey -- /home/git/.nostr/key
//! ```

use nostr::Keys;
use zeroize::Zeroize;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: pubkey <keyfile>");
        std::process::exit(2);
    };
    let mut raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            std::process::exit(1);
        }
    };
    let keys = Keys::parse(raw.trim());
    raw.zeroize();
    match keys {
        Ok(keys) => println!("{}", keys.public_key().to_hex()),
        Err(e) => {
            eprintln!("invalid key in {path}: {e}");
            std::process::exit(1);
        }
    }
}
