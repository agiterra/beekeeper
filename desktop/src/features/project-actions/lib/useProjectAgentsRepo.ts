/**
 * Where this computer's clone of a project's agents repository is, and the
 * explicit action that establishes it.
 *
 * Astra's Wave 2 review, finding 11: the Actions tab used to mount a hook
 * whose effect invoked `record_project_agents_repo`. That native call
 * synchronizes the managed packs cache — `git checkout --detach --force` and
 * `git clean -x -d --force` — and then writes the workdir store. Opening a
 * tab, as any viewer, therefore mutated a managed cache and this computer's
 * execution configuration.
 *
 * **Render reads; a person writes.** The query below is read-only; the
 * mutation is the same establishment, moved behind a control.
 */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { recordProjectAgentsRepo } from "@/features/projects-container/lib/projectAgentsInit";
import { fetchProjectPackSource } from "@/features/projects-container/lib/projectPackSource";
import { invokeTauri } from "@/shared/api/tauri";

/** React Query key for one project's recorded agents-repository clone. */
export function projectAgentsRepoQueryKey(projectRef: string) {
  return ["project-agents-repo", projectRef] as const;
}

/** What this computer has recorded, as the read reports it. */
export type ProjectAgentsRepoStatus = {
  /** Whether a clone is recorded for host steps to read `actions.yml` from. */
  recorded: boolean;
  /** Its path, when one is recorded. Host-local, never published. */
  path: string | null;
};

/**
 * Read the recorded clone. Writes nothing and synchronizes nothing.
 *
 * `invoke` is injectable so a test can assert *which* commands a render-time
 * read issues — the finding was that it issued a write.
 */
export async function readProjectAgentsRepoWith(
  projectRef: string,
  invoke: <T>(command: string, args: Record<string, unknown>) => Promise<T>,
): Promise<ProjectAgentsRepoStatus> {
  const raw = await invoke<{ recorded: boolean; path: string | null }>(
    "project_agents_repo_status",
    { projectRef },
  );
  return { recorded: raw.recorded === true, path: raw.path ?? null };
}

/** Read the recorded clone through the real bridge. */
export function readProjectAgentsRepo(
  projectRef: string,
): Promise<ProjectAgentsRepoStatus> {
  return readProjectAgentsRepoWith(projectRef, (command, args) =>
    invokeTauri(command, args),
  );
}

/**
 * Establish it: read the project's kind:30624 source, sync the clone, record
 * it. Called only from an explicit control.
 *
 * Returns the sentence to show, or `null` when it succeeded.
 */
export async function prepareProjectAgentsRepo(
  projectRef: string,
): Promise<string | null> {
  const source = await fetchProjectPackSource(projectRef);
  if (!source) {
    return "This project has no role source, so there is nothing to prepare; Finish repository setup under Project settings → Packs.";
  }
  const recorded = await recordProjectAgentsRepo({
    projectRef,
    repo: source.repo,
    ref: source.ref,
    sha: source.sha,
    path: source.path,
  });
  return recorded
    ? null
    : `This project's role source (${source.repo}, path ${source.path}) is not an agents repository, so host steps on this computer cannot find its actions.yml.`;
}

/**
 * The sentence the tab shows for a given read.
 *
 * `undefined` status is unknown — the read has not settled — which is not the
 * same as "nothing is recorded", and the tab must not offer Prepare as though
 * it knew.
 */
export function agentsRepoNote(input: {
  status: ProjectAgentsRepoStatus | undefined;
  error: string | null;
}): string {
  if (input.error) {
    return `This computer's agents-repository record could not be read: ${input.error}`;
  }
  if (!input.status) {
    return "Reading where this computer keeps the project's agents repository…";
  }
  return input.status.recorded
    ? `Host steps on this computer read this project's actions.yml from ${input.status.path ?? "its recorded clone"}.`
    : "This computer has no clone of the project's agents repository recorded, so host steps here cannot find its actions.yml. Nothing has been changed by opening this tab.";
}

/** The read, and the explicit preparation behind it. */
export function useProjectAgentsRepo(projectRef: string | null) {
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: projectAgentsRepoQueryKey(projectRef ?? ""),
    enabled: projectRef !== null,
    queryFn: () => readProjectAgentsRepo(projectRef ?? ""),
  });
  const prepare = useMutation({
    mutationFn: () => prepareProjectAgentsRepo(projectRef ?? ""),
    onSettled: () =>
      queryClient.invalidateQueries({
        queryKey: projectAgentsRepoQueryKey(projectRef ?? ""),
      }),
  });
  return {
    status: query.data,
    note: agentsRepoNote({
      status: query.data,
      error: query.error
        ? query.error instanceof Error
          ? query.error.message
          : String(query.error)
        : null,
    }),
    /** The outcome sentence of the last preparation, or `null`. */
    prepareNote: prepare.data ?? null,
    prepareError: prepare.error
      ? prepare.error instanceof Error
        ? prepare.error.message
        : String(prepare.error)
      : null,
    preparing: prepare.isPending,
    prepare: prepare.mutate,
  };
}
