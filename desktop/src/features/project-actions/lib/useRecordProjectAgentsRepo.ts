import * as React from "react";

import { recordProjectAgentsRepo } from "@/features/projects-container/lib/projectAgentsInit";
import { fetchProjectPackSource } from "@/features/projects-container/lib/projectPackSource";

/**
 * Tell this computer's provider where the project's agents repository is
 * (spec § 4.11): read the project's kind:30624, sync the clone, record it.
 * Runs once per project while the Actions tab is open; the outcome is a
 * disclosed note, never a thrown error — the tab still shows the relay's
 * records when this computer cannot execute a step.
 */
export function useRecordProjectAgentsRepo(projectRef: string | null): {
  /** What to tell the viewer when the provider will not find the file. */
  note: string | null;
} {
  const [note, setNote] = React.useState<string | null>(null);
  React.useEffect(() => {
    if (!projectRef) return;
    let cancelled = false;
    void (async () => {
      try {
        const source = await fetchProjectPackSource(projectRef);
        if (cancelled) return;
        if (!source) {
          setNote(
            "This project has no role source, so host steps on this computer cannot find its actions.yml; Finish repository setup under Project settings → Packs.",
          );
          return;
        }
        const recorded = await recordProjectAgentsRepo({
          projectRef,
          repo: source.repo,
          ref: source.ref,
          sha: source.sha,
          path: source.path,
        });
        if (cancelled) return;
        setNote(
          recorded
            ? null
            : `This project's role source (${source.repo}, path ${source.path}) is not an agents repository, so host steps on this computer cannot find its actions.yml.`,
        );
      } catch (error) {
        if (cancelled) return;
        setNote(
          `This computer could not record the agents repository for host steps: ${error instanceof Error ? error.message : String(error)}`,
        );
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [projectRef]);
  return { note };
}
