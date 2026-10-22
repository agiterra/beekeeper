# shellcheck shell=bash
# Which Postgres, Redis and S3 containers the integration recipes use.
#
# Sourced by the `_ensure-services`, `test-*` and `*-check` recipes and the
# scripts they run. The defaults are the dev compose stack (docker-compose.yml:
# buzz-postgres on 5432, buzz-redis on 6379, buzz-rustfs on 9000). A run that
# must not touch that stack — a second worktree, a validation lane, a machine
# whose dev database holds work — names its own containers instead, in the
# environment or in .env (the Justfile loads it):
#
#   BEEKEEPER_DEV_POSTGRES_CONTAINER  BEEKEEPER_DEV_POSTGRES_PORT
#   BEEKEEPER_DEV_REDIS_CONTAINER     BEEKEEPER_DEV_REDIS_PORT
#   BEEKEEPER_DEV_S3_CONTAINER        BEEKEEPER_S3_ENDPOINT (read by the relay)
#
# Each container must accept the dev credentials (buzz / buzz_dev). Scratch
# databases are still created per run inside the named Postgres and dropped on
# exit; the override only moves which server they are created on.
#
# `_ensure-services` starts the compose stack only when every name is a
# default. With an override it checks health and refuses rather than starting
# the shared stack next to the one you named.

: "${BEEKEEPER_DEV_POSTGRES_CONTAINER:=buzz-postgres}"
: "${BEEKEEPER_DEV_REDIS_CONTAINER:=buzz-redis}"
: "${BEEKEEPER_DEV_S3_CONTAINER:=buzz-rustfs}"
: "${BEEKEEPER_DEV_POSTGRES_PORT:=5432}"
: "${BEEKEEPER_DEV_REDIS_PORT:=6379}"
export BEEKEEPER_DEV_POSTGRES_CONTAINER BEEKEEPER_DEV_REDIS_CONTAINER \
  BEEKEEPER_DEV_S3_CONTAINER BEEKEEPER_DEV_POSTGRES_PORT BEEKEEPER_DEV_REDIS_PORT

# True when every service is the dev compose default.
dev_services_are_compose_defaults() {
  [[ "${BEEKEEPER_DEV_POSTGRES_CONTAINER}" == buzz-postgres &&
    "${BEEKEEPER_DEV_REDIS_CONTAINER}" == buzz-redis &&
    "${BEEKEEPER_DEV_S3_CONTAINER}" == buzz-rustfs ]]
}

# Health of a container by name: healthy / starting / unhealthy / not_found.
dev_service_health() {
  docker inspect --format '{{.State.Health.Status}}' "$1" 2>/dev/null || echo not_found
}

# psql inside the named Postgres container, over TCP: the image's first-boot
# init server listens only on the unix socket, so a TCP answer means the real
# server is up.
dev_pg() {
  docker exec -e PGPASSWORD=buzz_dev "${BEEKEEPER_DEV_POSTGRES_CONTAINER}" \
    psql -h 127.0.0.1 -U buzz -q "$@"
}

# URL of database $1 on the named Postgres, as seen from this machine.
dev_pg_url() {
  echo "postgres://buzz:buzz_dev@localhost:${BEEKEEPER_DEV_POSTGRES_PORT}/$1" # sadscan:disable np.postgres.1
}

# URL of Redis logical database $1 on the named Redis.
dev_redis_url() {
  echo "redis://localhost:${BEEKEEPER_DEV_REDIS_PORT}/$1"
}
