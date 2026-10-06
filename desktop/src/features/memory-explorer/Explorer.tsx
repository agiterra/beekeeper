import * as React from "react";
import { useQuery } from "@tanstack/react-query";
import { fetchProjectPackSource } from "@/features/projects-container/lib/projectPackSource";
import { useNavigate } from "@tanstack/react-router";
import { useProjectContainerQuery } from "@/features/projects-container/hooks";
import { ProjectPageTabs } from "@/features/projects-container/ui/ProjectPageTabs";
import { useCommunities } from "@/features/communities/useCommunities";
import { useIdentityQuery } from "@/shared/api/hooks";
import { relayClient } from "@/shared/api/relayClient";
import {
  KIND_REPO_STATE,
  KIND_PROJECT_PACK_SOURCE,
} from "@/shared/constants/kinds";
import { useFeatureEnabled } from "@/shared/features";
import { Button } from "@/shared/ui/button";
import { Markdown } from "@/shared/ui/markdown";
import {
  connect,
  localTarget,
  type DocumentIndex,
  type SourceNode,
} from "./model";
import {
  captureSnapshot,
  parserWorker,
  readSnapshot,
  releaseSnapshot,
  type Snapshot,
} from "./queries";
import "./explorer.css";

export function MemoryExplorer({ projectId }: { projectId: string }) {
  const { project } = useProjectContainerQuery(projectId);
  const { activeCommunity } = useCommunities();
  const identity = useIdentityQuery();
  const navigate = useNavigate();
  const pulse = useFeatureEnabled("project-pulse");
  const projectAddress = project?.address;
  const scope = `${activeCommunity?.relayUrl}|${identity.data?.pubkey}|${project?.address}`;
  const [sourceVersion, setSourceVersion] = React.useState(0);
  const source = useQuery({
    queryKey: ["memory-explorer-source", scope, sourceVersion],
    enabled: !!project && !!identity.data?.pubkey,
    queryFn: () => fetchProjectPackSource(project?.address ?? ""),
    staleTime: 0,
    retry: false,
  });
  React.useEffect(() => {
    if (!projectAddress) return;
    let disposed = false;
    let close: (() => Promise<void>) | null = null;
    void relayClient
      .subscribeLive(
        {
          kinds: [KIND_PROJECT_PACK_SOURCE],
          "#d": [projectAddress],
          since: Math.floor(Date.now() / 1000),
          limit: 1,
        },
        () => {
          if (!disposed) setSourceVersion((v) => v + 1);
        },
      )
      .then((handle) => {
        if (disposed) void handle?.();
        else close = handle;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      void close?.();
    };
  }, [projectAddress]);
  if (!project || !identity.data?.pubkey) return <p>Loading project access…</p>;
  return (
    <div className="flex min-h-0 flex-1 flex-col p-4">
      <h1 className="text-xl font-semibold">{project.name}</h1>
      <ProjectPageTabs active="files" projectId={projectId} showPulse={pulse} />
      <div className="my-3 flex items-center gap-2">
        <Button
          variant="ghost"
          size="sm"
          onClick={() =>
            void navigate({
              to: "/projects/$projectId/files",
              params: { projectId },
              search: {},
            })
          }
        >
          Files
        </Button>
        <span className="rounded-md bg-accent px-3 py-2 text-sm">Explore</span>
        <span className="text-xs text-muted-foreground">
          Experimental · committed content only
        </span>
      </div>
      {source.isPending ? (
        <p>Checking project source…</p>
      ) : source.isError ? (
        <p role="alert">{String(source.error)}</p>
      ) : !source.data ? (
        <p>
          This project has no configured agents repository. Set its source in
          Project settings → Packs.
        </p>
      ) : (
        <ExplorerReader
          key={`${scope}|${source.data.eventId}|${sourceVersion}`}
          projectRef={project.address}
          openArtifact={(path) =>
            void navigate({
              to: "/projects/$projectId/files",
              params: { projectId },
              search: { path },
            })
          }
        />
      )}
    </div>
  );
}

type Loaded = {
  snapshot: Snapshot;
  documents: DocumentIndex[];
  outcomes: Record<string, string>;
  partial: string[];
};
const MAX_INDEX_BYTES = 16 * 1024 * 1024;

function ExplorerReader({
  projectRef,
  openArtifact,
}: {
  projectRef: string;
  openArtifact: (path: string) => void;
}) {
  const [loaded, setLoaded] = React.useState<Loaded | null>(null);
  const [busy, setBusy] = React.useState(true);
  const [error, setError] = React.useState<string | null>(null);
  const [selected, setSelected] = React.useState("");
  const [history, setHistory] = React.useState<string[]>([]);
  const [query, setQuery] = React.useState("");
  const [source, setSource] = React.useState(false);
  const [compare, setCompare] = React.useState<SourceNode | null>(null);
  const [more, setMore] = React.useState(30);
  const [stale, setStale] = React.useState(false);
  const [refresh, setRefresh] = React.useState(0);
  const workerRef = React.useRef<ReturnType<typeof parserWorker> | null>(null);
  const loadedRef = React.useRef<Loaded | null>(null);
  const generation = React.useRef(0);
  const selectedRef = React.useRef(selected);
  selectedRef.current = selected;
  const pending = React.useRef(new Set<string>());

  React.useEffect(() => {
    const current = ++generation.current;
    void refresh;
    let disposed = false;
    let token: string | null = null;
    const worker = parserWorker();
    workerRef.current = worker;
    setBusy(true);
    setError(null);
    void (async () => {
      const snapshot = await captureSnapshot(projectRef);
      token = snapshot.token;
      if (disposed) {
        void releaseSnapshot(token);
        return;
      }
      const entries = snapshot.listing.entries.filter((e) =>
        /\.md(?:own)?$/i.test(e.path),
      );
      const beekeeper =
        entries.some((e) => e.path === "plans/CURRENT_STATE.md") &&
        entries.some((e) => e.path === "plans/SESSION_STATE.md");
      const initial =
        selectedRef.current.split("#")[0] ||
        (beekeeper
          ? "plans/CURRENT_STATE.md"
          : (entries.find((e) => /^readme\.md$/i.test(e.path))?.path ??
            entries[0]?.path));
      const queue = [
        ...new Set(
          [
            initial,
            ...(beekeeper
              ? ["plans/SESSION_STATE.md", "plans/SESSION_VIEW_PARITY_PLAN.md"]
              : []),
            ...entries
              .filter(
                (e) => !e.path.includes("/") || /^plans\/[^/]+$/.test(e.path),
              )
              .map((e) => e.path),
          ].filter((p): p is string => !!p),
        ),
      ];
      const documents: DocumentIndex[] = [];
      const outcomes: Record<string, string> = {};
      const partial: string[] = [];
      let bytes = 0;
      for (let i = 0; i < queue.length; i++) {
        if (disposed) return;
        const path = queue[i];
        const entry = entries.find((e) => e.path === path);
        if (!entry || documents.some((d) => d.path === path) || outcomes[path])
          continue;
        if (documents.length >= 256 || bytes + entry.size > MAX_INDEX_BYTES) {
          partial.push("Index budget: 256 files / 16 MiB");
          break;
        }
        const file = await readSnapshot(projectRef, snapshot.token, path);
        if (disposed) return;
        if (file.commit !== snapshot.listing.commit)
          throw new Error("Snapshot mismatch; refresh required");
        if (file.text === null) {
          outcomes[path] =
            `${file.state}${file.size ? ` (${file.size.toLocaleString()} bytes)` : ""}`;
          continue;
        }
        let document: DocumentIndex;
        try {
          document = await worker.parse(path, file.text, beekeeper);
        } catch (error) {
          if (disposed) return;
          outcomes[path] = `Parse unavailable: ${String(error)}`;
          continue;
        }
        documents.push(document);
        bytes += entry.size;
        const links = document.nodes
          .flatMap((n) => n.references)
          .filter((r) => r.basis === "markdown-link")
          .map((r) => r.target.split("#")[0]);
        queue.splice(
          Math.max(i + 1, beekeeper ? 3 : 1),
          0,
          ...links.filter((p) => !queue.includes(p)),
        );
      }
      if (disposed || current !== generation.current) return;
      if (documents.length < entries.length)
        partial.push(
          `${entries.length - documents.length} Markdown files unindexed; open a file to load it`,
        );
      const next = {
        snapshot,
        documents,
        outcomes,
        partial: [...new Set(partial)],
      };
      loadedRef.current = next;
      setLoaded(next);
      setStale(false);
      setCompare(null);
      const previous = selectedRef.current;
      const exists = documents
        .flatMap((d) => d.nodes)
        .some((n) => n.id === previous);
      setSelected(exists ? previous : `${initial ?? ""}#`);
      if (previous && !exists)
        setError(
          "The selected item moved or disappeared; its document is shown instead.",
        );
      setBusy(false);
    })().catch((e) => {
      if (!disposed) {
        setError(String(e));
        setBusy(false);
      }
    });
    return () => {
      disposed = true;
      generation.current++;
      worker.close();
      if (workerRef.current === worker) workerRef.current = null;
      if (token) void releaseSnapshot(token);
    };
  }, [projectRef, refresh]);

  const subscribedRepo = loaded?.snapshot.listing.repo;
  React.useEffect(() => {
    if (!subscribedRepo) return;
    const repoId = subscribedRepo.split(":").slice(2).join(":");
    let disposed = false;
    let unsubscribe: (() => Promise<void>) | null = null;
    void relayClient
      .subscribeLive(
        {
          kinds: [KIND_REPO_STATE],
          "#d": [repoId],
          limit: 1,
          since: Math.floor(Date.now() / 1000),
        },
        () => {
          if (!disposed) setStale(true);
        },
      )
      .then((handle) => {
        if (disposed) void handle?.();
        else unsubscribe = handle;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      void unsubscribe?.();
    };
  }, [subscribedRepo]);

  const nodes = React.useMemo(
    () => loaded?.documents.flatMap((d) => d.nodes) ?? [],
    [loaded],
  );
  const edges = React.useMemo(() => connect(loaded?.documents ?? []), [loaded]);
  const active = nodes.find((n) => n.id === selected);
  const activeDocument = loaded?.documents.find((d) => d.path === active?.path);
  const neighbors = active
    ? [
        ...new Set(
          edges
            .filter((e) => e.from === active.id)
            .flatMap((e) => e.to)
            .concat(
              edges.filter((e) => e.to.includes(active.id)).map((e) => e.from),
            ),
        ),
      ].filter((id) => id !== active.id)
    : [];
  const connections = edges.filter((e) => e.from === selected);
  const needle = query.trim().toLowerCase();
  const results = needle
    ? nodes
        .filter((n) =>
          `${n.title}\n${n.path}\n${n.text}`.toLowerCase().includes(needle),
        )
        .slice(0, 100)
    : [];

  const select = async (id: string, back = false) => {
    if (!loadedRef.current || busy) return;
    setError(null);
    setMore(30);
    setSource(false);
    if (!back && selected) setHistory((h) => [...h, selected].slice(-100));
    setSelected(id);
    const path = id.split("#")[0];
    const state = loadedRef.current;
    if (state.documents.some((d) => d.path === path)) {
      if (!state.documents.flatMap((d) => d.nodes).some((n) => n.id === id))
        setError(
          `Missing heading or item: ${id}. Select a passage in the outline.`,
        );
      return;
    }
    if (pending.current.has(path)) return;
    const entry = state.snapshot.listing.entries.find((e) => e.path === path);
    if (!entry) {
      setError(`Missing document: ${path}`);
      return;
    }
    pending.current.add(path);
    const current = generation.current;
    try {
      const file = await readSnapshot(projectRef, state.snapshot.token, path);
      if (current !== generation.current) return;
      if (file.text === null) {
        setError(`${path}: ${file.state} (${file.size ?? "unknown"} bytes)`);
        return;
      }
      const beekeeper =
        state.snapshot.listing.entries.some(
          (e) => e.path === "plans/CURRENT_STATE.md",
        ) &&
        state.snapshot.listing.entries.some(
          (e) => e.path === "plans/SESSION_STATE.md",
        );
      const document = await workerRef.current?.parse(
        path,
        file.text,
        beekeeper,
      );
      if (!document || current !== generation.current) return;
      const latest = loadedRef.current;
      if (!latest || latest.snapshot.token !== state.snapshot.token) return;
      const size = latest.documents.reduce(
        (n, d) =>
          n +
          (state.snapshot.listing.entries.find((e) => e.path === d.path)
            ?.size ?? 0),
        0,
      );
      const documents =
        size + entry.size > MAX_INDEX_BYTES || latest.documents.length >= 256
          ? [document]
          : [...latest.documents, document];
      const next = {
        ...latest,
        documents,
        partial:
          documents.length === 1
            ? [
                "Partially indexed: earlier files evicted to keep the 16 MiB budget",
              ]
            : latest.partial,
      };
      loadedRef.current = next;
      setLoaded(next);
      if (!document.nodes.some((n) => n.id === id))
        setError(
          `Missing heading or item: ${id}. Select a passage in the outline.`,
        );
    } catch (e) {
      if (current === generation.current) setError(String(e));
    } finally {
      pending.current.delete(path);
    }
  };

  const nodeButton = (node: SourceNode) => (
    <button
      type="button"
      key={node.id}
      className={`explorer-node ${selected === node.id ? "explorer-selected" : ""}`}
      onClick={() => void select(node.id)}
    >
      <span className="text-2xs uppercase tracking-wide text-muted-foreground">
        {node.type}
      </span>
      <strong className="block line-clamp-3 text-sm">{node.title}</strong>
      <span className="block truncate text-xs text-muted-foreground">
        {node.path}:{node.start}
      </span>
    </button>
  );
  return (
    <section
      className="explorer flex min-h-0 flex-1 flex-col"
      data-testid="memory-explorer"
    >
      <div className="flex flex-wrap items-center gap-2 border-b pb-3 text-xs text-muted-foreground">
        <Button
          size="sm"
          variant="outline"
          disabled={!history.length || busy}
          onClick={() => {
            const id = history.at(-1);
            setHistory((h) => h.slice(0, -1));
            if (id) void select(id, true);
          }}
        >
          Back
        </Button>
        <span
          className="min-w-0 break-all"
          title={loaded?.snapshot.listing.commit}
        >
          Snapshot {loaded?.snapshot.listing.commit.slice(0, 12) ?? "loading"} ·{" "}
          {loaded?.snapshot.listing.syncedAt ?? "fetch date unknown"}
        </span>
        <Button
          size="sm"
          variant="ghost"
          disabled={busy}
          onClick={() => setRefresh((n) => n + 1)}
        >
          Refresh
        </Button>
        {stale && (
          <strong className="text-amber-600">New version available</strong>
        )}
        {busy && (
          <span role="status">Reading and indexing committed documents…</span>
        )}
      </div>
      {error && (
        <p
          role="alert"
          className="my-2 rounded bg-destructive/10 p-2 text-sm text-destructive"
        >
          {error}
        </p>
      )}
      {loaded?.partial.length ? (
        <p className="my-2 text-xs text-muted-foreground">
          Partially indexed · {loaded.partial.join(" · ")}
        </p>
      ) : null}
      <div className="explorer-panes">
        <aside className="explorer-list">
          <label className="text-xs font-medium" htmlFor="explorer-search">
            Search indexed documents
          </label>
          <input
            id="explorer-search"
            className="my-2 w-full rounded-md border bg-background p-2 text-sm"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Find a passage, path or ID…"
          />
          {needle ? (
            <>
              <p className="mb-2 text-xs text-muted-foreground">
                {results.length
                  ? `${results.length} results (up to 100)`
                  : "No matches in indexed documents"}
              </p>
              {results.map((n) => (
                <div key={n.id}>
                  {nodeButton(n)}
                  <p className="mb-2 line-clamp-2 text-xs text-muted-foreground">
                    {n.text.slice(
                      Math.max(0, n.text.toLowerCase().indexOf(needle) - 50),
                      Math.max(0, n.text.toLowerCase().indexOf(needle) - 50) +
                        180,
                    )}
                  </p>
                </div>
              ))}
            </>
          ) : (
            loaded?.snapshot.listing.entries
              .filter((e) => /\.md(?:own)?$/i.test(e.path))
              .map((e) => {
                const document = loaded.documents.find(
                  (d) => d.path === e.path,
                );
                const node = document?.nodes[0];
                return (
                  <button
                    key={e.path}
                    type="button"
                    className="explorer-file"
                    onClick={() => void select(`${e.path}#`)}
                  >
                    <strong className="block text-sm">
                      {node?.title ??
                        e.path
                          .split("/")
                          .at(-1)
                          ?.replace(/\.md$/i, "")
                          .replaceAll("_", " ")}
                    </strong>
                    <span className="block break-all text-xs text-muted-foreground">
                      {e.path}
                    </span>
                    <span className="text-2xs text-muted-foreground">
                      {loaded.outcomes[e.path] ??
                        (document ? "indexed" : "not indexed")}
                    </span>
                  </button>
                );
              })
          )}
          {!busy && !loaded?.snapshot.listing.entries.length && (
            <p className="text-sm">
              No documents available in this repository.
            </p>
          )}
        </aside>
        <div className="explorer-connections">
          <h2 className="mb-3 text-xs font-semibold uppercase tracking-widest text-muted-foreground">
            Connections
          </h2>
          {active ? (
            <>
              {nodeButton(active)}
              <p className="my-3 text-xs text-muted-foreground">
                Explicit references and backlinks · {neighbors.length} neighbors
              </p>
              {neighbors.slice(more - 30, more - 1).map((id) => {
                const node = nodes.find((n) => n.id === id);
                return node ? nodeButton(node) : null;
              })}
              {neighbors.length > more - 1 && (
                <Button size="sm" onClick={() => setMore((n) => n + 29)}>
                  More
                </Button>
              )}
              {connections
                .filter((e) => e.to.length !== 1)
                .map((e) => (
                  <div
                    className="my-2 rounded border border-amber-500/30 p-2 text-xs"
                    key={`${e.target}-${e.line}`}
                  >
                    <p>
                      {e.to.length
                        ? "Ambiguous reference"
                        : "Unresolved or unindexed reference"}
                      : {e.target} (line {e.line})
                    </p>
                    {e.to.length
                      ? e.to.map((id) => (
                          <button
                            className="block underline"
                            type="button"
                            key={id}
                            onClick={() => void select(id)}
                          >
                            {id}
                          </button>
                        ))
                      : !e.target.startsWith("sv:") && (
                          <button
                            type="button"
                            className="underline"
                            onClick={() => void select(e.target)}
                          >
                            Open target
                          </button>
                        )}
                  </div>
                ))}
            </>
          ) : (
            <p className="text-sm text-muted-foreground">
              Select a document or search result.
            </p>
          )}
        </div>
        <main
          className="explorer-reader"
          onClickCapture={(e) => {
            const a = (e.target as HTMLElement).closest("a");
            const href = a?.getAttribute("href");
            if (!href || !active) return;
            const target = localTarget(active.path, href);
            if (target) {
              e.preventDefault();
              e.stopPropagation();
              void select(target);
            }
          }}
        >
          {active ? (
            <>
              <div className="mb-3 flex flex-wrap items-center gap-2">
                <h2 className="w-full text-lg font-semibold">{active.title}</h2>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => setSource((v) => !v)}
                >
                  {source ? "Read" : "Source"}
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => setCompare(compare ? null : active)}
                >
                  {compare ? "Clear comparison" : "Compare"}
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => openArtifact(active.path)}
                >
                  Open in Artifacts
                </Button>
              </div>
              <p className="mb-3 break-all font-mono text-2xs text-muted-foreground">
                {loaded?.snapshot.listing.repo} @{" "}
                {loaded?.snapshot.listing.commit} · {active.path}:{active.start}
                –{active.end}
              </p>
              {active.claim && (
                <details className="mb-3 rounded-md border p-3 text-xs">
                  <summary>
                    Source status claim ·{" "}
                    {/^(open|closed|landed|seen)\b/i.exec(active.claim)?.[1] ??
                      "unknown wording"}
                  </summary>
                  <Markdown content={active.claim} />
                </details>
              )}
              {compare && compare.id !== active.id && (
                <div className="mb-4 rounded-md border p-3">
                  <h3 className="text-sm font-semibold">
                    Source claims side by side
                  </h3>
                  {compare.fragment === active.fragment &&
                    compare.claim &&
                    active.claim &&
                    /^(open|closed|landed|seen)\b/i
                      .exec(compare.claim)?.[1]
                      ?.toLowerCase() !==
                      /^(open|closed|landed|seen)\b/i
                        .exec(active.claim)?.[1]
                        ?.toLowerCase() && (
                      <p className="text-xs">Claims differ</p>
                    )}
                  <p className="text-xs text-muted-foreground">
                    Different passages can describe different dates. Neither
                    claim is overwritten.
                  </p>
                  <div className="mt-2 grid gap-3 sm:grid-cols-2">
                    <div>
                      <p className="break-all text-xs">
                        {compare.path}:{compare.start}–{compare.end}
                      </p>
                      <Markdown content={compare.claim ?? compare.text} />
                    </div>
                    <div>
                      <p className="break-all text-xs">
                        {active.path}:{active.start}–{active.end}
                      </p>
                      <Markdown content={active.claim ?? active.text} />
                    </div>
                  </div>
                </div>
              )}
              {activeDocument?.frontmatter && (
                <details className="mb-3 text-xs">
                  <summary>Authored YAML frontmatter</summary>
                  <pre className="overflow-auto whitespace-pre-wrap">
                    {activeDocument.frontmatter}
                  </pre>
                </details>
              )}
              <details className="mb-4" open>
                <summary className="text-xs font-medium">
                  Passage outline
                </summary>
                <select
                  aria-label="Passage outline"
                  className="mt-2 w-full rounded border bg-background p-2 text-sm"
                  value={active.id}
                  onChange={(e) => void select(e.target.value)}
                >
                  {activeDocument?.nodes.map((n) => (
                    <option key={`${n.id}-${n.start}`} value={n.id}>
                      {n.title} · lines {n.start}–{n.end}
                    </option>
                  ))}
                </select>
              </details>
              {source ? (
                <pre
                  className="whitespace-pre-wrap break-words rounded bg-muted/40 p-3 font-mono text-xs"
                  data-testid="explorer-source"
                >
                  {active.text}
                </pre>
              ) : (
                <Markdown content={active.readText ?? active.text} />
              )}
              <div className="mt-5 flex gap-2">
                {["Previous passage", "Next passage"].map((label, i) => {
                  const outline = activeDocument?.nodes ?? [];
                  const neighbor =
                    outline[outline.indexOf(active) + (i ? 1 : -1)];
                  return (
                    <Button
                      size="sm"
                      variant="outline"
                      key={label}
                      disabled={!neighbor}
                      onClick={() => neighbor && void select(neighbor.id)}
                    >
                      {label}
                    </Button>
                  );
                })}
              </div>
            </>
          ) : (
            <p className="text-sm text-muted-foreground">
              {busy
                ? "Loading snapshot…"
                : "Choose a source passage. Missing references remain unresolved."}
            </p>
          )}
        </main>
      </div>
    </section>
  );
}
