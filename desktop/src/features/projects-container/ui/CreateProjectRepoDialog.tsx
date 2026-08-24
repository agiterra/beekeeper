import * as React from "react";

import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { ChooserDialogContent } from "@/shared/ui/chooser-dialog-content";
import { Dialog } from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";

import { ProjectRepoAccessNote } from "./ProjectRepoAccessNote";

const FIELD_SHELL_CLASS =
  "flex min-h-11 items-center rounded-xl border border-input bg-muted/40 px-3 transition-colors hover:border-muted-foreground/40 focus-within:border-muted-foreground/50";
const FIELD_CONTROL_CLASS =
  "h-8 border-0 bg-transparent px-0 py-0 text-muted-foreground/55 shadow-none outline-none ring-0 placeholder:text-muted-foreground/55 focus:bg-transparent focus:text-foreground focus-visible:ring-0";

/**
 * Modal for creating a repository inside an existing project container —
 * the repo-only sibling of the legacy create-project dialog.
 */
export function CreateProjectRepoDialog({
  isCreating,
  onCreate,
  onOpenChange,
  open,
  otherMemberCount,
  projectName,
}: {
  isCreating: boolean;
  onCreate: (input: { name: string; cloneUrl?: string }) => Promise<void>;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  /**
   * Project roster size excluding the viewer, or `null` while it is still
   * loading. Exactly `0` means nobody else can reach the new repository yet,
   * which the dialog says out loud rather than letting the user discover it
   * when a teammate's clone 404s.
   */
  otherMemberCount: number | null;
  projectName: string;
}) {
  const [name, setName] = React.useState("");
  const [cloneUrl, setCloneUrl] = React.useState("");
  const [errorMessage, setErrorMessage] = React.useState<string | null>(null);
  const nameInputRef = React.useRef<HTMLInputElement>(null);

  React.useEffect(() => {
    if (!open) return;
    setName("");
    setCloneUrl("");
    setErrorMessage(null);
    const timerId = globalThis.setTimeout(
      () => nameInputRef.current?.focus(),
      50,
    );
    return () => globalThis.clearTimeout(timerId);
  }, [open]);

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!name.trim()) return;
    setErrorMessage(null);
    try {
      await onCreate({
        cloneUrl: cloneUrl.trim() || undefined,
        name: name.trim(),
      });
      onOpenChange(false);
    } catch (error) {
      setErrorMessage(
        error instanceof Error ? error.message : "Failed to create repository.",
      );
    }
  }

  return (
    <Dialog
      onOpenChange={(nextOpen) => {
        if (!nextOpen && isCreating) return;
        onOpenChange(nextOpen);
      }}
      open={open}
    >
      <ChooserDialogContent
        className="max-w-lg"
        contentClassName="pt-3"
        data-testid="create-project-repo-dialog"
        description={`Add a repository to ${projectName}.`}
        footer={
          <Button
            data-testid="create-project-repo-submit"
            disabled={isCreating || !name.trim()}
            form="create-project-repo-form"
            type="submit"
          >
            {isCreating ? "Adding..." : "Add repository"}
          </Button>
        }
        footerClassName="border-t-0 pt-0"
        headerClassName="pb-2"
        title="Add repository"
      >
        <form
          className="space-y-5"
          id="create-project-repo-form"
          onSubmit={(event) => void handleSubmit(event)}
        >
          <div className="space-y-1.5">
            <label
              className="text-sm font-medium text-foreground"
              htmlFor="create-project-repo-name"
            >
              Name
            </label>
            <div className={FIELD_SHELL_CLASS}>
              <Input
                autoCapitalize="none"
                autoComplete="off"
                autoCorrect="off"
                className={cn(FIELD_CONTROL_CLASS)}
                data-testid="create-project-repo-name"
                disabled={isCreating}
                id="create-project-repo-name"
                onChange={(event) => {
                  setName(event.target.value);
                  setErrorMessage(null);
                }}
                placeholder="mobile-app"
                ref={nameInputRef}
                spellCheck={false}
                value={name}
              />
            </div>
          </div>
          <ProjectRepoAccessNote
            otherMemberCount={otherMemberCount}
            projectName={projectName}
          />
          <div className="space-y-1.5">
            <label
              className="text-sm font-medium text-foreground"
              htmlFor="create-project-repo-clone-url"
            >
              Clone URL
              <span className="ml-1 text-xs font-normal text-muted-foreground/50">
                Optional
              </span>
            </label>
            <div className={FIELD_SHELL_CLASS}>
              <Input
                autoCapitalize="none"
                autoComplete="off"
                autoCorrect="off"
                className={cn(FIELD_CONTROL_CLASS)}
                data-testid="create-project-repo-clone-url"
                disabled={isCreating}
                id="create-project-repo-clone-url"
                onChange={(event) => {
                  setCloneUrl(event.target.value);
                  setErrorMessage(null);
                }}
                placeholder="https://relay.example.com/git/mobile-app.git"
                spellCheck={false}
                value={cloneUrl}
              />
            </div>
          </div>
          {errorMessage ? (
            <p className="text-sm text-destructive">{errorMessage}</p>
          ) : null}
        </form>
      </ChooserDialogContent>
    </Dialog>
  );
}
