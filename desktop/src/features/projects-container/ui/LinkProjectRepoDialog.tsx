import * as React from "react";

import {
  pickProjectImportFolder,
  type ImportRepoFolderInfo,
  type RepoRemoteStrategy,
} from "@/shared/api/projectGit";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { ChooserDialogContent } from "@/shared/ui/chooser-dialog-content";
import { Dialog } from "@/shared/ui/dialog";

const FIELD_SHELL_CLASS =
  "flex min-h-11 items-center rounded-xl border border-input bg-muted/40 px-3 transition-colors hover:border-muted-foreground/40 focus-within:border-muted-foreground/50";

function normalizedUrl(value: string): string {
  return value
    .trim()
    .replace(/\/+$/, "")
    .replace(/\.git$/i, "");
}

/**
 * Modal for linking an existing local checkout to an already-announced
 * repository (no publish, no push): remote setup + registry entry so the git
 * tooling resolves the folder wherever it lives.
 */
export function LinkProjectRepoDialog({
  cloneUrl,
  isLinking,
  onLink,
  onOpenChange,
  open,
  repoName,
}: {
  /** Relay-hosted clone URL the remote will point at (foreign-origin check). */
  cloneUrl: string | null;
  isLinking: boolean;
  onLink: (input: {
    path: string;
    remoteStrategy: RepoRemoteStrategy;
  }) => Promise<void>;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  repoName: string;
}) {
  const [folder, setFolder] = React.useState<ImportRepoFolderInfo | null>(null);
  const [remoteStrategy, setRemoteStrategy] =
    React.useState<RepoRemoteStrategy>("set-origin");
  const [errorMessage, setErrorMessage] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (!open) return;
    setFolder(null);
    setRemoteStrategy("set-origin");
    setErrorMessage(null);
  }, [open]);

  const problem =
    folder && !folder.isGitRepo ? "Choose a git repository." : null;
  const showRemoteChoice = Boolean(
    folder?.originUrl &&
      cloneUrl &&
      normalizedUrl(folder.originUrl) !== normalizedUrl(cloneUrl),
  );

  async function handleBrowse() {
    setErrorMessage(null);
    try {
      const picked = await pickProjectImportFolder();
      if (!picked) return;
      setFolder(picked);
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
    if (!folder || problem) return;
    setErrorMessage(null);
    try {
      await onLink({
        path: folder.path,
        remoteStrategy: showRemoteChoice ? remoteStrategy : "set-origin",
      });
      onOpenChange(false);
    } catch (error) {
      setErrorMessage(
        error instanceof Error ? error.message : "Failed to link the checkout.",
      );
    }
  }

  return (
    <Dialog
      onOpenChange={(nextOpen) => {
        if (!nextOpen && isLinking) return;
        onOpenChange(nextOpen);
      }}
      open={open}
    >
      <ChooserDialogContent
        className="max-w-lg"
        contentClassName="pt-3"
        data-testid="link-project-repo-dialog"
        description={`Use an existing local checkout of ${repoName} for pushes, pulls, and sync status.`}
        footer={
          <Button
            data-testid="link-project-repo-submit"
            disabled={isLinking || !folder || Boolean(problem)}
            form="link-project-repo-form"
            type="submit"
          >
            {isLinking ? "Linking..." : "Link checkout"}
          </Button>
        }
        footerClassName="border-t-0 pt-0"
        headerClassName="pb-2"
        title="Link local checkout"
      >
        <form
          className="space-y-5"
          id="link-project-repo-form"
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
                data-testid="link-project-repo-path"
              >
                {folder?.path ?? "Choose a local git repository"}
              </span>
              <Button
                data-testid="link-project-repo-browse"
                disabled={isLinking}
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
                    data-testid="link-project-repo-remote-origin"
                    disabled={isLinking}
                    name="link-remote-strategy"
                    onChange={() => setRemoteStrategy("set-origin")}
                    type="radio"
                  />
                  <span className="text-sm">
                    Point origin at this repository (current origin becomes{" "}
                    <code>upstream</code>)
                  </span>
                </label>
                <label className="flex cursor-pointer items-start gap-2">
                  <input
                    checked={remoteStrategy === "add-buzz-remote"}
                    className="mt-1"
                    data-testid="link-project-repo-remote-buzz"
                    disabled={isLinking}
                    name="link-remote-strategy"
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
