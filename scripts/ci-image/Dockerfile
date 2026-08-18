# CI base image for the Woodpecker gate/nightly rust + desktop steps.
#
# Build on the forge host's docker daemon (the one the Woodpecker agent uses);
# a locally-tagged versioned image is used without pulling:
#
#   ssh agincus "incus exec forge -- docker build -t buzz-ci:1 -" \
#     < scripts/ci-image/Dockerfile
#
# (Lives under scripts/, not .woodpecker/ — Woodpecker's config fetcher treats
# everything under .woodpecker/ as pipeline YAML and errors on subdirectories.)
#
# No Rust toolchain is baked in: hermit + the mounted /srv/ci-cache volumes own
# toolchains, so rust-toolchain.toml / hermit bumps need NO image rebuild.
# Rebuild (and bump the tag: buzz-ci:2, update .woodpecker/*.yml) only when the
# apt dependency set changes. See docs/INTEGRATION.md.
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
# Union of the rust and desktop steps' apt deps, plus mold for optional fast
# linking (not enabled by default — see INTEGRATION.md).
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl git unzip xz-utils libatomic1 \
      build-essential cmake perl clang mold pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
