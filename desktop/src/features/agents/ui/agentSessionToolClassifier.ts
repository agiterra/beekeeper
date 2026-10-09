import type {
  AgentActivityAction,
  AgentActivityDescriptor,
  AgentActivityRenderClass,
  AgentActivityTone,
  TranscriptItem,
} from "./agentSessionTypes";
import {
  formatToolTitle,
  getBeekeeperToolInfo,
  normalizeToolNameText,
} from "./agentSessionToolCatalog";
import {
  asRecord,
  getToolString,
  getToolStringList,
  parseToolResultDisplayValue,
} from "./agentSessionUtils";

type ToolItem = Extract<TranscriptItem, { type: "tool" }>;

export type ToolClassificationInput = {
  title: string;
  toolName: string;
  beekeeperToolName: string | null;
  args: Record<string, unknown>;
  result: string;
  isError: boolean;
  /**
   * ACP's tool discriminant (`"execute"`, `"read"`, …) when the producer sent
   * one. Optional: callers that never had it classify exactly as before.
   */
  toolKind?: string | null;
};

type ToolClassifierProvider = (
  input: ToolClassificationInput,
) => AgentActivityDescriptor | null;

const DEVELOPER_TOOL_BASES = new Set([
  "shell",
  "read_file",
  "view_image",
  "str_replace",
  "todo",
  "stop",
  "postcompact",
]);

const BEEKEEPER_CLI_GROUPS = new Set([
  "messages",
  "channels",
  "dms",
  "reactions",
  "canvas",
  "feed",
  "users",
  "workflows",
  "social",
  "repos",
  "upload",
  "mem",
  "notes",
  "patches",
  "pr",
  "issues",
  "emoji",
  "pack",
]);

const BEEKEEPER_CLI_ADMIN_VERBS = new Set([
  "archive",
  "unarchive",
  "create",
  "delete",
  "remove",
  "add-channel-member",
  "remove-channel-member",
  "set-channel-add-policy",
]);

const BEEKEEPER_CLI_READ_VERBS = new Set([
  "get",
  "list",
  "thread",
  "search",
  "members",
  "runs",
  "notes",
]);

const TOOL_CLASS_LABELS: Record<AgentActivityRenderClass, string> = {
  message: "Message",
  "relay-op": "Relay op",
  "file-edit": "File edit",
  "file-read": "File read",
  "skill-read": "Skill read",
  image: "Image",
  shell: "Shell command",
  status: "Status",
  thought: "Thought",
  plan: "Plan",
  permission: "Permission",
  error: "Error",
  generic: "Tool",
  "raw-rail": "Raw event",
  suppressed: "Suppressed",
};

const providers: ToolClassifierProvider[] = [
  classifySessionContextTool,
  classifyLoadSkillTool,
  classifyDeveloperHarnessTool,
  classifyProviderShellTool,
  classifyBeekeeperTool,
];

/**
 * Claude Code's `Bash`, and any ACP call whose discriminant is `execute`, ran
 * a command (SV-03) — T3 maps the same call to `command_execution`
 * (`apps/server/src/provider/Layers/ClaudeAdapter.ts`). Without this they
 * fell through to "Ran tool" and a fold read "ran 5 tool calls".
 *
 * A `bee` invocation still reads as the relay operation it performed.
 */
function classifyProviderShellTool(
  input: ToolClassificationInput,
): AgentActivityDescriptor | null {
  const isBash = [input.toolName, input.title].some(
    (value) => value && normalizeToolNameText(value) === "bash",
  );
  if (!isBash && input.toolKind !== "execute") return null;
  const command = getToolString(input.args, ["command", "cmd"]);
  return (
    (command ? parseBeekeeperCliCommand(command) : null) ??
    shellDescriptor(command, "acp")
  );
}

function shellDescriptor(
  command: string | null,
  source: AgentActivityDescriptor["source"],
): AgentActivityDescriptor {
  return {
    renderClass: "shell",
    label: "Ran command",
    preview: command,
    action: { verb: "Ran", object: command ?? "command" },
    source,
    groupKey: "shell:command",
  };
}

/**
 * MCP server names as they appear in a normalized tool name. Each server was
 * renamed from `buzz-*` to `beekeeper-*`, but persisted transcripts still carry
 * the old names (`mcp__buzz-dev-mcp__shell`), so both are recognised. Keep the
 * legacy spellings for as long as old transcripts can be opened.
 */
const DEV_MCP_SERVER_NAMES = ["beekeeper_dev_mcp", "buzz_dev_mcp"] as const;
const DEV_MCP_SERVER_PREFIX = /^(?:beekeeper|buzz)_dev_mcp_/;
const SESSION_CONTEXT_SERVER_PREFIXES = [
  "beekeeper_session_context_",
  "buzz_session_context_",
] as const;

function classifySessionContextTool(
  input: ToolClassificationInput,
): AgentActivityDescriptor | null {
  const operation = [input.toolName, input.title, input.beekeeperToolName]
    .filter((value): value is string => Boolean(value))
    .map(normalizeToolNameText)
    .find((value) =>
      ["session_history", "session_overview", "search_session"].some(
        (candidate) =>
          value === candidate ||
          value.endsWith(`_${candidate}`) ||
          SESSION_CONTEXT_SERVER_PREFIXES.some((prefix) =>
            value.includes(`${prefix}${candidate}`),
          ),
      ),
    );
  if (!operation) return null;

  const result = asRecord(parseToolResultDisplayValue(input.result));
  const available = result.availableHistoryItems;
  const returned = result.returned;
  const count =
    typeof available === "number" && Number.isFinite(available)
      ? available
      : typeof returned === "number" && Number.isFinite(returned)
        ? returned
        : null;
  const label = operation.includes("session_history")
    ? "Session history"
    : operation.includes("search_session")
      ? "Searched session"
      : "Session overview";
  const preview =
    count === null ? null : `· ${count} item${count === 1 ? "" : "s"}`;

  return {
    renderClass: "generic",
    label,
    preview,
    action: { verb: label, object: preview },
    source: "mcp",
    groupKey: `session-context:${operation}`,
  };
}

export function classifyTool(
  input: ToolClassificationInput,
): AgentActivityDescriptor {
  for (const provider of providers) {
    const descriptor = provider(input);
    if (descriptor) {
      return input.isError || descriptor.renderClass === "error"
        ? {
            ...descriptor,
            renderClass: "error",
            label: descriptor.label.endsWith("failed")
              ? descriptor.label
              : `${descriptor.label} failed`,
          }
        : descriptor;
    }
  }

  return genericDescriptor(input);
}

export function classifyToolItem(item: ToolItem): AgentActivityDescriptor {
  return classifyTool({
    title: item.title,
    toolName: item.toolName,
    beekeeperToolName: item.beekeeperToolName,
    args: item.args,
    result: item.result,
    isError: item.isError,
    toolKind: item.toolKind ?? null,
  });
}

export function renderClassLabel(renderClass: AgentActivityRenderClass) {
  return TOOL_CLASS_LABELS[renderClass];
}

function classifyLoadSkillTool(
  input: ToolClassificationInput,
): AgentActivityDescriptor | null {
  const isLoadSkill = [
    input.toolName,
    input.title,
    input.beekeeperToolName,
  ].some((value) => value && normalizeToolNameText(value) === "load_skill");
  if (!isLoadSkill) return null;

  const skillRef = getToolString(input.args, ["name"]);
  const object = skillRef ?? "skill";
  const isSupportingFile = skillRef?.includes("/") ?? false;

  return {
    renderClass: "skill-read",
    label: isSupportingFile ? "Read skill file" : "Read skill",
    preview: skillRef,
    action: { verb: "Read", object },
    source: "harness",
    groupKey: isSupportingFile ? "skill:load-file" : "skill:load",
  };
}

function classifyDeveloperHarnessTool(
  input: ToolClassificationInput,
): AgentActivityDescriptor | null {
  const kind = resolveDeveloperToolKind(input);
  if (!kind) return null;

  if (kind === "shell") {
    const command = getToolString(input.args, ["command"]);
    const beekeeperCli = command ? parseBeekeeperCliCommand(command) : null;
    if (beekeeperCli) {
      return beekeeperCli;
    }
    return shellDescriptor(command, "harness");
  }

  if (kind === "read_file") {
    const path = getToolString(input.args, ["path"]);
    return {
      renderClass: "file-read",
      label: "Read file",
      preview: path,
      action: { verb: "Read", object: path ?? "file" },
      source: "harness",
      groupKey: "read_file",
    };
  }

  if (kind === "view_image") {
    const source = getToolString(input.args, ["source"]);
    return {
      renderClass: "image",
      label: "Viewed image",
      preview: source ? basenameOrUrl(source) : null,
      action: {
        verb: "Viewed",
        object: source ? basenameOrUrl(source) : "image",
      },
      source: "harness",
      groupKey: "view_image",
    };
  }

  if (kind === "str_replace") {
    const path = getToolString(input.args, ["path"]);
    return {
      renderClass: "file-edit",
      label: "Edited file",
      preview: path,
      action: { verb: "Edited", object: path ?? "file" },
      source: "harness",
      groupKey: "file-edit:str_replace",
    };
  }

  if (kind === "todo") {
    const preview = getTodoPreview(input.args);
    return {
      renderClass: "plan",
      label: "Updated todos",
      preview,
      action: { verb: "Updated", object: preview },
      source: "harness",
      groupKey: "plan:todo",
    };
  }

  if (kind === "stop_hook") {
    return {
      renderClass: "suppressed",
      label: "Checked todos",
      preview: null,
      action: { verb: "Checked", object: "todos" },
      source: "harness",
      groupKey: "suppressed:stop-hook",
    };
  }

  if (kind === "post_compact_hook") {
    return {
      renderClass: "status",
      label: "Context compacted",
      preview: null,
      action: { verb: "Compacted", object: "context" },
      source: "harness",
      groupKey: "status:post-compact",
    };
  }

  const preview = genericPreview(input);
  return {
    renderClass: "generic",
    label: "Ran tool",
    preview,
    action: { verb: "Ran", object: preview ?? "tool" },
    source: "harness",
    groupKey: "generic:dev-mcp",
  };
}

function classifyBeekeeperTool(
  input: ToolClassificationInput,
): AgentActivityDescriptor | null {
  const name = [input.beekeeperToolName, input.toolName, input.title].find(
    (value) => value && getBeekeeperToolInfo(value),
  );
  if (!name) return null;

  const info = getBeekeeperToolInfo(name);
  if (!info) return null;

  const operation = normalizeToolNameText(name);
  const label = formatToolTitle(name, input.title);
  const preview = extractBeekeeperToolPreview(input.args);
  return {
    renderClass: isBeekeeperMessageSend(operation) ? "message" : "relay-op",
    label,
    preview,
    action: actionForBeekeeperOperation(operation, preview, info.tone),
    tone: info.tone,
    operation,
    object: preview,
    source: "mcp",
    groupKey: `buzz:${operation}`,
  };
}

function genericDescriptor(
  input: ToolClassificationInput,
): AgentActivityDescriptor {
  const preview = genericPreview(input);
  return {
    renderClass: "generic",
    label: "Ran tool",
    preview,
    action: { verb: "Ran", object: preview ?? "tool" },
    source: "fallback",
    groupKey: `generic:${normalizeToolNameText(input.toolName || input.title)}`,
  };
}

function resolveDeveloperToolKind(
  input: ToolClassificationInput,
):
  | "shell"
  | "read_file"
  | "view_image"
  | "str_replace"
  | "todo"
  | "stop_hook"
  | "post_compact_hook"
  | "dev_mcp"
  | null {
  for (const value of [input.toolName, input.title, input.beekeeperToolName]) {
    const kind = classifyDeveloperToolName(value);
    if (kind) return kind;
  }
  return null;
}

function classifyDeveloperToolName(value: string | null | undefined) {
  if (!value) return null;

  const normalized = normalizeToolNameText(value);
  const base = normalized.replace(DEV_MCP_SERVER_PREFIX, "");

  if (base === "shell" || normalized.endsWith("_shell")) return "shell";
  if (base === "read_file" || normalized.endsWith("_read_file"))
    return "read_file";
  if (base === "view_image" || normalized.endsWith("_view_image"))
    return "view_image";
  if (base === "str_replace" || normalized.endsWith("_str_replace"))
    return "str_replace";
  if (base === "todo") return "todo";
  if (base === "stop") return "stop_hook";
  if (base === "postcompact") return "post_compact_hook";
  if (
    DEVELOPER_TOOL_BASES.has(base) ||
    DEV_MCP_SERVER_NAMES.some((name) => normalized.includes(name))
  ) {
    return "dev_mcp";
  }
  return null;
}

export function parseBeekeeperCliCommand(
  command: string,
): AgentActivityDescriptor | null {
  const tokens = tokenizeShellCommand(command);
  const range = findBeekeeperCommand(tokens);
  if (!range) return null;

  const group = tokens[range.groupIndex];
  const verb = tokens[range.verbIndex] ?? "run";
  const operation = `${group}.${verb}`;
  const isSend = group === "messages" && verb === "send";
  const preview = isSend
    ? extractBeekeeperCliInlineContent(tokens, range)
    : extractBeekeeperCliObjectPreview(tokens, range);
  const tone = beekeeperCliTone(group, verb);
  return {
    renderClass: isSend ? "message" : "relay-op",
    label: titleForBeekeeperCli(group, verb),
    preview,
    action: actionForBeekeeperOperation(operation, preview, tone),
    tone,
    operation,
    object: preview,
    source: "shell",
    groupKey: `buzz-cli:${operation}`,
  };
}

function titleForBeekeeperCli(group: string, verb: string) {
  if (group === "messages" && verb === "send") return "Send Message";
  return [group, verb]
    .map((part) =>
      part
        .split(/[-_]+/)
        .filter(Boolean)
        .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
        .join(" "),
    )
    .filter(Boolean)
    .join(" ");
}

function actionForBeekeeperOperation(
  operation: string,
  object: string | null,
  tone: AgentActivityTone,
): AgentActivityAction {
  const verb = beekeeperOperationVerbToken(operation);
  return {
    verb: beekeeperOperationVerb(verb, tone),
    object: object ?? beekeeperOperationObject(operation),
  };
}

function beekeeperOperationVerbToken(operation: string) {
  if (operation.includes(".")) {
    return operation.split(".")[1] ?? "run";
  }
  return operation.split("_")[0] ?? "run";
}

function beekeeperOperationVerb(verb: string, tone: AgentActivityTone) {
  if (verb === "add") return "Added";
  if (verb === "archive") return "Archived";
  if (verb === "create") return "Created";
  if (verb === "delete") return "Deleted";
  if (verb === "get" || verb === "list" || verb === "members") return "Read";
  if (verb === "remove") return "Removed";
  if (verb === "runs") return "Read";
  if (verb === "search") return "Searched";
  if (verb === "send") return "Sent";
  if (verb === "thread") return "Read";
  if (verb === "unarchive") return "Unarchived";
  if (tone === "read") return "Read";
  return "Updated";
}

function beekeeperOperationObject(operation: string) {
  if (isBeekeeperMessageSend(operation)) return "message";
  if (operation.includes(".")) {
    const [group] = operation.split(".");
    return group ? group.replace(/[-_]+/g, " ") : "relay";
  }
  const object = operation.replace(
    /^(add|approve|archive|create|delete|edit|get|hide|join|leave|list|open|publish|remove|search|send|set|trigger|unarchive|update|vote)_/,
    "",
  );
  return object ? object.replace(/[-_]+/g, " ") : "relay";
}

function beekeeperCliTone(group: string, verb: string): AgentActivityTone {
  if (BEEKEEPER_CLI_ADMIN_VERBS.has(verb)) return "admin";
  if (BEEKEEPER_CLI_READ_VERBS.has(verb)) return "read";
  if (group === "feed" && verb === "get") return "read";
  return "write";
}

function extractBeekeeperCliInlineContent(
  tokens: string[],
  range: BeekeeperCommandRange,
): string | null {
  const content = getFlagValue(tokens, range.verbIndex + 1, "--content");
  if (!content || content === "-") return null;
  if (content.includes("$") || content.includes("`")) return null;
  return content;
}

function extractBeekeeperCliObjectPreview(
  tokens: string[],
  range: BeekeeperCommandRange,
): string | null {
  const flagPreview =
    getFlagValue(tokens, range.verbIndex + 1, "--channel") ??
    getFlagValue(tokens, range.verbIndex + 1, "--event") ??
    getFlagValue(tokens, range.verbIndex + 1, "--query") ??
    getFlagValue(tokens, range.verbIndex + 1, "--name") ??
    getFlagValue(tokens, range.verbIndex + 1, "--file");
  if (flagPreview) return flagPreview;

  const next = tokens[range.verbIndex + 1];
  return next && !isCommandSeparator(next) && !next.startsWith("-")
    ? next
    : null;
}

type BeekeeperCommandRange = {
  beekeeperIndex: number;
  groupIndex: number;
  verbIndex: number;
};

function findBeekeeperCommand(tokens: string[]): BeekeeperCommandRange | null {
  for (let i = 0; i < tokens.length; i++) {
    if (!isBeekeeperExecutable(tokens[i])) continue;

    for (let j = i + 1; j < tokens.length; j++) {
      if (isCommandSeparator(tokens[j])) break;
      if (tokens[j].startsWith("-")) {
        if (
          !tokens[j].includes("=") &&
          tokens[j + 1]?.startsWith("-") === false
        ) {
          j += 1;
        }
        continue;
      }
      if (!BEEKEEPER_CLI_GROUPS.has(tokens[j])) continue;
      const verbIndex = j + 1;
      if (!tokens[verbIndex] || isCommandSeparator(tokens[verbIndex])) {
        return null;
      }
      return { beekeeperIndex: i, groupIndex: j, verbIndex };
    }
  }
  return null;
}

export function tokenizeShellCommand(command: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let quote: "'" | '"' | null = null;
  let escaping = false;

  const pushCurrent = () => {
    if (current.length > 0) {
      tokens.push(current);
      current = "";
    }
  };

  for (const char of command) {
    if (escaping) {
      current += char;
      escaping = false;
      continue;
    }
    if (char === "\\" && quote !== "'") {
      escaping = true;
      continue;
    }
    if (quote) {
      if (char === quote) quote = null;
      else current += char;
      continue;
    }
    if (char === "'" || char === '"') {
      quote = char;
      continue;
    }
    if (/\s/.test(char)) {
      pushCurrent();
      continue;
    }
    if (char === "|" || char === ";" || char === "&") {
      pushCurrent();
      tokens.push(char);
      continue;
    }
    current += char;
  }

  if (escaping) current += "\\";
  pushCurrent();
  return tokens;
}

function isBeekeeperExecutable(token: string) {
  return token === "bee" || token.split(/[\\/]/).pop() === "bee";
}

function isCommandSeparator(token: string) {
  return token === "|" || token === ";" || token === "&";
}

function getFlagValue(tokens: string[], start: number, flag: string) {
  for (let i = start; i < tokens.length; i++) {
    const token = tokens[i];
    if (isCommandSeparator(token)) return null;
    if (token === flag) {
      return tokens[i + 1] && !isCommandSeparator(tokens[i + 1])
        ? tokens[i + 1]
        : null;
    }
    if (token.startsWith(`${flag}=`)) return token.slice(flag.length + 1);
  }
  return null;
}

function extractBeekeeperToolPreview(
  args: Record<string, unknown>,
): string | null {
  const content = getToolString(args, ["content", "message", "text", "body"]);
  if (content) return content;
  const query = getToolString(args, ["query", "search"]);
  if (query) return query;
  const channelId = getToolString(args, ["channel_id", "channelId"]);
  if (channelId) return channelId;
  const workflowId = getToolString(args, ["workflow_id", "workflowId"]);
  if (workflowId) return workflowId;
  const pubkeys = getToolStringList(args, ["pubkeys", "pubkey"]);
  if (pubkeys.length === 1) return pubkeys[0];
  if (pubkeys.length > 1) return `${pubkeys.length} users`;
  return getToolString(args, ["event_id", "eventId", "name"]);
}

function genericPreview(input: ToolClassificationInput): string | null {
  return (
    getToolString(input.args, [
      "command",
      "path",
      "source",
      "query",
      "name",
      "content",
      "message",
    ]) ?? (input.title ? input.title : null)
  );
}

function isBeekeeperMessageSend(operation: string) {
  return operation === "send_message" || operation === "messages_send";
}

function basenameOrUrl(source: string): string {
  const trimmed = source.trim();
  if (
    trimmed.startsWith("data:image/") ||
    trimmed.startsWith("http://") ||
    trimmed.startsWith("https://")
  ) {
    return trimmed;
  }
  return trimmed.split(/[/\\]/).pop() ?? trimmed;
}

function getTodoPreview(args: Record<string, unknown>): string | null {
  const todos = args.todos;
  if (!Array.isArray(todos)) return "todo list";
  if (todos.length === 0) return "empty list";
  const first = todos[0];
  const firstText =
    first && typeof first === "object"
      ? getToolString(asRecord(first), ["text"])
      : null;
  if (firstText)
    return todos.length > 1 ? `${firstText} (+${todos.length - 1})` : firstText;
  return `${todos.length} item${todos.length === 1 ? "" : "s"}`;
}
