/**
 * Grouping and naming the agents repository's files for the Files tab.
 * The grammar itself lives in `agentsRepoDraftOp.ts` (the wire twin); this
 * is presentation: which group a path sits in, in what order, and what it
 * is called.
 */
import type { AgentsRepoEntryKind } from "@/shared/api/agentsRepoTypes";

import { draftPathClass } from "./agentsRepoDraftOp";

export type AgentsRepoGroup =
  | "plans"
  | "roles"
  | "skills"
  | "team"
  | "actions"
  | "readme"
  | "archive"
  | "other";

/** Group order in the tree: plans lead, per Andy's intent for the tab. */
export const GROUP_ORDER: readonly AgentsRepoGroup[] = [
  "plans",
  "roles",
  "skills",
  "team",
  "actions",
  "readme",
  "archive",
  "other",
];

export const GROUP_LABEL: Record<AgentsRepoGroup, string> = {
  plans: "Plans",
  roles: "Roles",
  skills: "Skills",
  team: "Team",
  actions: "Actions",
  readme: "README",
  archive: "Archive (not in force)",
  other: "Other files (read-only)",
};

/** The group a path belongs to. */
export function groupOf(path: string): AgentsRepoGroup {
  const classified = draftPathClass(path);
  if (!classified.ok) return "other";
  switch (classified.class) {
    case "plan":
      return "plans";
    case "role":
      return "roles";
    case "role-skill":
    case "shared-skill":
      return "skills";
    case "archived-role":
    case "archived-plan":
      return "archive";
    case "root-file":
      if (path === "team.yml") return "team";
      if (path === "actions.yml") return "actions";
      return "readme";
  }
}

/** The host's entry kind for a path, for rows the host did not list (draft-only). */
export function kindOf(path: string): AgentsRepoEntryKind {
  if (path.endsWith("/.gitkeep")) return "gitkeep";
  const classified = draftPathClass(path);
  if (!classified.ok) return "other";
  switch (classified.class) {
    case "plan":
      return "plan";
    case "role":
      return "role";
    case "role-skill":
    case "shared-skill":
      return "skill";
    case "archived-role":
      return "archived-role";
    case "archived-plan":
      return "archived-plan";
    case "root-file":
      return path === "README.md" ? "readme" : "manifest";
  }
}

/** Whether a path renders as Markdown (preview) rather than monospace. */
export function isMarkdownPath(path: string): boolean {
  return path.endsWith(".md");
}

/** Whether a path may be drafted at all (a `.gitkeep` or an `other` file may not). */
export function isDraftablePath(path: string): boolean {
  return !path.endsWith("/.gitkeep") && draftPathClass(path).ok;
}

/** The short name shown in the tree: the file stem for plans and roles, else the tail. */
export function displayName(path: string): string {
  const tail = path.slice(path.lastIndexOf("/") + 1);
  const group = groupOf(path);
  if (group === "plans" || group === "roles" || group === "archive") {
    return tail.endsWith(".md") ? tail.slice(0, -3) : tail;
  }
  if (group === "skills") {
    // `skills/<skill>/SKILL.md` → `<skill>`; deeper files keep their tail.
    const segments = path.split("/");
    if (tail === "SKILL.md") return segments[segments.length - 2] ?? tail;
  }
  return tail;
}

/** Group a list of paths into ordered sections. */
export function groupPaths<T extends { path: string }>(
  entries: readonly T[],
): { group: AgentsRepoGroup; label: string; entries: T[] }[] {
  const buckets = new Map<AgentsRepoGroup, T[]>();
  for (const entry of entries) {
    const group = groupOf(entry.path);
    const list = buckets.get(group) ?? [];
    list.push(entry);
    buckets.set(group, list);
  }
  return GROUP_ORDER.flatMap((group) => {
    const list = buckets.get(group);
    if (!list || list.length === 0) return [];
    list.sort((a, b) => a.path.localeCompare(b.path));
    return [{ group, label: GROUP_LABEL[group], entries: list }];
  });
}

/** A new plan's path from a name the person typed. */
export function newPlanPath(
  name: string,
): { ok: true; path: string } | { ok: false; error: string } {
  const slug = name
    .trim()
    .toLowerCase()
    .replace(/\.md$/, "")
    .replace(/[^a-z0-9-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  if (slug.length === 0) return { ok: false, error: "Give the plan a name." };
  if (slug === "archive") return { ok: false, error: "“archive” is reserved." };
  const path = `plans/${slug}.md`;
  const classified = draftPathClass(path);
  return classified.ok
    ? { ok: true, path }
    : { ok: false, error: classified.error };
}
