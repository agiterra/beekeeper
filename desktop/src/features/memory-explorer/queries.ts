import type {
  AgentsRepoFile,
  AgentsRepoListing,
} from "@/shared/api/agentsRepoTypes";
import { invokeTauri } from "@/shared/api/tauri";
import type { DocumentIndex } from "./model";
export type Snapshot = { token: string; listing: AgentsRepoListing };
export const captureSnapshot = (projectRef: string) =>
  invokeTauri<Snapshot>("memory_explorer_snapshot", { projectRef });
export const readSnapshot = (projectRef: string, token: string, path: string) =>
  invokeTauri<AgentsRepoFile>("memory_explorer_read", {
    projectRef,
    token,
    path,
  });
export const releaseSnapshot = (token: string) =>
  invokeTauri<void>("memory_explorer_release", { token }).catch(() => {});

/** Worker lifetime equals reader lifetime; termination rejects all stale parses. */
export function parserWorker() {
  const worker = new Worker(new URL("./worker.ts", import.meta.url), {
    type: "module",
  });
  let sequence = 0;
  const pending = new Map<
    number,
    { resolve: (d: DocumentIndex) => void; reject: (e: Error) => void }
  >();
  worker.onerror = (event) => {
    for (const task of pending.values())
      task.reject(new Error(`Markdown parser unavailable: ${event.message}`));
    pending.clear();
  };
  worker.onmessage = ({ data }) => {
    const task = pending.get(data.id);
    pending.delete(data.id);
    if (data.error) task?.reject(new Error(data.error));
    else task?.resolve(data.document);
  };
  return {
    parse: (path: string, text: string, beekeeper: boolean) =>
      new Promise<DocumentIndex>((resolve, reject) => {
        const id = ++sequence;
        pending.set(id, { resolve, reject });
        worker.postMessage({ id, path, text, beekeeper });
      }),
    close: () => {
      worker.terminate();
      for (const task of pending.values())
        task.reject(new Error("Reader closed"));
      pending.clear();
    },
  };
}
