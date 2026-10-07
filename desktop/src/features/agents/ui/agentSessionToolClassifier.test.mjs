import assert from "node:assert/strict";
import test from "node:test";

import {
  classifyTool,
  parseBeekeeperCliCommand,
  tokenizeShellCommand,
} from "./agentSessionToolClassifier.ts";

test("tokenizeShellCommand preserves quoted strings and command separators", () => {
  assert.deepEqual(
    tokenizeShellCommand(
      'echo "hello world" | bee messages send --content - --channel agents; bee feed get',
    ),
    [
      "echo",
      "hello world",
      "|",
      "bee",
      "messages",
      "send",
      "--content",
      "-",
      "--channel",
      "agents",
      ";",
      "bee",
      "feed",
      "get",
    ],
  );
});

test("parseBeekeeperCliCommand returns null preview for echo-piped stdin sends", () => {
  const descriptor = parseBeekeeperCliCommand(
    'echo "Permission wired" | bee messages send --channel agents --content -',
  );

  assert.equal(descriptor?.renderClass, "message");
  assert.equal(descriptor?.label, "Send Message");
  assert.equal(descriptor?.preview, null);
  assert.equal(descriptor?.operation, "messages.send");
});

test("parseBeekeeperCliCommand returns null preview for printf-piped stdin sends", () => {
  const descriptor = parseBeekeeperCliCommand(
    "printf 'hello\\n\\nworld\\n' | bee messages send --channel a6e0737c-4205-4bcc-9741-2aad800e613f --content -",
  );

  assert.equal(descriptor?.renderClass, "message");
  assert.equal(descriptor?.preview, null);
});

test("parseBeekeeperCliCommand returns null preview for heredoc/cat stdin sends", () => {
  const descriptor = parseBeekeeperCliCommand(
    'bee messages send --channel some-uuid --content "$(cat /tmp/file)"',
  );

  assert.equal(descriptor?.renderClass, "message");
  assert.equal(descriptor?.preview, null);
});

test("parseBeekeeperCliCommand returns null preview for --content with embedded command substitution", () => {
  const descriptor = parseBeekeeperCliCommand(
    'bee messages send --channel some-uuid --content "prefix $(cat /tmp/f)"',
  );

  assert.equal(descriptor?.renderClass, "message");
  assert.equal(descriptor?.preview, null);
});

test("parseBeekeeperCliCommand returns null preview for --content with a bare variable", () => {
  const descriptor = parseBeekeeperCliCommand(
    'bee messages send --channel some-uuid --content "$MESSAGE"',
  );

  assert.equal(descriptor?.renderClass, "message");
  assert.equal(descriptor?.preview, null);
});

test("parseBeekeeperCliCommand returns null preview for --content with a prefixed variable", () => {
  const descriptor = parseBeekeeperCliCommand(
    'bee messages send --channel some-uuid --content "prefix $MESSAGE"',
  );

  assert.equal(descriptor?.renderClass, "message");
  assert.equal(descriptor?.preview, null);
});

test("parseBeekeeperCliCommand preserves inline --content for sends", () => {
  const descriptor = parseBeekeeperCliCommand(
    'bee messages send --channel agents --content "Hello from inline"',
  );

  assert.equal(descriptor?.renderClass, "message");
  assert.equal(descriptor?.preview, "Hello from inline");
});

test("parseBeekeeperCliCommand preserves --content=inline for sends", () => {
  const descriptor = parseBeekeeperCliCommand(
    "bee messages send --channel agents --content=Acknowledged",
  );

  assert.equal(descriptor?.renderClass, "message");
  assert.equal(descriptor?.preview, "Acknowledged");
});

test("parseBeekeeperCliCommand never surfaces --channel as preview for sends", () => {
  const commands = [
    "printf 'msg' | bee messages send --channel my-uuid --content -",
    'bee messages send --channel my-uuid --content "$(cat /tmp/f)"',
    "bee messages send --channel my-uuid --content -",
  ];

  for (const cmd of commands) {
    const descriptor = parseBeekeeperCliCommand(cmd);
    assert.equal(descriptor?.renderClass, "message");
    assert.notEqual(
      descriptor?.preview,
      "my-uuid",
      `send preview leaked --channel for: ${cmd}`,
    );
  }
});

test("classifyTool promotes load_skill to skill-read descriptors", () => {
  const descriptor = classifyTool({
    title: "load_skill",
    toolName: "load_skill",
    buzzToolName: null,
    args: { name: "block-safe-github" },
    result: "# Safe GitHub usage at Block\n",
    isError: false,
  });

  assert.equal(descriptor.renderClass, "skill-read");
  assert.equal(descriptor.label, "Read skill");
  assert.equal(descriptor.preview, "block-safe-github");
  assert.deepEqual(descriptor.action, {
    verb: "Read",
    object: "block-safe-github",
  });
  assert.equal(descriptor.groupKey, "skill:load");
});

test("classifyTool promotes supporting-file load_skill to skill-read file descriptors", () => {
  const descriptor = classifyTool({
    title: "load_skill",
    toolName: "load_skill",
    buzzToolName: null,
    args: { name: "block-safe-github/references/foo.md" },
    result: "# Reference\n",
    isError: false,
  });

  assert.equal(descriptor.renderClass, "skill-read");
  assert.equal(descriptor.label, "Read skill file");
  assert.equal(descriptor.groupKey, "skill:load-file");
});

test("classifyTool promotes buzz CLI shell commands to relay operations", () => {
  const descriptor = classifyTool({
    title: "Shell",
    toolName: "dev__shell",
    buzzToolName: null,
    args: { command: "bee channels get --channel buzz-agent-observability" },
    result: "{}",
    isError: false,
  });

  assert.equal(descriptor.renderClass, "relay-op");
  assert.equal(descriptor.label, "Channels Get");
  assert.equal(descriptor.preview, "buzz-agent-observability");
  assert.equal(descriptor.groupKey, "buzz-cli:channels.get");
});

test("classifyTool humanizes session history and counts its unwrapped result", () => {
  const descriptor = classifyTool({
    title: "mcp.buzz-session-context.session_history",
    toolName: "mcp.buzz-session-context.session_history",
    buzzToolName: null,
    args: {},
    result: JSON.stringify({
      content: [
        {
          type: "text",
          text: JSON.stringify({ availableHistoryItems: 47, items: [] }),
        },
      ],
    }),
    isError: false,
  });

  assert.equal(descriptor.label, "Session history");
  assert.equal(descriptor.preview, "· 47 items");
  assert.deepEqual(descriptor.action, {
    verb: "Session history",
    object: "· 47 items",
  });
  assert.equal(descriptor.source, "mcp");
});

test("classifyTool falls back once to a generic descriptor", () => {
  const descriptor = classifyTool({
    title: "Mystery",
    toolName: "mcp__mystery",
    buzzToolName: null,
    args: { path: "notes.md" },
    result: "",
    isError: false,
  });

  assert.equal(descriptor.renderClass, "generic");
  assert.equal(descriptor.label, "Ran tool");
  assert.equal(descriptor.preview, "notes.md");
  assert.equal(descriptor.source, "fallback");
});

test("classifyTool reads Claude Code's Bash as a command (SV-03)", () => {
  const descriptor = classifyTool({
    title: "Bash",
    toolName: "Bash",
    buzzToolName: null,
    args: { command: "cargo test", description: "Run tests" },
    result: "",
    isError: false,
  });
  assert.equal(descriptor.renderClass, "shell");
  assert.equal(descriptor.label, "Ran command");
  assert.deepEqual(descriptor.action, { verb: "Ran", object: "cargo test" });

  const failed = classifyTool({
    title: "Bash",
    toolName: "Bash",
    buzzToolName: null,
    args: { command: "false" },
    result: "",
    isError: true,
  });
  assert.equal(failed.renderClass, "error");
  assert.equal(failed.label, "Ran command failed");
  assert.deepEqual(failed.action, { verb: "Ran", object: "false" });
});

test("classifyTool reads an ACP execute call as a command whatever its title", () => {
  const descriptor = classifyTool({
    title: "`ls -la`",
    toolName: "`ls -la`",
    buzzToolName: null,
    args: { command: "ls -la" },
    result: "",
    isError: false,
    toolKind: "execute",
  });
  assert.equal(descriptor.renderClass, "shell");
  assert.equal(descriptor.preview, "ls -la");
  // Without the discriminant the same title stays a generic tool.
  assert.equal(
    classifyTool({
      title: "`ls -la`",
      toolName: "`ls -la`",
      buzzToolName: null,
      args: {},
      result: "",
      isError: false,
    }).renderClass,
    "generic",
  );
});

test("a Bash call that runs bee still reads as its relay operation", () => {
  const descriptor = classifyTool({
    title: "Bash",
    toolName: "Bash",
    buzzToolName: null,
    args: { command: "bee messages send --channel x --content hi" },
    result: "",
    isError: false,
  });
  assert.equal(descriptor.renderClass, "message");
});

// The MCP servers were renamed buzz-* -> beekeeper-* (2026-10). Persisted
// transcripts still carry the old server names, so each case below is run with
// BOTH: the `buzz-*` fixtures are old data on purpose and must stay old.
const DEV_MCP_SERVERS = ["buzz-dev-mcp", "beekeeper-dev-mcp"];
const SESSION_CONTEXT_SERVERS = [
  "buzz-session-context",
  "beekeeper-session-context",
];

for (const server of DEV_MCP_SERVERS) {
  test(`classifyTool reads ${server}__todo as a todo update`, () => {
    const descriptor = classifyTool({
      title: `${server}__todo`,
      toolName: `${server}__todo`,
      buzzToolName: null,
      args: {},
      result: "",
      isError: false,
    });
    assert.equal(descriptor.renderClass, "plan");
    assert.equal(descriptor.groupKey, "plan:todo");
  });

  test(`classifyTool reads ${server}__stop as the suppressed stop hook`, () => {
    const descriptor = classifyTool({
      title: `${server}__stop`,
      toolName: `${server}__stop`,
      buzzToolName: null,
      args: {},
      result: "",
      isError: false,
    });
    assert.equal(descriptor.renderClass, "suppressed");
    assert.equal(descriptor.groupKey, "suppressed:stop-hook");
  });

  test(`classifyTool attributes an unknown ${server} tool to the harness`, () => {
    const descriptor = classifyTool({
      title: `mcp__${server}__brand_new_tool`,
      toolName: `mcp__${server}__brand_new_tool`,
      buzzToolName: null,
      args: {},
      result: "",
      isError: false,
    });
    assert.equal(descriptor.source, "harness");
    assert.equal(descriptor.groupKey, "generic:dev-mcp");
  });

  test(`classifyTool reads mcp__${server}__shell as a command`, () => {
    const descriptor = classifyTool({
      title: `mcp__${server}__shell`,
      toolName: `mcp__${server}__shell`,
      buzzToolName: null,
      args: { command: "ls -la" },
      result: "",
      isError: false,
    });
    assert.equal(descriptor.renderClass, "shell");
  });
}

for (const server of SESSION_CONTEXT_SERVERS) {
  test(`classifyTool humanizes ${server} session overview`, () => {
    const descriptor = classifyTool({
      title: `mcp__${server}__session_overview`,
      toolName: `mcp__${server}__session_overview`,
      buzzToolName: null,
      args: {},
      result: "",
      isError: false,
    });
    assert.equal(descriptor.label, "Session overview");
    assert.equal(descriptor.source, "mcp");
  });
}
