import { readProjectFile } from "@/shared/api/projectFiles";
import {
  parseModelRegistry,
  type ModelRegistry,
} from "./codingSessionModelRegistry";
import { MODEL_REGISTRY_PROJECT_PATH } from "./codingSessionRegistryAccess";
import type { CodingSessionRegistrySource } from "./codingSessionHireRouting";

/**
 * The one place this app reads `team/model-registry.yaml`.
 *
 * # Why one reader
 *
 * Two surfaces need the registry and they must never disagree about it: the
 * hire host, which routes a 44221 `session.hire` against registry ∩ live
 * catalog, and the Agents tab's staleness badge. If each read the file its own
 * way, the badge could say "covers the catalog" while the router refused every
 * hire — the exact class of comfortable-but-unverified answer this surface
 * exists to prevent. So both call this module, and it is the only caller of
 * `read_project_file` for the registry.
 *
 * # Where the file is read from
 *
 * From the project checkout the Agents tab already resolves
 * (`features/agents/lib/rolePacksProject.ts` → `useRolePacksProject`), named
 * by its NIP-MP coordinate (`ProjectContainer.address`, e.g.
 * `30621:<owner>:<dtag>`). The host turns that coordinate into a directory
 * through the same record the Agents tab ordered its projects by —
 * `CodingSessionWorkdirStore::by_project` — so the file this reads is the file
 * in the checkout the operator is actually looking at.
 *
 * # Every no-answer is an answer
 *
 * There is no fallback and no built-in copy. A project with no recorded
 * checkout, a file that is not there, a file that is not a registry this build
 * understands — each comes back as `unreadable` carrying the host's own
 * sentence, naming the path. A registry the operator cannot see is not a
 * registry; it is a hardcoded opinion wearing one's name.
 */

/**
 * What this host holds for the shared registry.
 *
 * This is `CodingSessionRegistrySource` itself, not a parallel shape. The hire
 * host feeds the value straight into `resolveCodingSessionHireRouting`, so a
 * second near-identical type here would be one rename away from a reader whose
 * answers the router silently cannot read — the shape of the bug this batch
 * exists to fix. `label` carries the absolute path the text was read from.
 */
export type ModelRegistrySource = CodingSessionRegistrySource;

/** Test seam: the host call, injectable so the reader is testable in jsdom. */
export type ModelRegistryReaderDeps = {
  read: (
    projectRef: string,
    relativePath: string,
  ) => Promise<{
    path: string;
    text: string;
  }>;
};

const DEFAULT_DEPS: ModelRegistryReaderDeps = { read: readProjectFile };

/** The sentence for a thrown refusal, a thrown `Error`, or anything else. */
function sentenceFor(error: unknown): string {
  if (
    typeof error === "object" &&
    error !== null &&
    typeof (error as { message?: unknown }).message === "string"
  ) {
    return (error as { message: string }).message;
  }
  if (typeof error === "string") return error;
  return String(error);
}

/**
 * Read the shared model registry out of a project checkout.
 *
 * `projectRef` is the project's NIP-MP coordinate, or `null` when no project
 * could be resolved at all — which is itself reported rather than treated as
 * an empty registry.
 */
export async function readModelRegistry(
  projectRef: string | null,
  deps: ModelRegistryReaderDeps = DEFAULT_DEPS,
): Promise<ModelRegistrySource> {
  if (projectRef === null || projectRef.trim().length === 0) {
    return {
      kind: "unreadable",
      why:
        `This app resolved no project to read ${MODEL_REGISTRY_PROJECT_PATH} ` +
        "from. Open a project, or choose one on the Agents tab.",
    };
  }
  try {
    const file = await deps.read(projectRef, MODEL_REGISTRY_PROJECT_PATH);
    return { kind: "readable", text: file.text, label: file.path };
  } catch (error) {
    return { kind: "unreadable", why: sentenceFor(error) };
  }
}

/** One registry row reduced to the pair the coverage badge compares. */
export type ModelRegistryCoverageRow = {
  provider: string;
  model: string;
};

/** The registry as the badge needs it, or why there is none. */
export type ModelRegistryRowsResult =
  | {
      kind: "read";
      rows: ModelRegistryCoverageRow[];
      version: number;
      /** The path the rows were read from, for the badge's detail sentence. */
      path: string;
      registry: ModelRegistry;
    }
  | { kind: "unreadable"; reason: string };

/**
 * Read the registry and parse it, for the surfaces that want rows rather than
 * text.
 *
 * Parsed by `parseModelRegistry` — the same parser the router uses — so a file
 * the router would refuse can never show up as a fresh badge.
 */
export async function readModelRegistryRows(
  projectRef: string | null,
  deps: ModelRegistryReaderDeps = DEFAULT_DEPS,
): Promise<ModelRegistryRowsResult> {
  const source = await readModelRegistry(projectRef, deps);
  if (source.kind === "unreadable") {
    return { kind: "unreadable", reason: source.why };
  }
  const parsed = parseModelRegistry(source.text);
  if (!parsed.ok) {
    return { kind: "unreadable", reason: `${source.label}: ${parsed.why}` };
  }
  return {
    kind: "read",
    rows: parsed.registry.targets.map((target) => ({
      provider: target.provider,
      model: target.model,
    })),
    version: parsed.registry.version,
    path: source.label,
    registry: parsed.registry,
  };
}
