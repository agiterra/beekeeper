"""Exercise the scripted ACP through an explicit Python executable (no shebang).

Run with the Python executable that commissioning will name as --driver, and
pass the absolute acme-acp path as --driver-arg. This checks the adapter only;
it does not prove Host, relay, permission, or Windows process custody behavior.
"""
import json
import pathlib
import subprocess
import sys
import tempfile


def main():
    adapter = pathlib.Path(__file__).resolve().with_name("acme-acp")
    with tempfile.TemporaryDirectory(prefix="acme-explicit-python-") as cwd:
        requests = [
            {"jsonrpc": "2.0", "id": 1, "method": "initialize",
             "params": {"protocolVersion": 2, "clientCapabilities": {}}},
            {"jsonrpc": "2.0", "id": 2, "method": "session/new",
             "params": {"cwd": cwd, "mcpServers": []}},
        ]
        result = subprocess.run(
            [sys.executable, str(adapter)],
            input="".join(json.dumps(row) + "\n" for row in requests),
            text=True, capture_output=True, timeout=15, cwd=cwd, check=True,
        )
        replies = {row["id"]: row for line in result.stdout.splitlines()
                   if "id" in (row := json.loads(line))}
        assert replies[1]["result"]["protocolVersion"] == 2, replies[1]
        assert replies[2]["result"]["sessionId"], replies[2]
        print(json.dumps({"passed": True, "python": sys.executable,
                          "adapter": str(adapter), "scope": "ACP initialize and session/new"}))


if __name__ == "__main__":
    main()
