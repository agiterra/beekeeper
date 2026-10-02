import * as React from "react";

import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";

import { agentsRepoCopy as copy } from "../lib/agentsRepoCopy";
import { newDocumentPath, newFolderKeepPath } from "../lib/agentsRepoPaths";

/**
 * Asks for a document's folder, name and format and hands back its path.
 *
 * A dialog rather than `window.prompt` for the reason the plan dialog gives:
 * the Tauri webview does not implement `prompt`. The format is a choice
 * because it is a real one — Markdown renders in the tab, HTML previews in its
 * own window — and the name is not slugified, because a document's stem is a
 * document name.
 */
export function AgentsRepoNewDocumentDialog({
  open,
  onOpenChange,
  onCreate,
  /** The folder the tree had selected, offered as the default. */
  initialFolder = "",
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreate: (path: string) => void;
  initialFolder?: string;
}) {
  const [folder, setFolder] = React.useState(initialFolder);
  const [name, setName] = React.useState("");
  const [format, setFormat] = React.useState<"md" | "html">("md");
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (open) {
      setFolder(initialFolder);
      setName("");
      setFormat("md");
      setError(null);
    }
  }, [initialFolder, open]);

  const made = React.useMemo(
    () =>
      name.trim().length === 0 ? null : newDocumentPath(folder, name, format),
    [folder, format, name],
  );

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const result = newDocumentPath(folder, name, format);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    onCreate(result.path);
    onOpenChange(false);
  };

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent data-testid="agents-repo-new-document-dialog">
        <form onSubmit={submit}>
          <DialogHeader>
            <DialogTitle>{copy.newDocument}</DialogTitle>
            <DialogDescription>{copy.newDocumentHelp}</DialogDescription>
          </DialogHeader>
          <div className="space-y-3 py-4">
            <div className="space-y-1">
              <label
                className="text-sm font-medium"
                htmlFor="agents-repo-new-document-folder"
              >
                {copy.newDocumentFolderLabel}
              </label>
              <Input
                data-testid="agents-repo-new-document-folder"
                id="agents-repo-new-document-folder"
                onChange={(event) => {
                  setFolder(event.target.value);
                  setError(null);
                }}
                placeholder="mockups"
                value={folder}
              />
              <p className="text-xs text-muted-foreground">
                {copy.newDocumentFolderHelp}
              </p>
            </div>
            <div className="space-y-1">
              <label
                className="text-sm font-medium"
                htmlFor="agents-repo-new-document-name"
              >
                {copy.newDocumentNameLabel}
              </label>
              <Input
                autoFocus
                data-testid="agents-repo-new-document-name"
                id="agents-repo-new-document-name"
                onChange={(event) => {
                  setName(event.target.value);
                  setError(null);
                }}
                placeholder="login"
                value={name}
              />
            </div>
            <fieldset className="space-y-1">
              <legend className="text-sm font-medium">
                {copy.newDocumentFormatLabel}
              </legend>
              <div className="flex gap-2">
                {(
                  [
                    ["md", "Markdown"],
                    ["html", "HTML"],
                  ] as const
                ).map(([value, label]) => (
                  <Button
                    data-testid={`agents-repo-new-document-format-${value}`}
                    key={value}
                    onClick={() => {
                      setFormat(value);
                      setError(null);
                    }}
                    size="sm"
                    type="button"
                    variant={format === value ? "default" : "outline"}
                  >
                    {label}
                  </Button>
                ))}
              </div>
            </fieldset>
            <p
              className="font-mono text-xs text-muted-foreground"
              data-testid="agents-repo-new-document-preview"
            >
              {error ??
                (made === null
                  ? copy.newDocumentHelp
                  : made.ok
                    ? made.path
                    : made.error)}
            </p>
          </div>
          <DialogFooter>
            <Button
              onClick={() => onOpenChange(false)}
              type="button"
              variant="ghost"
            >
              {copy.cancel}
            </Button>
            <Button
              data-testid="agents-repo-new-document-create"
              disabled={name.trim().length === 0}
              type="submit"
            >
              {copy.create}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/**
 * Asks for a folder and hands back its `.gitkeep`.
 *
 * The keep is not an implementation detail hidden from the person: the help
 * text says the folder lands as its keep on commit, because that is what they
 * will see in the diff and in git.
 */
export function AgentsRepoNewFolderDialog({
  open,
  onOpenChange,
  onCreate,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Called with the keep's path and the folder it makes. */
  onCreate: (keepPath: string, folder: string) => void;
}) {
  const [folder, setFolder] = React.useState("");
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (open) {
      setFolder("");
      setError(null);
    }
  }, [open]);

  const made = React.useMemo(
    () => (folder.trim().length === 0 ? null : newFolderKeepPath(folder)),
    [folder],
  );

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const result = newFolderKeepPath(folder);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    onCreate(result.path, result.folder);
    onOpenChange(false);
  };

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent data-testid="agents-repo-new-folder-dialog">
        <form onSubmit={submit}>
          <DialogHeader>
            <DialogTitle>{copy.newFolder}</DialogTitle>
            <DialogDescription>{copy.newFolderHelp}</DialogDescription>
          </DialogHeader>
          <div className="space-y-2 py-4">
            <Input
              autoFocus
              data-testid="agents-repo-new-folder-name"
              onChange={(event) => {
                setFolder(event.target.value);
                setError(null);
              }}
              placeholder="mockups"
              value={folder}
            />
            <p
              className="font-mono text-xs text-muted-foreground"
              data-testid="agents-repo-new-folder-preview"
            >
              {error ??
                (made === null
                  ? copy.newFolderHelp
                  : made.ok
                    ? made.path
                    : made.error)}
            </p>
          </div>
          <DialogFooter>
            <Button
              onClick={() => onOpenChange(false)}
              type="button"
              variant="ghost"
            >
              {copy.cancel}
            </Button>
            <Button
              data-testid="agents-repo-new-folder-create"
              disabled={folder.trim().length === 0}
              type="submit"
            >
              {copy.create}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
