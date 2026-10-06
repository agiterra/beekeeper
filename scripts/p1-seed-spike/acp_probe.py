#!/usr/bin/env python3
"""Probe an ACP adapter's initialize response for the G1 capability facts.

Spawns the adapter command, sends the same `initialize` the harness sends
(protocolVersion 2, newline-framed JSON-RPC — see build_initialize_params in
crates/beekeeper-acp/src/acp.rs:131 and the capability parse at acp.rs:773-779),
and prints:

  loadSession        agentCapabilities.loadSession == true        -> session/load path
  resume             agentCapabilities.sessionCapabilities.resume  -> session/resume path
                     (present, non-null, not false)

Usage:
  acp_probe.py claude-agent-acp [args...]
  acp_probe.py codex-acp [args...]
"""

import json
import subprocess
import sys
import threading


def main():
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        sys.exit(1)
    command = sys.argv[1:]

    request = {
        "jsonrpc": "2.0",
        "id": 0,
        "method": "initialize",
        "params": {
            "protocolVersion": 2,
            "clientCapabilities": {"fs": {"readTextFile": False, "writeTextFile": False}},
            "clientInfo": {"name": "p1-seed-spike-probe", "version": "0"},
        },
    }

    proc = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )

    result_holder = {}

    def read_response():
        for line in proc.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                message = json.loads(line)
            except ValueError:
                continue
            if message.get("id") == 0 and ("result" in message or "error" in message):
                result_holder["message"] = message
                return

    reader = threading.Thread(target=read_response, daemon=True)
    reader.start()
    try:
        proc.stdin.write(json.dumps(request) + "\n")
        proc.stdin.flush()
    except BrokenPipeError:
        print("adapter exited before accepting input", file=sys.stderr)
        sys.exit(2)
    reader.join(timeout=20)
    proc.kill()

    message = result_holder.get("message")
    if message is None:
        print("no initialize response within 20s from: %s" % " ".join(command), file=sys.stderr)
        sys.exit(2)
    if "error" in message:
        print("initialize error: %s" % json.dumps(message["error"]))
        sys.exit(2)

    result = message["result"]
    caps = result.get("agentCapabilities", {}) or {}
    load_supported = caps.get("loadSession") is True
    resume_value = (caps.get("sessionCapabilities") or {}).get("resume", "ABSENT")
    resume_supported = resume_value not in ("ABSENT", None, False)

    print("adapter:          %s" % " ".join(command))
    print("protocolVersion:  %s" % result.get("protocolVersion"))
    print("loadSession:      %s  -> session/load %s" % (load_supported, "SUPPORTED" if load_supported else "unsupported"))
    print("resume:           %r  -> session/resume %s" % (resume_value, "SUPPORTED" if resume_supported else "unsupported"))
    print("raw agentCapabilities: %s" % json.dumps(caps))


if __name__ == "__main__":
    main()
