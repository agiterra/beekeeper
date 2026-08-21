import * as React from "react";

import { deriveRelayCloneUrl } from "@/features/projects/lib/projectCloneUrl";
import { projectDtagFromName } from "@/features/projects/projectCreation";
import {
  pickProjectImportFolder,
  type ImportRepoFolderInfo,
  type RepoRemoteStrategy,
} from "@/shared/api/projectGit";
import type { Channel } from "@/shared/api/types";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { ChooserDialogContent } from "@/shared/ui/chooser-dialog-content";
import { Dialog } from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";

const FIELD_SHELL_CLASS =
  "flex min-h-11 items-center rounded-xl border border-input bg-muted/40 px-3 transition-colors hover:border-muted-foreground/40 focus-within:border-muted-foreground/50";
const FIELD_CONTROL_CLASS =
  "h-8 border-0 bg-transparent px-0 py-0 text-muted-foreground/55 shadow-none outline-none ring-0 placeholder:text-muted-foreground/55 focus:bg-transparent focus:text-foreground focus-visible:ring-0";

function normalizedUrl(value: string): string {
  return value
    .trim()
    .replace(/\/+$/, "")
    .replace(/\.git$/i, "");
}

/** Validation problem with the picked folder, if any. */
function folderProblem(folder: ImportRepoFolderInfo | null): string | null {
  if (!folder) return null;
  if (!folder.isGitRepo) return "Choose a git repository.";
  if (!folder.hasCommits) {
    return "The repository has no commits yet — commit before importing.";
  }
  if (!folder.currentBranch) {
    return "Check out a branch before importing (detached HEAD).";
  }
  return null;
}

/**
 * Modal for importing an existing local checkout into a project: announce,
 * point the folder's remote at the relay, push, and register the checkout.
 */
export function ImportProjectRepoDialog({
  channels,
  defaultChannelId,
  isImporting,
  onImport,
  onOpenChange,
  open,
  ownerPubkey,
  projectName,
  relayOrigin,
}: {
  channels: Channel[];
  defaultChannelId?: string;
  isImporting: boolean;
  onImport: (input: {
    name: string;
    accessChannelId: string;
    path: string;
    remoteStrategy: RepoRemoteStrategy;
  }) => Promise<void>;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  ownerPubkey: string | undefined;
  projectName: string;
  relayOrigin: string | null;
}) {
  const [folder, setFolder] = React.useState<ImportRepoFolderInfo | null>(null);
  const [name, setName] = React.useState("");
  const [selectedChannelId, setSelectedChannelId] = React.useState("");
  const [remoteStrategy, setRemoteStrategy] =
    React.useState<RepoRemoteStrategy>("set-origin");
  const [errorMessage, setErrorMessage] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (!open) return;
    setFolder(null);
    setName("");
    setSelectedChannelId(defaultChannelId ?? "");
    setRemoteStrategy("set-origin");
    setErrorMessage(null);
  }, [defaultChannelId, open]);

  const dtag = projectDtagFromName(name.trim());
  const problem = folderProblem(folder);
  // The remote prompt only matters when the folder already tracks somewhere
  // else — no origin (or one already at the relay URL) silently uses origin.
  const derivedCloneUrl =
    ownerPubkey && dtag
      ? deriveRelayCloneUrl(relayOrigin, ownerPubkey, dtag)
      : null;
  const showRemoteChoice = Boolean(
    folder?.originUrl &&
      (!derivedCloneUrl ||
        normalizedUrl(folder.originUrl) !== normalizedUrl(derivedCloneUrl)),
  );

  async function handleBrowse() {
    setErrorMessage(null);
    try {
      const picked = await pickProjectImportFolder();
      if (!picked) return;
      setFolder(picked);
      setName((current) => (current.trim() ? current : picked.name));
    } catch (error) {
      setErrorMessage(
        error instanceof Error
          ? error.message
          : "Failed to open the folder picker.",
      );
    }
  }

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!folder || problem || !name.trim() || !selectedChannelId) return;
    setErrorMessage(null);
    try {
      await onImport({
        accessChannelId: selectedChannelId,
        name: name.trim(),
        path: folder.path,
        remoteStrategy: showRemoteChoice ? remoteStrategy : "set-origin",
      });
      onOpenChange(false);
    } catch (error) {
      // Keep the dialog open — the flow is idempotent, so resubmitting after
      // a failed push resumes where it stopped.
      setErrorMessage(
        error instanceof Error ? error.message : "Failed to import repository.",
      );
    }
  }

  return (
    <Dialog
      onOpenChange={(nextOpen) => {
        if (!nextOpen && isImporting) return;
        onOpenChange(nextOpen);
      }}
      open={open}
    >
      <ChooserDialogContent
        className="max-w-lg"
        contentClassName="pt-3"
        data-testid="import-project-repo-dialog"
        description={`Publish an existing local checkout as a repository in ${projectName}.`}
        footer={
          <Button
            data-testid="import-project-repo-submit"
            disabled={
              isImporting ||
              !folder ||
              Boolean(problem) ||
              !name.trim() ||
              !selectedChannelId
            }
            form="import-project-repo-form"
            type="submit"
          >
            {isImporting ? "Importing..." : "Import repository"}
          </Button>
        }
        footerClassName="border-t-0 pt-0"
        headerClassName="pb-2"
        title="Import local repository"
      >
        <form
          className="space-y-5"
          id="import-project-repo-form"
          onSubmit={(event) => void handleSubmit(event)}
        >
          <div className="space-y-1.5">
            <span className="text-sm font-medium text-foreground">Folder</span>
            <div className={cn(FIELD_SHELL_CLASS, "gap-2")}>
              <span
                className={cn(
                  "min-w-0 flex-1 truncate text-sm",
                  folder ? "text-foreground" : "text-muted-foreground/55",
                )}
                data-testid="import-project-repo-path"
              >
                {folder?.path ?? "Choose a local git repository"}
              </span>
              <Button
                data-testid="import-project-repo-browse"
                disabled={isImporting}
                onClick={() => void handleBrowse()}
                size="sm"
                type="button"
                variant="outline"
              >
                Browse…
              </Button>
            </div>
            {problem ? (
              <p className="text-sm text-destructive">{problem}</p>
            ) : null}
          </div>
          <div className="space-y-1.5">
            <label
              className="text-sm font-medium text-foreground"
              htmlFor="import-project-repo-name"
            >
              Name
            </label>
            <div className={FIELD_SHELL_CLASS}>
              <Input
                autoCapitalize="none"
                autoComplete="off"
                autoCorrect="off"
                className={cn(FIELD_CONTROL_CLASS)}
                data-testid="import-project-repo-name"
                disabled={isImporting}
                id="import-project-repo-name"
                onChange={(event) => {
                  setName(event.target.value);
                  setErrorMessage(null);
                }}
                placeholder="mobile-app"
                spellCheck={false}
                value={name}
              />
            </div>
            {dtag && dtag !== name.trim() ? (
              <p className="text-xs text-muted-foreground">
                Published as "{dtag}"
              </p>
            ) : null}
          </div>
          <div className="space-y-1.5">
            <label
              className="text-sm font-medium text-foreground"
              htmlFor="import-project-repo-channel"
            >
              Access channel
            </label>
            <div className={FIELD_SHELL_CLASS}>
              <select
                className={cn(FIELD_CONTROL_CLASS, "w-full")}
                data-testid="import-project-repo-channel"
                disabled={isImporting}
                id="import-project-repo-channel"
                onChange={(event) => {
                  setSelectedChannelId(event.target.value);
                  setErrorMessage(null);
                }}
                required
                value={selectedChannelId}
              >
                <option value="">Select a channel</option>
                {channels.map((channel) => (
                  <option key={channel.id} value={channel.id}>
                    {channel.name}
                  </option>
                ))}
              </select>
            </div>
            <p className="text-xs text-muted-foreground">
              Members of this channel can access the repository.
            </p>
          </div>
          {showRemoteChoice ? (
            <div className="space-y-1.5">
              <span className="text-sm font-medium text-foreground">
                This folder already has an origin remote
              </span>
              <div className="flex flex-col gap-2">
                <label className="flex cursor-pointer items-start gap-2">
                  <input
                    checked={remoteStrategy === "set-origin"}
                    className="mt-1"
                    data-testid="import-project-repo-remote-origin"
                    disabled={isImporting}
                    name="import-remote-strategy"
                    onChange={() => setRemoteStrategy("set-origin")}
                    type="radio"
                  />
                  <span className="text-sm">
                    Point origin at this project (current origin becomes{" "}
                    <code>upstream</code>)
                  </span>
                </label>
                <label className="flex cursor-pointer items-start gap-2">
                  <input
                    checked={remoteStrategy === "add-buzz-remote"}
                    className="mt-1"
                    data-testid="import-project-repo-remote-buzz"
                    disabled={isImporting}
                    name="import-remote-strategy"
                    onChange={() => setRemoteStrategy("add-buzz-remote")}
                    type="radio"
                  />
                  <span className="text-sm">
                    Keep origin unchanged; add a separate <code>buzz</code>{" "}
                    remote
                  </span>
                </label>
              </div>
            </div>
          ) : null}
          {errorMessage ? (
            <p className="text-sm text-destructive">{errorMessage}</p>
          ) : null}
        </form>
      </ChooserDialogContent>
    </Dialog>
  );
}
