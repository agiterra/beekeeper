import { FolderGit2 } from "lucide-react";
import * as React from "react";

import type { Repository as CodeRepo } from "@/features/projects/hooks";
import { Button } from "@/shared/ui/button";
import { ChooserDialogContent } from "@/shared/ui/chooser-dialog-content";
import { Dialog } from "@/shared/ui/dialog";

/**
 * Picker for moving an existing repository into a project container — the
 * container-native sibling of the legacy attach-repository dialog.
 */
export function AttachProjectRepoDialog({
  isAttaching,
  onAttach,
  onOpenChange,
  open,
  projectName,
  repos,
}: {
  isAttaching: boolean;
  onAttach: (repo: CodeRepo) => Promise<void>;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  projectName: string;
  repos: CodeRepo[];
}) {
  const [errorMessage, setErrorMessage] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (open) setErrorMessage(null);
  }, [open]);

  async function handleAttach(repo: CodeRepo) {
    setErrorMessage(null);
    try {
      await onAttach(repo);
      onOpenChange(false);
    } catch (error) {
      setErrorMessage(
        error instanceof Error ? error.message : "Failed to add repository.",
      );
    }
  }

  return (
    <Dialog
      onOpenChange={(nextOpen) => {
        if (!nextOpen && isAttaching) return;
        onOpenChange(nextOpen);
      }}
      open={open}
    >
      <ChooserDialogContent
        className="max-w-lg"
        contentClassName="max-h-96 overflow-y-auto pt-3"
        data-testid="attach-project-repo-dialog"
        description={`Choose an existing repository to add to ${projectName}.`}
        title="Add existing repository"
      >
        <div className="space-y-2">
          {repos.length === 0 ? (
            <p className="py-6 text-center text-sm text-muted-foreground">
              Every available repository is already in this project.
            </p>
          ) : (
            repos.map((repo) => (
              <Button
                className="h-auto w-full justify-start gap-3 px-3 py-2.5 text-left"
                data-testid={`attach-project-repo-item-${repo.dtag}`}
                disabled={isAttaching}
                key={repo.repoAddress}
                onClick={() => void handleAttach(repo)}
                type="button"
                variant="outline"
              >
                <FolderGit2 className="h-4 w-4 shrink-0 text-muted-foreground" />
                <span className="min-w-0">
                  <span className="block truncate font-medium">
                    {repo.name}
                  </span>
                  <span className="block truncate text-xs font-normal text-muted-foreground">
                    {repo.dtag}
                  </span>
                </span>
              </Button>
            ))
          )}
          {errorMessage ? (
            <p className="text-sm text-destructive">{errorMessage}</p>
          ) : null}
        </div>
      </ChooserDialogContent>
    </Dialog>
  );
}
