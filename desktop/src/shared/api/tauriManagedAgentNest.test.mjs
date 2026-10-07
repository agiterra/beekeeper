/**
 * The nest fields crossing the Tauri boundary.
 *
 * One rule, the same one `hasRolePack` already lives by: **absence is not a
 * claim**. A backend that never answered must not turn every agent into one
 * whose pack is being refused, because the badge that reads that field carries
 * a remedy — and telling an operator to move an agent that is not stuck is
 * exactly the kind of confident wrong answer this surface exists to avoid.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { fromRawManagedAgent } from "./tauriManagedAgentRecord.ts";

/** The smallest record the mapper accepts, plus whatever the case is about. */
function raw(extra) {
  return {
    pubkey: "a".repeat(64),
    name: "Bob",
    persona_id: null,
    relay_url: "wss://relay.example",
    acp_command: "beekeeper-acp",
    agent_command: "claude",
    agent_args: [],
    mcp_command: "",
    turn_timeout_seconds: 320,
    idle_timeout_seconds: null,
    max_turn_duration_seconds: null,
    parallelism: 1,
    system_prompt: null,
    model: null,
    provider: null,
    persona_out_of_date: false,
    persona_orphaned: false,
    needs_restart: false,
    status: "stopped",
    pid: null,
    created_at: "",
    updated_at: "",
    last_started_at: null,
    last_stopped_at: null,
    last_exit_code: null,
    last_error: null,
    last_error_code: null,
    log_path: "",
    start_on_app_launch: false,
    backend: { type: "local" },
    backend_agent_id: null,
    ...extra,
  };
}

test("an agent in a nest of its own is not called refused", () => {
  const agent = fromRawManagedAgent(raw({ pack_refused_shared_home: false }));
  assert.equal(agent.packRefusedSharedHome, false);
});

test("an agent still in the shared home discloses the refusal", () => {
  const agent = fromRawManagedAgent(raw({ pack_refused_shared_home: true }));
  assert.equal(agent.packRefusedSharedHome, true);
});

test("a backend that never answered accuses the agent of nothing", () => {
  const agent = fromRawManagedAgent(raw({}));
  assert.equal(
    agent.packRefusedSharedHome,
    undefined,
    "NOT `?? false` — the badge reads presence, and a guess in either direction is wrong",
  );
});
