#!/bin/sh
# Mock ACP adapter for testing acp_probe.py — mirrors the capability shape in
# crates/buzz-acp/src/acp.rs:4446 test fixtures.
read _line
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":2,"agentCapabilities":{"loadSession":true,"sessionCapabilities":{"resume":{}}}}}'
sleep 5
