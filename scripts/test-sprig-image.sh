#!/usr/bin/env bash
set -euo pipefail

IMAGE="${1:-buzz-sprig:contract-test}"
if [[ "${SKIP_BUILD:-0}" != 1 ]]; then
    docker build --file Dockerfile.sprig --tag "$IMAGE" .
fi

assert_run() {
    docker run --rm --entrypoint /bin/bash "$IMAGE" -ceu "$1"
}

assert_run '
  command -v bash git update-ca-certificates >/dev/null
  for name in beekeeper-acp beekeeper-agent beekeeper-dev-mcp \
              buzz-acp buzz-agent buzz-dev-mcp \
              rg tree bee buzz git-credential-nostr git-sign-nostr; do
    test "$(readlink "/usr/local/bin/$name")" = sprig
  done
  test "$(git config --system gpg.x509.program)" = /usr/local/bin/git-sign-nostr
  ! git config --system --get-all credential.helper
  test "$HOME" = /home/agent
  test "$(pwd)" = /home/agent
'

assert_run '
  grep -Eq "^[[:space:]]*exec beekeeper-acp" /usr/local/bin/sprig-entrypoint
  ! grep -Eq "^[[:space:]]*(beekeeper-acp|bash -c .*beekeeper-acp)" /usr/local/bin/sprig-entrypoint
'

# Both spellings of the relay URL: a backend built before the rename sends
# BUZZ_RELAY_URL.
for relay_var in BEEKEEPER_RELAY_URL BUZZ_RELAY_URL; do
docker run --rm --entrypoint /bin/bash \
  -e "$relay_var=wss://relay.example.test/" "$IMAGE" -ceu '
    /usr/local/bin/sprig-entrypoint --help >/dev/null 2>&1 & pid=$!
    for _ in 1 2 3 4 5; do
      git config --global --get credential.https://relay.example.test/git.helper >/dev/null 2>&1 && break
      sleep 0.1
    done
    test "$(git config --global --get credential.https://relay.example.test/git.helper)" = /usr/local/bin/git-credential-nostr
    test "$(git config --global --get credential.https://relay.example.test/git.useHttpPath)" = true
    ! git config --global --get-all credential.helper
    wait "$pid" || true
  '
done

echo "PASS: Sprig image runtime contract ($IMAGE)"
