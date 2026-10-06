#!/usr/bin/env bash
# Set up the forge (incus container `forge` on agincus) to mirror
# hive -> local bare repo -> GitHub, flipping the old GitHub-pull direction
# for the beekeeper repo. Idempotent — safe to re-run after each manual step.
#
# Run from a repo checkout on a machine with ssh access to agincus:
#
#   ./scripts/forge/setup-hive-mirror.sh
#
# It derives the hive repo URL from this clone's `origin` fetch URL (never
# hard-codes a remote name or host), installs the direction-aware
# git-mirror-update, builds git-credential-nostr (+ the keyfile-pubkey tool)
# in a rust:alpine container on the forge's docker daemon, and configures the
# `git` user's credential helper. The sync direction is only flipped once the
# operator-created key file exists AND an authenticated fetch from hive
# succeeds; until then the script prints exactly what is still missing.
#
# The key file (/home/git/.nostr/key, 0600, nsec1... or 64-hex) is the
# operator's to create — this script writes no key material.
#
# The GitHub push authenticates as a GitHub App, not a deploy key: GitHub
# names a deploy-key push's sender as whoever added the key, and Woodpecker
# labels every pipeline with that sender — so every bridged push read as one
# person's. An app's pushes read as `<app-slug>[bot]`. The app needs
# Contents and Workflows read/write on the repo (Workflows, or any push
# touching .github/workflows is refused). Its private key
# (/home/git/.github-app/key.pem, 0600, owned by git) is the operator's to
# place, like the Nostr key. Pass the app's ids on the first run; they are
# kept in the git user's config after that:
#
#   GITHUB_APP_ID=<id> GITHUB_APP_INSTALLATION_ID=<id> \
#     ./scripts/forge/setup-hive-mirror.sh
set -euo pipefail

FORGE_HOST=agincus
FORGE_EXEC=(ssh "$FORGE_HOST" incus exec forge --)
KEYFILE=/home/git/.nostr/key
APP_KEYFILE=/home/git/.github-app/key.pem
MIRROR_REPO=/srv/git/beekeeper.git

# ---------------------------------------------------------------- remote part
if [[ "${1:-}" == "--remote" ]]; then
    HIVE_URL="$2"
    RELAY_SCOPE="$3"
    APP_ID="${4:-}"
    APP_INSTALLATION_ID="${5:-}"
    # The script arrives on stdin (bash -s); a credential prompt would read
    # the rest of it as a username. Fail instead.
    export GIT_TERMINAL_PROMPT=0

    echo "== git version =="
    ver=$(git --version | sed -E 's/git version ([0-9]+)\.([0-9]+).*/\1 \2/')
    read -r maj min <<<"$ver"
    if [ "$maj" -lt 2 ] || { [ "$maj" -eq 2 ] && [ "$min" -lt 46 ]; }; then
        echo "git $(git --version) < 2.46 — upgrading via ppa:git-core/ppa"
        apt-get update -qq
        apt-get install -y -qq software-properties-common >/dev/null
        add-apt-repository -y ppa:git-core/ppa >/dev/null
        apt-get update -qq
        apt-get install -y -qq git >/dev/null
    fi
    git --version

    echo "== build git-credential-nostr, nostr-keyfile-pubkey, buzz-mirror-bridge =="
    if [ ! -x /usr/local/bin/git-credential-nostr ] \
        || [ ! -x /usr/local/bin/nostr-keyfile-pubkey ] \
        || [ ! -x /usr/local/bin/buzz-mirror-bridge ]; then
        rm -rf /tmp/bridge-build
        git -c safe.directory="$MIRROR_REPO" clone -q "$MIRROR_REPO" /tmp/bridge-build
        if [ ! -d /tmp/bridge-build/crates/beekeeper-mirror-bridge ]; then
            echo "mirror clone predates the bridge crate — land the bridge" >&2
            echo "commit on main and let the mirror update, then re-run" >&2
            exit 1
        fi
        docker run --rm -v /tmp/bridge-build:/src -w /src rust:alpine sh -c \
            'apk add -q --no-progress musl-dev build-base perl git && \
             cargo build -q --release -p git-credential-nostr && \
             cargo build -q --release -p git-credential-nostr --example pubkey && \
             cargo build -q --release -p beekeeper-mirror-bridge'
        install -m755 /tmp/bridge-build/target/release/git-credential-nostr /usr/local/bin/
        install -m755 /tmp/bridge-build/target/release/examples/pubkey /usr/local/bin/nostr-keyfile-pubkey
        install -m755 /tmp/bridge-build/target/release/buzz-mirror-bridge /usr/local/bin/
        rm -rf /tmp/bridge-build
    fi
    echo "helper: $(ls -l /usr/local/bin/git-credential-nostr | awk '{print $NF}')"

    echo "== git user credential config =="
    install -d -m700 -o git -g git /home/git/.nostr
    # -C /home/git: this script's cwd is /root, which user git cannot read —
    # git 2.55's repo discovery makes even `config --global` fatal there.
    sudo -u git git -C /home/git config --global "credential.${RELAY_SCOPE}.helper" nostr
    sudo -u git git -C /home/git config --global "credential.${RELAY_SCOPE}.useHttpPath" true
    sudo -u git git -C /home/git config --global nostr.keyfile "$KEYFILE"

    echo "== hive remote on $MIRROR_REPO =="
    if sudo -u git git -C "$MIRROR_REPO" config --get remote.hive.url >/dev/null 2>&1; then
        sudo -u git git -C "$MIRROR_REPO" remote set-url hive "$HIVE_URL"
    else
        sudo -u git git -C "$MIRROR_REPO" remote add hive "$HIVE_URL"
    fi
    # Keep `git remote update` (the default mode for other repos, and the
    # pre-flip hourly run here) from tripping over the not-yet-authenticated
    # hive remote; the mirror script fetches it explicitly.
    sudo -u git git -C "$MIRROR_REPO" config remote.hive.skipFetchAll true

    if ! sudo -u git test -f "$KEYFILE"; then
        cat <<EOF

Setup is staged. Still missing: the key file. As the operator, run

  incus exec forge -- sudo -u git sh -c \\
    'umask 077; printf "%s\n" "<nsec1-or-64-hex-secret>" > $KEYFILE'

then re-run this script; it will print the pubkey to add as a community
member and, once a fetch works, flip the mirror direction.
EOF
        exit 0
    fi

    echo "== key present — pubkey =="
    PUBKEY=$(sudo -u git /usr/local/bin/nostr-keyfile-pubkey "$KEYFILE")
    echo "forge mirror pubkey: $PUBKEY"

    echo "== authenticated fetch test =="
    if ! sudo -u git git -C "$MIRROR_REPO" fetch --dry-run hive \
            '+refs/heads/*:refs/heads/*' 2>&1; then
        cat <<EOF

Fetch from hive failed. Most likely the pubkey above is not yet a member of
the community. From the admin box:

  ssh agincus "incus exec hive -- docker exec buzz-prod-relay-1 \\
    buzz-admin add-member --pubkey $PUBKEY --role member"

If it then fails with "not a project member or channel member", the repo's
read gate also wants channel/project membership — add the pubkey to the
repo's channel, then re-run this script.
EOF
        exit 1
    fi

    echo "== GitHub App credential =="
    gitcfg() { sudo -u git git -C /home/git config --global "$@"; }
    [ -n "$APP_ID" ] && gitcfg githubApp.appId "$APP_ID"
    [ -n "$APP_INSTALLATION_ID" ] && gitcfg githubApp.installationId "$APP_INSTALLATION_ID"
    gitcfg githubApp.keyFile "$APP_KEYFILE"
    gitcfg credential.https://github.com.helper github-app
    missing=""
    gitcfg --get githubApp.appId >/dev/null || missing="$missing GITHUB_APP_ID"
    gitcfg --get githubApp.installationId >/dev/null \
        || missing="$missing GITHUB_APP_INSTALLATION_ID"
    sudo -u git test -r "$APP_KEYFILE" || missing="$missing $APP_KEYFILE"
    if [ -n "$missing" ]; then
        echo "GitHub App setup incomplete — missing:$missing" >&2
        echo "See the header of this script; nothing was flipped." >&2
        exit 1
    fi
    # Every repo that re-publishes to GitHub (mirror.pushRemote set; this
    # repo before its first flip, below, pushes to origin) moves from an ssh
    # remote (the deploy-key era's `git@<alias>:org/repo.git`) to the https
    # URL the credential helper answers for. The app must be installed on
    # each such repo, or its push fails with 403. Idempotent.
    for repo in /srv/git/*.git; do
        push=$(sudo -u git git -C "$repo" config --get mirror.pushRemote || true)
        [ -z "$push" ] && [ "$repo" = "$MIRROR_REPO" ] && push=origin
        [ -n "$push" ] || continue
        GH_URL=$(sudo -u git git -C "$repo" remote get-url "$push")
        case "$GH_URL" in
            https://github.com/*) ;;
            git@*:*) sudo -u git git -C "$repo" remote set-url "$push" \
                         "https://github.com/${GH_URL#git@*:}" ;;
            *) echo "unexpected GitHub remote URL in $repo: $GH_URL" >&2; exit 1 ;;
        esac
        echo "$repo $push -> $(sudo -u git git -C "$repo" remote get-url "$push")"
    done

    echo "== GitHub push access test =="
    # The bare repo was cloned with --mirror; that push mode is incompatible
    # with explicit refspecs (fatal) and would prune GitHub-only refs. The
    # sync pushes explicit refspecs, so drop the mirror push semantics.
    sudo -u git git -C "$MIRROR_REPO" config remote.origin.mirror false
    if ! sudo -u git git -C "$MIRROR_REPO" push --dry-run origin \
            '+refs/heads/main:refs/heads/main' 2>&1; then
        echo
        echo "GitHub push failed — the git output above is authoritative." >&2
        echo "A 403 means the app lacks Contents: write, or is not" >&2
        echo "installed on this repo: the org's Settings > GitHub Apps." >&2
        exit 1
    fi

    echo "== flipping mirror direction (hive -> local -> GitHub) =="
    sudo -u git git -C "$MIRROR_REPO" config mirror.fetchRemote hive
    sudo -u git git -C "$MIRROR_REPO" config mirror.pushRemote origin

    echo "== first sync =="
    systemctl start git-mirror.service
    journalctl -u git-mirror.service -n 20 --no-pager | tail -12

    echo "== event-driven bridge service =="
    WS_URL="wss://$(printf '%s' "$RELAY_SCOPE" | sed -E 's#^https://([^/]+)/git#\1#')"
    # No --repo filter: the sync command already covers every mirror repo and
    # no-ops when nothing changed, whereas a wrong d-tag guess would silently
    # disarm the bridge.
    cat > /etc/systemd/system/hive-mirror-bridge.service <<EOF
[Unit]
Description=Relay ref-state (kind:30618) -> git-mirror-update bridge
After=network-online.target
Wants=network-online.target

[Service]
User=git
ExecStart=/usr/local/bin/buzz-mirror-bridge --relay $WS_URL --keyfile $KEYFILE
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF
    systemctl daemon-reload
    systemctl enable hive-mirror-bridge.service
    # restart, not `enable --now`: a previous broken install may be
    # crash-looping, and --now is a no-op on an already-enabled unit.
    systemctl restart hive-mirror-bridge.service
    sleep 3
    if ! systemctl is-active --quiet hive-mirror-bridge.service; then
        echo "bridge service failed to start:" >&2
        journalctl -u hive-mirror-bridge.service -n 20 --no-pager >&2
        exit 1
    fi
    journalctl -u hive-mirror-bridge.service -n 10 --no-pager

    # The hourly git-mirror.timer stays as the reconcile fallback; the bridge
    # provides the seconds-latency path, so no per-minute polling is needed.
    rm -f /etc/systemd/system/git-mirror.timer.d/override.conf
    rmdir /etc/systemd/system/git-mirror.timer.d 2>/dev/null || true
    systemctl daemon-reload
    systemctl restart git-mirror.timer

    echo
    echo "Done. Verify with a hive-only push; the SHA should reach GitHub"
    echo "within seconds and trigger a Woodpecker pipeline."
    exit 0
fi

# ----------------------------------------------------------------- local part
cd "$(dirname "$0")/../.."

HIVE_URL=$(git remote get-url origin)
case "$HIVE_URL" in
    https://*/git/*) ;;
    *)  echo "origin ($HIVE_URL) does not look like a hive git URL" >&2
        echo "run from a clone whose origin fetch URL is the relay" >&2
        exit 1 ;;
esac
RELAY_SCOPE="$(printf '%s' "$HIVE_URL" | sed -E 's#^(https://[^/]+/git)/.*#\1#')"

echo "hive repo URL:   $HIVE_URL"
echo "credential scope: $RELAY_SCOPE"

echo "== install git-mirror-update on forge =="
"${FORGE_EXEC[@]}" tee /usr/local/bin/git-mirror-update >/dev/null \
    < scripts/forge/git-mirror-update
"${FORGE_EXEC[@]}" chmod 755 /usr/local/bin/git-mirror-update

echo "== install git-credential-github-app on forge =="
"${FORGE_EXEC[@]}" tee /usr/local/bin/git-credential-github-app >/dev/null \
    < scripts/forge/git-credential-github-app
"${FORGE_EXEC[@]}" chmod 755 /usr/local/bin/git-credential-github-app

echo "== run remote setup =="
"${FORGE_EXEC[@]}" bash -s -- --remote "$HIVE_URL" "$RELAY_SCOPE" \
    "${GITHUB_APP_ID:-}" "${GITHUB_APP_INSTALLATION_ID:-}" \
    < "scripts/forge/$(basename "$0")"
