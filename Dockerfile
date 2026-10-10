# syntax=docker/dockerfile:1.7
#
# Beekeeper relay image. The relay host builds it from this file with a plain
# `docker build` (deploy/autodeploy); it is not published to a registry.
#
# Builds the `beekeeper-relay` binary (Rust 1.95) and the web static bundles
# (pnpm + vite), then assembles them into a small debian-slim runtime with
# `git` available (the relay shells out to git for repo hydrate / receive-pack
# / upload-pack — see crates/beekeeper-relay/src/api/git).
#
# The Dockerfile is platform-agnostic: it builds natively for whatever host
# runs it (amd64 or arm64). Do not add --platform pins.

ARG RUST_VERSION=1.95
ARG NODE_VERSION=24
ARG DEBIAN_VERSION=bookworm

# Optional extra CA bundle for builds behind a TLS-intercepting corporate proxy
# (e.g. a Cloudflare/Zscaler gateway that re-signs TLS). Empty by default, so
# public CI builds are unaffected. Point it at a PEM file in the build context:
#   docker build --build-arg EXTRA_CA_CERTS=path/to/proxy-ca.pem ...
# Consumed by the network-touching stages below (cargo + pnpm).
ARG EXTRA_CA_CERTS=

# Optional npm registry for builds where the public registry is unreachable or
# policy-blocked (e.g. a corporate mirror / Artifactory). Empty default = public
# npmjs, so public CI builds are unaffected. Consumed by the web-builder stage.
ARG NPM_REGISTRY=

# ─── Stage 1: cargo-chef base ───────────────────────────────────────────────
FROM rust:${RUST_VERSION}-${DEBIAN_VERSION} AS chef
# Trust an optional corporate-proxy CA before any network fetch (no-op if unset).
ARG EXTRA_CA_CERTS
COPY --chmod=0644 ${EXTRA_CA_CERTS:-Dockerfile} /tmp/extra-ca/src
RUN if [ -n "${EXTRA_CA_CERTS}" ]; then \
        cp /tmp/extra-ca/src /usr/local/share/ca-certificates/extra-proxy-ca.crt \
        && update-ca-certificates \
        && echo "CARGO_HTTP_CAINFO=/etc/ssl/certs/ca-certificates.crt" >> /etc/environment; \
    fi
ENV CARGO_HTTP_CAINFO=/etc/ssl/certs/ca-certificates.crt
RUN cargo install cargo-chef --locked --version 0.1.71
WORKDIR /build

# ─── Stage 2: plan dependency graph ─────────────────────────────────────────
# Only the manifests are needed to compute the recipe; this layer rebuilds
# only when Cargo.{toml,lock} or crate manifests change, not on every source
# edit.
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ─── Stage 3: cook dependencies, then build the binary ──────────────────────
FROM chef AS builder
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        pkg-config \
        libssl-dev \
        ca-certificates \
        git \
    && rm -rf /var/lib/apt/lists/*
# Keep enough DWARF for native profilers to resolve optimized code to source
# locations. The normal runtime strips it below; runtime-debug retains it.
ENV CARGO_PROFILE_RELEASE_DEBUG=line-tables-only
COPY --from=planner /build/recipe.json recipe.json
# Cook the full workspace recipe — relay deps include workspace siblings, so
# scoping to -p beekeeper-relay misses transitive deps and re-builds them later.
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
# Compile immutable artifact identity into the relay. Defaults preserve local
# and third-party builds that do not run in provenance-aware CI.
#
# Each value is accepted under both names: BEEKEEPER_<X> is current, and
# BUZZ_<X> is what a deployer installed before the rename still passes
# (`--build-arg BUZZ_SOURCE_SHA=...`). The BEEKEEPER_ name wins when both are
# set and non-empty; the defaults sit on the BUZZ_ ARGs so either name alone
# overrides them.
ARG BEEKEEPER_SOURCE_SHA=
ARG BUZZ_SOURCE_SHA=unknown
# Empty, not `unknown`: the count is numeric, and empty is the value
# `parse_commit_count` refuses to a disclosed `null`. Must describe the same
# commit as BEEKEEPER_SOURCE_SHA — build.rs pairs them or drops the count.
ARG BEEKEEPER_SOURCE_COMMIT_COUNT=
ARG BUZZ_SOURCE_COMMIT_COUNT=
ARG BEEKEEPER_BUILD_ID=
ARG BUZZ_BUILD_ID=local
ARG BEEKEEPER_BUILD_URL=
ARG BUZZ_BUILD_URL=unknown
ENV BEEKEEPER_SOURCE_SHA=${BEEKEEPER_SOURCE_SHA:-$BUZZ_SOURCE_SHA} \
    BEEKEEPER_SOURCE_COMMIT_COUNT=${BEEKEEPER_SOURCE_COMMIT_COUNT:-$BUZZ_SOURCE_COMMIT_COUNT} \
    BEEKEEPER_BUILD_ID=${BEEKEEPER_BUILD_ID:-$BUZZ_BUILD_ID} \
    BEEKEEPER_BUILD_URL=${BEEKEEPER_BUILD_URL:-$BUZZ_BUILD_URL}
RUN cargo build --release --locked -p beekeeper-relay --bin beekeeper-relay \
                                   -p beekeeper-admin --bin beekeeper-admin \
                                   -p beekeeper-pair-relay --bin beekeeper-pair-relay

# Derive the normal release binaries from the same optimized ELF files as the
# debug image so the two variants cannot drift at code-generation time.
FROM builder AS stripped-binaries
RUN strip target/release/beekeeper-relay \
    && strip target/release/beekeeper-admin \
    && strip target/release/beekeeper-pair-relay

# ─── Stage 4: web bundle (pnpm + vite) ──────────────────────────────────────
# Independent of the Rust layers so a CSS change doesn't bust Rust cache and
# vice versa.
FROM node:${NODE_VERSION}-${DEBIAN_VERSION}-slim AS web-builder
WORKDIR /build
# Trust an optional corporate-proxy CA so corepack + pnpm can fetch over an
# intercepting TLS gateway (no-op if EXTRA_CA_CERTS is unset).
ARG EXTRA_CA_CERTS
COPY --chmod=0644 ${EXTRA_CA_CERTS:-Dockerfile} /tmp/extra-ca/src
RUN if [ -n "${EXTRA_CA_CERTS}" ]; then \
        apt-get update && apt-get install -y --no-install-recommends ca-certificates \
        && cp /tmp/extra-ca/src /usr/local/share/ca-certificates/extra-proxy-ca.crt \
        && update-ca-certificates \
        && rm -rf /var/lib/apt/lists/*; \
    fi
ENV NODE_EXTRA_CA_CERTS=/etc/ssl/certs/ca-certificates.crt
# Point npm + corepack at an optional mirror (no-op when NPM_REGISTRY is unset).
# corepack reads COREPACK_NPM_REGISTRY to fetch the pinned pnpm; pnpm/npm read
# the .npmrc registry for dependency installs.
ARG NPM_REGISTRY
ENV COREPACK_NPM_REGISTRY=${NPM_REGISTRY}
# When using a mirror, disable corepack's npmjs signature check: the mirror
# republishes tarballs without the public registry's provenance signatures, so
# strict verification fails ("No compatible signature found"). Only relaxed on
# the mirror path — public builds (NPM_REGISTRY unset) keep strict verification.
RUN if [ -n "${NPM_REGISTRY}" ]; then \
        echo "registry=${NPM_REGISTRY}" > /build/.npmrc \
        && echo "COREPACK_INTEGRITY_KEYS=0" >> /etc/environment; \
    fi
ENV COREPACK_INTEGRITY_KEYS=${NPM_REGISTRY:+0}
RUN corepack enable
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY patches/ patches/
COPY web/package.json web/
COPY admin-web/package.json admin-web/
RUN pnpm install --frozen-lockfile --filter beekeeper-web --filter beekeeper-admin-web
COPY web/ web/
COPY admin-web/ admin-web/
RUN pnpm -C web build && pnpm -C admin-web build

# ─── Stage 5: shared runtime ────────────────────────────────────────────────
FROM debian:${DEBIAN_VERSION}-slim AS runtime-base

# OCI annotations. org.opencontainers.image.source is the one registries (GHCR
# among them) use to link an image to its repository, should it ever be pushed.
LABEL org.opencontainers.image.title="Beekeeper" \
      org.opencontainers.image.description="WebSocket relay server for the Beekeeper communications platform" \
      org.opencontainers.image.source="https://github.com/agiterra/beekeeper" \
      org.opencontainers.image.url="https://github.com/agiterra/beekeeper" \
      org.opencontainers.image.documentation="https://github.com/agiterra/beekeeper#readme" \
      org.opencontainers.image.licenses="Apache-2.0"

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        git \
        openssl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 1000 buzz \
    && useradd  --system --uid 1000 --gid 1000 --home-dir /var/lib/buzz \
                --create-home --shell /usr/sbin/nologin buzz

COPY --from=web-builder /build/web/dist                 /srv/buzz/web
COPY --from=web-builder /build/admin-web/dist           /srv/buzz/admin-web

# The invite landing page is always served from the bundled web UI. Repository
# browser routes require the separate BEEKEEPER_SERVE_GIT_WEB_GUI=true opt-in. The
# admin bundle is inert until BEEKEEPER_ADMIN_HOST is configured.
#
# These two defaults are spelled BUZZ_* on purpose, for the transition: an image
# default is the weakest value, and under the new spelling it would shadow an
# operator's legacy BUZZ_WEB_DIR / BUZZ_ADMIN_WEB_DIR from .env (the relay
# prefers a set BEEKEEPER_* name). Spelled this way, an operator's value of
# either spelling still wins, and the relay adopts this default as
# BEEKEEPER_WEB_DIR when nothing else is set.
ENV BUZZ_WEB_DIR=/srv/buzz/web \
    BUZZ_ADMIN_WEB_DIR=/srv/buzz/admin-web

# 3000: app (WS + REST)  ·  8080: /_liveness, /_readiness  ·  9102: /metrics
EXPOSE 3000 8080 9102

# deploy/compose mounts a volume here; pre-created so it inherits buzz:buzz.
RUN mkdir -p /data/git && chown buzz:buzz /data/git

# The pre-rename names, as links to the current binaries. Hand-managed scripts
# on deployed hosts call them (hive's run.sh calls /usr/local/bin/buzz-admin),
# and a compose file may name buzz-relay as its command. Remove once no
# deployment does. Created before USER: /usr/local/bin is root's.
RUN ln -s beekeeper-relay /usr/local/bin/buzz-relay \
    && ln -s beekeeper-admin /usr/local/bin/buzz-admin \
    && ln -s beekeeper-pair-relay /usr/local/bin/buzz-pair-relay

USER buzz:buzz
WORKDIR /var/lib/buzz

ENTRYPOINT ["/usr/local/bin/beekeeper-relay"]

# Optimized binaries with line-table debug information for native profiling.
# Published under debug-* tags; runtime behavior otherwise matches the normal
# image exactly.
FROM runtime-base AS runtime-debug
COPY --from=builder /build/target/release/beekeeper-relay /usr/local/bin/beekeeper-relay
COPY --from=builder /build/target/release/beekeeper-admin /usr/local/bin/beekeeper-admin
COPY --from=builder /build/target/release/beekeeper-pair-relay /usr/local/bin/beekeeper-pair-relay

# Keep the stripped runtime as the final/default Dockerfile target so existing
# `docker build .` callers and release tags retain their current behavior.
FROM runtime-base AS runtime
COPY --from=stripped-binaries /build/target/release/beekeeper-relay /usr/local/bin/beekeeper-relay
COPY --from=stripped-binaries /build/target/release/beekeeper-admin /usr/local/bin/beekeeper-admin
COPY --from=stripped-binaries /build/target/release/beekeeper-pair-relay /usr/local/bin/beekeeper-pair-relay
