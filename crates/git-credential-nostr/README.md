# git-credential-nostr

NIP-98 credential helper for git — signs HTTP auth events with your Nostr key so git can push/pull from Buzz's git server without passwords.

## Requirements

- **git 2.46+** (requires `authtype` capability in the credential protocol)
- **Rust toolchain** (for building from source)

## Installation

```bash
cargo install --path crates/git-credential-nostr --root "$HOME/.local"
```

`--root` matters inside this repo: hermit pins `CARGO_HOME` to
`.hermit/rust`, so a bare `cargo install` puts the binary inside the working
tree — off `PATH`, and deleted by a hermit clean.

## Setup

From a Beekeeper checkout, one command does everything below:

```bash
just install-git-credentials     # or: bee git setup
```

Both write the same three entries, and `bee git status` reports whether they
actually work. To do it by hand:

```bash
# 1. Register the helper — SCOPED to the relay's git path.
git config --global credential.https://relay.example/git.helper nostr
git config --global credential.https://relay.example/git.useHttpPath true

# 2. Store your nsec in a key file (must be 0600).
mkdir -p ~/.nostr
printf '%s\n' "nsec1..." > ~/.nostr/key && chmod 600 ~/.nostr/key
git config --global nostr.keyfile ~/.nostr/key
```

That's it. Use git normally — `git clone`, `git push`, `git fetch`.

### Scope it to the relay, not to everything

An unscoped `credential.helper nostr` is consulted for **every** remote,
GitHub included. This helper does decline politely when the server never sends
a `Nostr` challenge — it prints nothing and exits 0, so git falls through to
the next helper — but that makes your local correctness depend on how a remote
third-party server behaves. Scoping to `<relay-origin>/git` keeps whatever
already serves GitHub (osxkeychain, a PAT, `gh`) untouched by construction.

`credential.useHttpPath` is set under the same scope on purpose: the helper
requires it (it needs the repo path to sign the right URL), and setting it
globally would change how credentials are matched for unrelated hosts.

## CI / CD

Set `$NOSTR_PRIVATE_KEY` instead of a key file. The env var takes precedence
over `nostr.keyfile` and avoids touching the filesystem:

```bash
export NOSTR_PRIVATE_KEY=nsec1...
git clone https://relay.example.com/git/owner/repo.git
```

## How It Works

When a Buzz git server returns `HTTP 401` with a
`WWW-Authenticate: Nostr realm="...", method="GET"` header, git calls this
helper with the request details on stdin. The helper loads your Nostr private
key, builds a [NIP-98](https://github.com/nostr-protocol/nips/blob/master/98.md)
kind-27235 event signed over the request URL and method, base64-encodes it, and
writes it back to stdout. Git then retries the request with
`Authorization: Nostr <token>`, which the server verifies by checking the event
signature.

```
git ──stdin──▶ git-credential-nostr ──stdout──▶ git
                     │
                     ▼
              sign kind:27235 event
              (NIP-98 HTTP Auth)
```

## Troubleshooting

| Error | Cause | Fix |
|-------|-------|-----|
| `no nostr key configured` | Neither `$NOSTR_PRIVATE_KEY` nor `nostr.keyfile` is set | Follow the Setup steps above |
| `insecure permissions` | Key file is readable by group/others | `chmod 600 ~/.nostr/key` |
| `method hint` | Server's `WWW-Authenticate` header is missing `method="..."` | Upgrade the Buzz server |
| `useHttpPath` | `credential.useHttpPath` is not set | `git config --global credential.useHttpPath true` |
| Empty output / no auth | git version is older than 2.46 | Upgrade git |
| `clock skew` / auth rejected | System clock is off by more than 60 s | Sync your system clock (`ntpdate`, `timedatectl`) |
