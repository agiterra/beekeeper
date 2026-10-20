/**
 * The relay's plugin route (`docs/plugins.md`), for any surface that drives
 * a plugin. A plugin runs on the relay as a WebAssembly component; this
 * client only asks it for operations and reports what came back. It never
 * holds plugin logic: a surface that needs to know what an operation means
 * asks the plugin.
 *
 * A refusal is a typed {@link PluginCallError} carrying the relay's code, so
 * a surface can draw an honest state ("this relay runs no Planning plugin",
 * "you may only read this project") instead of a generic failure.
 */
import { invokeTauri } from "@/shared/api/tauri";

/** One registered plugin, as `GET /plugins` lists it. */
export type PluginInfo = {
  name: string;
  version: string;
  sha256: string;
  /** Operation APIs it implements, e.g. `beekeeper.planning/1`. */
  api: string[];
  operations: { name: string; effect: "read" | "draft"; summary: string }[];
  /** Kinds it reads in a project — what a client follows live. */
  readKinds?: number[];
  /** Kinds it drafts for the caller to sign. */
  draftKinds?: number[];
};

/** An unsigned event a write asks the caller to sign and publish. */
export type PluginDraft = {
  kind: number;
  createdAt: number;
  tags: string[][];
  content: string;
  /** Drafts sharing a group stand or fall together under `skip-group`. */
  group: string;
  ifRefused: "abort" | "skip-group";
  /** What to say if this draft is refused after earlier ones landed. */
  partialSummary?: string;
};

export type PluginInvocation<T> = {
  plugin: { name: string; version: string; sha256: string; api: string[] };
  operation: string;
  /** What the relay decided the caller may do in this project. */
  access: "read" | "write";
  result: T;
  drafts: PluginDraft[];
};

type PluginReply = { ok: boolean; status: number; body: unknown };

/** A refusal from the relay or the plugin, with the relay's code. */
export class PluginCallError extends Error {
  readonly status: number;
  /** e.g. `plugin-not-registered`, `scope-forbidden`, `plugin-trapped`. */
  readonly code: string;
  readonly trap: string | null;

  constructor(
    status: number,
    code: string,
    message: string,
    trap: string | null,
  ) {
    super(message);
    this.name = "PluginCallError";
    this.status = status;
    this.code = code;
    this.trap = trap;
  }
}

function field(body: unknown, key: string): unknown {
  return body !== null && typeof body === "object"
    ? (body as Record<string, unknown>)[key]
    : undefined;
}

function refusal(reply: PluginReply): PluginCallError {
  const code = field(reply.body, "code");
  const error = field(reply.body, "error");
  const trap = field(reply.body, "trap");
  return new PluginCallError(
    reply.status,
    typeof code === "string"
      ? code
      : // A relay that predates the plugin route answers 404 with no code.
        reply.status === 404
        ? "route-not-found"
        : "relay-error",
    typeof error === "string" ? error : `relay returned ${reply.status}`,
    typeof trap === "string" ? trap : null,
  );
}

/** `GET /plugins` on the active community's relay. */
export async function listRelayPlugins(): Promise<PluginInfo[]> {
  const reply = await invokeTauri<PluginReply>("relay_plugins_list");
  if (!reply.ok) throw refusal(reply);
  const plugins = field(reply.body, "plugins");
  return Array.isArray(plugins) ? (plugins as PluginInfo[]) : [];
}

/** The registered plugins that implement `api`, by name. */
export function providersOf(
  plugins: readonly PluginInfo[],
  api: string,
): PluginInfo[] {
  return plugins
    .filter((plugin) => plugin.api.includes(api))
    .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
}

/** `POST /plugins/{plugin}/invoke` for one operation on one project. */
export async function invokeRelayPlugin<T>(
  plugin: string,
  operation: string,
  project: string,
  input: unknown,
): Promise<PluginInvocation<T>> {
  const reply = await invokeTauri<PluginReply>("relay_plugin_invoke", {
    plugin,
    operation,
    project,
    input,
  });
  if (!reply.ok) throw refusal(reply);
  return reply.body as PluginInvocation<T>;
}
