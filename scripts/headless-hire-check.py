#!/usr/bin/env python3
"""Run the existing acme commissioning harness with entirely separate services.

No compose reset, named persistent volume, production endpoint or login service.
Only container IDs created by this invocation are removed; evidence is retained.
Build bee/relay/host/provider/git-credential-nostr before invoking this script.
"""
import json
import os
import pathlib
import socket
import signal
import subprocess
import tempfile
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def command(*args, **kwargs):
    return subprocess.check_output(args, text=True, **kwargs).strip()


def main():
    work = pathlib.Path(tempfile.mkdtemp(prefix="hire-ab2-", dir="/tmp"))
    os.chmod(work, 0o700)
    namespace = "bk-hire-ab2-" + uuid.uuid4().hex[:8]
    ports = {name: port() for name in ("postgres", "redis", "s3", "health", "metrics")}
    names = {name: namespace + "-" + name for name in ports}
    containers = []
    allowed = {"PATH", "HOME", "USER", "LOGNAME", "TMPDIR", "LANG", "LC_ALL", "TERM", "DOCKER_HOST", "DOCKER_CONTEXT", "HERMIT_ENV", "HERMIT_BIN"}
    env = {k: v for k, v in os.environ.items() if k in allowed}
    try:
        containers.append(command("docker", "run", "-d", "--rm", "--name", names["postgres"], "--memory=512m", "-p", f"127.0.0.1:{ports['postgres']}:5432", "-e", "POSTGRES_USER=buzz", "-e", "POSTGRES_PASSWORD=buzz_dev", "-e", "POSTGRES_DB=buzz", "--health-cmd", "pg_isready -h 127.0.0.1 -U buzz -d buzz", "--health-interval=2s", "--health-retries=45", "postgres:17-alpine"))
        containers.append(command("docker", "run", "-d", "--rm", "--name", names["redis"], "--memory=128m", "-p", f"127.0.0.1:{ports['redis']}:6379", "redis:7-alpine"))
        containers.append(command("docker", "run", "-d", "--rm", "--name", names["s3"], "--memory=768m", "-p", f"127.0.0.1:{ports['s3']}:9000", "-e", "RUSTFS_ACCESS_KEY=buzz_dev", "-e", "RUSTFS_SECRET_KEY=buzz_dev_secret", "--health-cmd", "curl -f http://localhost:9000/health", "--health-interval=2s", "--health-retries=45", "rustfs/rustfs:1.0.1"))
        (work / "isolation.json").write_text(json.dumps({"namespace": namespace, "ports": ports, "names": names, "container_ids": containers}, indent=2))
        for name in ("postgres", "s3"):
            deadline = time.monotonic() + 120
            while command("docker", "inspect", "--format", "{{.State.Health.Status}}", names[name]) != "healthy":
                if time.monotonic() > deadline:
                    raise RuntimeError(f"isolated {name} did not become healthy")
                time.sleep(1)
        bucket = "hire-proof-" + namespace.rsplit("-", 1)[1]
        command("docker", "run", "--rm", "--network", "container:" + names["s3"], "--entrypoint", "/bin/sh", "pgsty/mc:RELEASE.2026-09-16T00-00-00Z", "-c", "mc alias set local http://127.0.0.1:9000 buzz_dev buzz_dev_secret && mc mb local/" + bucket)
        env.update({"BEEKEEPER_RATE_LIMIT_HUMAN_API_CALLS_PER_MIN": "10000", "BEEKEEPER_HEALTH_PORT": str(ports["health"]), "BEEKEEPER_METRICS_PORT": str(ports["metrics"]), "BEEKEEPER_DEV_POSTGRES_CONTAINER": names["postgres"], "BEEKEEPER_DEV_POSTGRES_PORT": str(ports["postgres"]),
                    "BEEKEEPER_DEV_REDIS_CONTAINER": names["redis"], "BEEKEEPER_DEV_REDIS_PORT": str(ports["redis"]), "BEEKEEPER_DEV_S3_CONTAINER": names["s3"],
                    "BEEKEEPER_S3_ENDPOINT": f"http://127.0.0.1:{ports['s3']}", "BEEKEEPER_S3_ACCESS_KEY": "buzz_dev", "BEEKEEPER_S3_SECRET_KEY": "buzz_dev_secret",
                    "BEEKEEPER_S3_BUCKET": bucket, "BEEKEEPER_S3_REGION": "us-east-1", "BEEKEEPER_S3_ADDRESSING_STYLE": "path",
                    "HEADLESS_HIRE_CHECK": "1", "HEADLESS_ACME_KEEP": "1", "HEADLESS_ACME_WORKDIR": str(work / "run")})
        print("isolated hire evidence: " + str(work), flush=True)
        process = subprocess.Popen(["bash", "scripts/headless-acme-check.sh"], cwd=ROOT, env=env, start_new_session=True)
        try:
            return process.wait(timeout=900)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            raise
    finally:
        for container in reversed(containers):
            subprocess.run(["docker", "rm", "-f", container], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        (work / "cleanup.json").write_text(json.dumps({"removed_owned_container_ids": containers}))
        print("retained evidence: " + str(work), flush=True)


if __name__ == "__main__":
    raise SystemExit(main())
