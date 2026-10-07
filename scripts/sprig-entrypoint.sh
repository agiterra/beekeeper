#!/bin/bash
set -euo pipefail

# Match desktop's URL-scoped git credential configuration without installing a
# helper globally (which would make it answer for unrelated remotes).
# BUZZ_RELAY_URL is the pre-rename spelling, which a Kubernetes backend built
# before the rename still sends; the harness itself adopts it at startup.
relay_url="${BEEKEEPER_RELAY_URL:-${BUZZ_RELAY_URL:-}}"
if [[ -n "$relay_url" ]]; then
    relay_http_url="${relay_url/#ws:/http:}"
    relay_http_url="${relay_http_url/#wss:/https:}"
    relay_http_url="${relay_http_url%/}"
    git config --global "credential.${relay_http_url}/git.helper" \
        /usr/local/bin/git-credential-nostr
    git config --global "credential.${relay_http_url}/git.useHttpPath" true
fi

# The harness must receive Kubernetes' termination signal directly.
exec beekeeper-acp "$@"
