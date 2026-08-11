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
import { Textarea } from "@/shared/ui/textarea";

import type { ProjectContainer } from "../hooks";

/** Modal for renaming a project container / editing its description. */
export function EditProjectContainerDialog({
  project,
  isSaving,
  onSave,
  onOpenChange,
}: {
  project: ProjectContainer | null;
  isSaving: boolean;
  onSave: (input: { name: string; description?: string }) => Promise<void>;
  onOpenChange: (open: boolean) => void;
}) {
  const [name, setName] = React.useState("");
  const [description, setDescription] = React.useState("");
  const [errorMessage, setErrorMessage] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (!project) return;
    setName(project.name);
    setDescription(project.description);
    setErrorMessage(null);
  }, [project]);

  async function doSave() {
    setErrorMessage(null);
    try {
      await onSave({
        name: name.trim(),
        description: description.trim() || undefined,
      });
      onOpenChange(false);
    } catch (error) {
      setErrorMessage(
        error instanceof Error ? error.message : "Failed to save the project.",
      );
    }
  }

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmedName = name.trim();
    if (!trimmedName) return;
    await doSave();
  }

  return (
    <Dialog onOpenChange={onOpenChange} open={project !== null}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>Edit project</DialogTitle>
          <DialogDescription>
            Rename the project or update its description. The project id (
            {project?.dtag}) stays the same.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit}>
          <div className="flex flex-col gap-3">
            <Input
              autoFocus
              aria-label="Project name"
              data-testid="edit-project-container-name"
              onChange={(event) => setName(event.target.value)}
              placeholder="Project name"
              value={name}
            />
            <Textarea
              aria-label="Project description"
              data-testid="edit-project-container-description"
              onChange={(event) => setDescription(event.target.value)}
              placeholder="What is this project about? (optional)"
              rows={3}
              value={description}
            />
            {errorMessage ? (
              <p className="text-sm text-destructive">{errorMessage}</p>
            ) : null}
          </div>
          <DialogFooter className="mt-4">
            <Button
              onClick={() => onOpenChange(false)}
              type="button"
              variant="outline"
            >
              Cancel
            </Button>
            <Button
              data-testid="edit-project-container-save"
              disabled={isSaving || name.trim() === ""}
              type="submit"
            >
              Save
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
