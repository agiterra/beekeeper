import * as React from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { useUsersBatchQuery } from "@/features/profile/hooks";
import { useProjectContainerQuery } from "@/features/projects-container/hooks";
import { useProjectRosterQuery } from "@/features/projects-container/lib/projectMembers";
import { useProjectCapabilities } from "@/features/projects-container/lib/projectPermissions";
import { LOCAL_GENERAL_ID } from "@/features/projects-container/lib/projectContainerModel";
import { ProjectPageTabs } from "@/features/projects-container/ui/ProjectPageTabs";
import { todoPerson } from "@/features/project-todos/lib/todoPeople";
import { useIdentityQuery } from "@/shared/api/hooks";
import type { AgentsRepoCommitResult } from "@/shared/api/agentsRepoTypes";
import {
  agentsRepoCommitDrafts,
  validatePlanSource,
} from "@/shared/api/tauriAgentsRepo";
import { useFeatureEnabled } from "@/shared/features";
import { Button } from "@/shared/ui/button";

import {
  agentsRepoCommitAccess,
  agentsRepoDraftAccess,
} from "../lib/agentsRepoAccess";
import { gitBlobSha } from "../lib/agentsRepoBlobSha";
import { agentsRepoCopy as copy } from "../lib/agentsRepoCopy";
import type { DraftPath } from "../lib/agentsRepoDraftFold";
import { useAgentsRepoMutations } from "../lib/agentsRepoMutations";
import { artifactPreviewOpen } from "@/shared/api/tauriAgentsRepo";
import { cn } from "@/shared/lib/cn";

import { draftPathClass } from "../lib/agentsRepoDraftOp";
import { planCommitRefusals } from "../lib/agentsRepoPlanSource";
import { useArtifactPinMutations } from "../lib/artifactPinMutations";
import { useArtifactPins } from "../lib/artifactPinQueries";
import {
  agentsRepoDraftsQueryKey,
  agentsRepoFileQueryKey,
  agentsRepoListingQueryKey,
  useAgentsRepoDrafts,
  useAgentsRepoFile,
  useAgentsRepoListing,
  useAgentsRepoLiveInvalidation,
  useAgentsRepoSource,
} from "../lib/agentsRepoQueries";
import { AgentsRepoCommitDialog, changeOf } from "./AgentsRepoCommitDialog";
import {
  AgentsRepoNewDocumentDialog,
  AgentsRepoNewFolderDialog,
} from "./AgentsRepoNewDocumentDialog";
import { AgentsRepoNewPlanDialog } from "./AgentsRepoNewPlanDialog";
import { AgentsRepoDraftsPanel } from "./AgentsRepoDraftsPanel";
import { AgentsRepoEditor, type EditorSubject } from "./AgentsRepoEditor";
import { AgentsRepoFileTree, treeRows } from "./AgentsRepoFileTree";

/**
 * `/projects/$projectId/files` — the project's agents repository: `main`
 * on the left as this computer last fetched it, one file in the middle
 * with its open draft, the drafts on the right, and Commit (spec § 4.12).
 */
export function ProjectAgentsRepoScreen({
  projectId,
  selectedPath,
  focused = false,
}: {
  projectId: string;
  selectedPath: string | null;
  /**
   * `?view=file`: show that one file alone — its name, its content and its
   * history — the way a pinned sidebar row opens it. No tabs, no tree and
   * none of the repository-wide controls, because none of them is about the
   * document someone pinned.
   *
   * It only applies to a file. A pinned *folder* is a row in the tree, so
   * hiding the tree would hide the thing itself.
   */
  focused?: boolean;
}) {
  const { project } = useProjectContainerQuery(projectId);
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const explorerEnabled = useFeatureEnabled("memory-explorer");
  const pulseEnabled = useFeatureEnabled("project-pulse");
  const coordinate =
    project && project.id !== LOCAL_GENERAL_ID && project.owner.length > 0
      ? project.address
      : null;

  const sourceQuery = useAgentsRepoSource(coordinate);
  const source = sourceQuery.data ?? null;
  const repo = source?.repo ?? null;
  const isAgentsRepo =
    source !== null && source.path === "." && source.ref !== null;
  useAgentsRepoLiveInvalidation(coordinate, isAgentsRepo ? source : null);

  const listing = useAgentsRepoListing(coordinate, isAgentsRepo);
  const drafts = useAgentsRepoDrafts(coordinate, repo);
  const file = useAgentsRepoFile(
    coordinate,
    isAgentsRepo ? selectedPath : null,
  );

  const identity = useIdentityQuery();
  const self = identity.data?.pubkey ?? null;
  const capabilities = useProjectCapabilities(project);
  const rosterQuery = useProjectRosterQuery(project);
  const roster = rosterQuery.data ?? project?.members ?? [];
  const draftAccess = agentsRepoDraftAccess(
    self,
    project,
    roster,
    capabilities,
  );
  const commitAccess = agentsRepoCommitAccess(
    self,
    project,
    roster,
    capabilities,
  );

  const pubkeys = React.useMemo(() => {
    const set = new Set<string>();
    for (const entry of drafts.read?.digest.paths ?? []) {
      set.add(entry.head.author);
      for (const row of entry.superseded) set.add(row.author);
    }
    for (const record of drafts.read?.digest.commits ?? []) set.add(record.by);
    return [...set];
  }, [drafts.read]);
  const profiles = useUsersBatchQuery(pubkeys);
  const profileMap = profiles.data?.profiles;
  const personName = React.useCallback(
    (pubkey: string) => todoPerson(pubkey, profileMap).name,
    [profileMap],
  );
  const isSelf = React.useCallback(
    (pubkey: string) =>
      self !== null && pubkey.toLowerCase() === self.toLowerCase(),
    [self],
  );

  const mutations = useAgentsRepoMutations(coordinate, repo, personName);

  // Pins are a second op log over the same repository (NIP-AR). The tab reads
  // them so a row can show its pin and the control can toggle it, and the
  // sidebar reads the same query key — so a pin toggled here moves the
  // sidebar row on the same render.
  const pins = useArtifactPins(coordinate, repo);
  const pinMutations = useArtifactPinMutations(coordinate, repo);
  const pinnedTargets = React.useMemo(() => {
    const set = new Set<string>();
    for (const row of pins.read?.digest.pins ?? []) {
      if (row.pinned) set.add(row.target);
    }
    return set;
  }, [pins.read]);
  const isPinned = React.useCallback(
    (target: string) => pinnedTargets.has(target),
    [pinnedTargets],
  );
  // A folder keep is never the pinned thing: the folder it holds open is
  // (NIP-AR), so the control on an open keep pins the folder.
  const pinTargetOf = React.useCallback(
    (path: string) =>
      path.endsWith("/.gitkeep") ? path.slice(0, -"/.gitkeep".length) : path,
    [],
  );
  // What the preview could not find, said by name rather than left to render
  // as a broken image.
  const [previewNotice, setPreviewNotice] = React.useState<string | null>(null);
  const openPreview = React.useCallback(
    async (text: string) => {
      if (coordinate === null || selectedPath === null) return;
      setPreviewNotice(null);
      try {
        const handle = await artifactPreviewOpen(
          coordinate,
          selectedPath,
          text,
        );
        setPreviewNotice(
          handle.missing.length > 0
            ? copy.previewMissing(handle.missing)
            : null,
        );
      } catch (caught) {
        setPreviewNotice(
          copy.previewFailed(
            caught instanceof Error ? caught.message : String(caught),
          ),
        );
      }
    },
    [coordinate, selectedPath],
  );

  const select = React.useCallback(
    (path: string | null) => {
      void navigate({
        to: "/projects/$projectId/files",
        params: { projectId },
        search: path ? { path } : {},
        replace: true,
      });
    },
    [navigate, projectId],
  );

  const rows = React.useMemo(
    () =>
      treeRows(listing.data?.entries ?? [], drafts.read?.digest.paths ?? []),
    [listing.data, drafts.read],
  );
  const draftFor = React.useCallback(
    (path: string): DraftPath | null =>
      drafts.read?.digest.paths.find((entry) => entry.path === path) ?? null,
    [drafts.read],
  );
  const blobFor = React.useCallback(
    (path: string): string | null =>
      listing.data?.entries.find((entry) => entry.path === path)?.blob ?? null,
    [listing.data],
  );

  const subject: EditorSubject | null = selectedPath
    ? {
        path: selectedPath,
        main: file.data ?? null,
        mainError:
          file.error instanceof Error
            ? file.error.message
            : file.error
              ? String(file.error)
              : null,
        draft: draftFor(selectedPath),
        tip: listing.data?.commit ?? null,
      }
    : null;

  // "identical to main": the head draft's text hashes to the tip's blob.
  const [identical, setIdentical] = React.useState(false);
  React.useEffect(() => {
    let cancelled = false;
    const head = subject?.draft?.head;
    const blob = subject?.main?.blob ?? null;
    const text = head?.op === "file.put" ? head.text : null;
    if (text === null || blob === null) {
      setIdentical(false);
      return;
    }
    void gitBlobSha(text).then((sha) => {
      if (!cancelled) setIdentical(sha === blob);
    });
    return () => {
      cancelled = true;
    };
  }, [subject?.draft?.head, subject?.main?.blob]);

  const [busy, setBusy] = React.useState(false);
  const [commitOpen, setCommitOpen] = React.useState(false);
  const [commitResult, setCommitResult] =
    React.useState<AgentsRepoCommitResult | null>(null);
  const [recordError, setRecordError] = React.useState<string | null>(null);
  const pendingRecord = React.useRef<{
    commit: string;
    paths: string[];
    drafts: string[];
    message: string | null;
  } | null>(null);

  const publishRecord = React.useCallback(async () => {
    const pending = pendingRecord.current;
    if (!pending) return;
    try {
      await mutations.recordCommit(pending);
      pendingRecord.current = null;
      setRecordError(null);
    } catch (caught) {
      setRecordError(caught instanceof Error ? caught.message : String(caught));
    }
  }, [mutations]);

  const onCommit = React.useCallback(
    async (chosen: DraftPath[], message: string) => {
      if (coordinate === null || !drafts.read) return;
      setBusy(true);
      setRecordError(null);
      try {
        const refusals = await planCommitRefusals(
          chosen.map(changeOf),
          validatePlanSource,
        );
        if (refusals.length > 0) throw new Error(refusals.join(" "));
        const result = await agentsRepoCommitDrafts({
          projectRef: coordinate,
          expectedTip: listing.data?.commit ?? null,
          message,
          drafts: chosen.map(changeOf),
          authors: pubkeys.map((pubkey) => ({
            pubkey,
            name: personName(pubkey),
          })),
        });
        setCommitResult(result);
        if (result.pushed === "yes" && result.commit) {
          // Close the whole chain of each landed path — head and superseded.
          const paths = new Set<string>();
          const ids = new Set<string>();
          for (const entry of chosen) {
            paths.add(entry.head.path);
            if (entry.head.to) paths.add(entry.head.to);
            for (const e of drafts.read.digest.paths.filter(
              (p) => p.path === entry.head.path,
            )) {
              ids.add(e.head.id);
              for (const row of e.superseded) ids.add(row.id);
            }
          }
          pendingRecord.current = {
            commit: result.commit,
            paths: [...paths],
            drafts: [...ids],
            message: message.trim() ? message.trim() : null,
          };
          await publishRecord();
          toast.success(
            copy.pushedYes(result.paths.length, result.commit.slice(0, 8)),
          );
        }
        if (result.pushed !== "no") {
          void queryClient.invalidateQueries({
            queryKey: agentsRepoListingQueryKey(coordinate),
          });
          void queryClient.invalidateQueries({
            queryKey: ["agents-repo-file", coordinate],
          });
        }
      } catch (caught) {
        toast.error(caught instanceof Error ? caught.message : String(caught));
      } finally {
        setBusy(false);
      }
    },
    [
      coordinate,
      drafts.read,
      listing.data,
      pubkeys,
      personName,
      publishRecord,
      queryClient,
    ],
  );

  // A pinned target the repository does not have — neither a file on main nor
  // a draft, and for a folder nothing under its prefix. Said by name, because
  // the alternative is a sidebar row that opens nothing and never says why.
  /**
   * Whether to paint the one-file view: asked for by the route, and only for
   * a path the layout calls a file. A hand-typed `?view=file` naming a folder
   * (or a folder's keep, which stands for the folder) falls back to the whole
   * tab — hiding the tree there would hide the thing the path names, and a
   * dead end is worse than too much chrome.
   */
  const onlyThisFile = React.useMemo(() => {
    if (!focused || selectedPath === null) return false;
    const classified = draftPathClass(selectedPath);
    return classified.ok && classified.class !== "document-folder";
  }, [focused, selectedPath]);

  const missingPins = React.useMemo(() => {
    const present = new Set(rows.map((row) => row.path));
    const prefixes = [...present];
    return (pins.read?.digest.pins ?? [])
      .filter((pin) => pin.pinned)
      .filter((pin) =>
        pin.targetKind === "folder"
          ? !prefixes.some((path) => path.startsWith(`${pin.target}/`))
          : !present.has(pin.target),
      )
      .map((pin) => pin.target);
  }, [pins.read, rows]);
  const [newPlanOpen, setNewPlanOpen] = React.useState(false);
  const onNewPlan = React.useCallback(() => setNewPlanOpen(true), []);
  const [newDocumentOpen, setNewDocumentOpen] = React.useState(false);
  const [newFolderOpen, setNewFolderOpen] = React.useState(false);
  // A new document defaults to the folder the tree has selected, so creating
  // the second document in a folder does not mean typing its name again.
  const selectedFolder = React.useMemo(() => {
    if (selectedPath === null) return "";
    const classified = draftPathClass(selectedPath);
    if (classified.ok && classified.class === "document-folder") {
      return selectedPath.replace(/\/[.]gitkeep$/, "").replace(/^docs\//, "");
    }
    if (!selectedPath.startsWith("docs/")) return "";
    const folder = selectedPath.slice("docs/".length);
    const at = folder.lastIndexOf("/");
    return at < 0 ? "" : folder.slice(0, at);
  }, [selectedPath]);

  if (!project) {
    return (
      <div
        className="flex flex-1 items-center justify-center p-4"
        data-testid="agents-repo-missing"
      >
        <p className="text-sm text-muted-foreground">Project not found.</p>
      </div>
    );
  }

  const listingError =
    listing.error instanceof Error
      ? listing.error.message
      : listing.error
        ? String(listing.error)
        : null;

  return (
    <div
      className="flex h-full min-h-0 min-w-0 flex-col overflow-y-auto p-4"
      data-testid="agents-repo-screen"
    >
      {onlyThisFile ? null : (
        <>
          <div>
            <h1 className="break-words text-xl font-semibold text-foreground">
              {project.name}
            </h1>
            <ProjectPageTabs
              active="files"
              projectId={project.id}
              showPulse={pulseEnabled}
            />
          </div>
          {explorerEnabled ? (
            <div className="mb-3 flex gap-2">
              <span className="rounded-md bg-accent px-3 py-2 text-sm">
                Files
              </span>
              <Button
                size="sm"
                variant="ghost"
                onClick={() =>
                  void navigate({
                    to: "/projects/$projectId/files",
                    params: { projectId },
                    search: { view: "explore" },
                  })
                }
              >
                Explore
              </Button>
            </div>
          ) : null}
          <p className="mb-3 text-sm text-muted-foreground">{copy.subtitle}</p>
        </>
      )}
      {sourceQuery.isSuccess && source === null ? (
        <p
          className="rounded-md bg-muted/50 px-3 py-2 text-sm"
          data-testid="agents-repo-no-source"
        >
          {copy.noSource}
        </p>
      ) : null}
      {source !== null && !isAgentsRepo ? (
        <p
          className="rounded-md bg-muted/50 px-3 py-2 text-sm"
          data-testid="agents-repo-not-agents-repo"
        >
          This project's source is a pack-layout repository ({source.repo}
          {source.path !== "." ? ` at ${source.path}` : ""}
          {source.sha ? `, pinned to ${source.sha.slice(0, 8)}` : ""}); the
          Files tab edits an agents repository at a repository root following a
          branch.
        </p>
      ) : null}
      {listingError ? (
        <p
          className="mb-2 rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive"
          data-testid="agents-repo-listing-error"
        >
          {copy.mainUnreadable(listingError)}
        </p>
      ) : null}
      {/* One pinned document is not the place to fetch the whole
          repository or start another file, so the row of
          repository-wide controls is not drawn at all — an invisible
          button is still a button. */}
      {onlyThisFile ? null : (
        <div className="mb-2 flex flex-wrap items-center gap-2 text-2xs text-muted-foreground">
          {listing.data ? (
            <span data-testid="agents-repo-tip">
              {listing.data.syncedAt
                ? copy.asFetchedAt(
                    new Date(listing.data.syncedAt).toLocaleString(),
                  )
                : `main at ${listing.data.commit.slice(0, 8)}`}
              {" · "}
              <span className="font-mono">
                {listing.data.commit.slice(0, 8)}
              </span>
            </span>
          ) : null}
          <Button
            data-testid="agents-repo-refresh"
            disabled={listing.isFetching}
            onClick={() => {
              if (coordinate === null) return;
              void queryClient.invalidateQueries({
                queryKey: agentsRepoListingQueryKey(coordinate),
              });
              void queryClient.invalidateQueries({
                queryKey: agentsRepoDraftsQueryKey(coordinate),
              });
              if (selectedPath) {
                void queryClient.invalidateQueries({
                  queryKey: agentsRepoFileQueryKey(coordinate, selectedPath),
                });
              }
            }}
            size="sm"
            type="button"
            variant="ghost"
          >
            {copy.refresh}
          </Button>
          {draftAccess.kind === "writable" && isAgentsRepo ? (
            <>
              <Button
                data-testid="agents-repo-new-plan"
                onClick={onNewPlan}
                size="sm"
                type="button"
                variant="outline"
              >
                {copy.newPlan}
              </Button>
              <Button
                data-testid="agents-repo-new-document"
                onClick={() => setNewDocumentOpen(true)}
                size="sm"
                type="button"
                variant="outline"
              >
                {copy.newDocument}
              </Button>
              <Button
                data-testid="agents-repo-new-folder"
                onClick={() => setNewFolderOpen(true)}
                size="sm"
                type="button"
                variant="outline"
              >
                {copy.newFolder}
              </Button>
            </>
          ) : null}
        </div>
      )}
      <div
        className={cn(
          "grid min-h-0 flex-1 grid-cols-1 gap-4",
          onlyThisFile
            ? "md:grid-cols-[minmax(0,1fr)_16rem]"
            : "md:grid-cols-[14rem_minmax(0,1fr)_16rem]",
        )}
      >
        {onlyThisFile ? null : (
          <div className="min-h-0 overflow-auto">
            <AgentsRepoFileTree
              isPinned={isPinned}
              onSelect={select}
              personName={personName}
              rows={rows}
              selectedPath={selectedPath}
            />
          </div>
        )}
        <div className="flex min-h-0 flex-col">
          {subject ? (
            <AgentsRepoEditor
              access={draftAccess}
              busy={busy}
              identicalToMain={identical}
              isSelf={isSelf}
              key={subject.path}
              onBack={onlyThisFile ? () => select(null) : null}
              onOpenPreview={
                subject.path.endsWith(".html") && coordinate !== null
                  ? openPreview
                  : null
              }
              onTogglePin={async () => {
                setPreviewNotice(null);
                const target = pinTargetOf(subject.path);
                if (isPinned(target)) await pinMutations.unpin(target);
                // The editor knows which it has: an open keep is its folder,
                // anything else is the file. Saying so skips an inference
                // that refuses the one shape the two grammars share.
                else {
                  await pinMutations.pin(target, {
                    kind: subject.path.endsWith("/.gitkeep")
                      ? "folder"
                      : "file",
                  });
                }
              }}
              pinned={
                repo === null ? null : isPinned(pinTargetOf(subject.path))
              }
              previewNotice={previewNotice}
              onArchive={async (message, openedOn) => {
                if (!subject.main?.blob)
                  throw new Error("Only a file on main can be archived.");
                await mutations.moveDraft({
                  path: subject.path,
                  base: subject.main.blob,
                  baseCommit: subject.main.commit,
                  openedOn,
                  message,
                });
              }}
              onDelete={async (message, openedOn) => {
                if (!subject.main?.blob)
                  throw new Error("Only a file on main can be deleted.");
                await mutations.deleteDraft({
                  path: subject.path,
                  base: subject.main.blob,
                  baseCommit: subject.main.commit,
                  openedOn,
                  message,
                });
              }}
              onSave={async (text, message, openedOn) => {
                await mutations.saveDraft({
                  path: subject.path,
                  text,
                  base: subject.main?.blob ?? null,
                  baseCommit: subject.main?.commit ?? null,
                  openedOn,
                  message,
                });
              }}
              onWithdraw={mutations.withdrawDraft}
              personName={personName}
              subject={subject}
            />
          ) : (
            <p
              className="text-sm text-muted-foreground"
              data-testid="agents-repo-nothing-selected"
            >
              Pick a file, or start a new plan.
            </p>
          )}
        </div>
        <div className="min-h-0 overflow-auto">
          <AgentsRepoDraftsPanel
            commitAccess={commitAccess}
            digest={drafts.read?.digest ?? null}
            onCommit={() => {
              setCommitResult(null);
              setCommitOpen(true);
            }}
            onOpen={select}
            personName={personName}
            truncated={drafts.read?.truncated ?? false}
          />
          {pins.read?.truncated ? (
            <p
              className="mt-2 text-2xs text-muted-foreground"
              data-testid="agents-repo-pins-truncated"
            >
              {copy.pinsTruncated}
            </p>
          ) : null}
          {pins.read && pins.read.digest.ranksWithoutPin > 0 ? (
            <p
              className="mt-2 text-2xs text-muted-foreground"
              data-testid="agents-repo-pins-ranks-without-pin"
            >
              {copy.pinsRanksWithoutPin(pins.read.digest.ranksWithoutPin)}
            </p>
          ) : null}
          {missingPins.length > 0 ? (
            <p
              className="mt-2 rounded-md bg-amber-500/10 px-2 py-1 text-2xs text-amber-800 dark:text-amber-200"
              data-testid="agents-repo-pins-missing"
            >
              {`${copy.pinMissing} ${missingPins.join(", ")}`}
            </p>
          ) : null}
          {drafts.kind === "error" ? (
            <p
              className="mt-2 text-xs text-destructive"
              data-testid="agents-repo-drafts-error"
            >
              {drafts.message}
            </p>
          ) : null}
        </div>
      </div>
      {drafts.read ? (
        <AgentsRepoCommitDialog
          blobFor={blobFor}
          busy={busy}
          digest={drafts.read.digest}
          onCommit={onCommit}
          onOpenChange={(open) => {
            setCommitOpen(open);
            if (!open) setCommitResult(null);
          }}
          onRetryRecord={() => void publishRecord()}
          open={commitOpen}
          personName={personName}
          recordError={recordError}
          result={commitResult}
          tip={listing.data?.commit ?? null}
        />
      ) : null}
      <AgentsRepoNewPlanDialog
        onCreate={select}
        onOpenChange={setNewPlanOpen}
        open={newPlanOpen}
      />
      <AgentsRepoNewDocumentDialog
        initialFolder={selectedFolder}
        onCreate={select}
        onOpenChange={setNewDocumentOpen}
        open={newDocumentOpen}
      />
      <AgentsRepoNewFolderDialog
        onCreate={(keepPath) => {
          // The keep opens like any other draft: an empty folder is a saved
          // empty file, so the person can see what they are about to commit.
          select(keepPath);
        }}
        onOpenChange={setNewFolderOpen}
        open={newFolderOpen}
      />
    </div>
  );
}
