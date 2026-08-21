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

import type { CreateProjectContainerInput } from "../useCreateProjectContainer";

/** Modal for creating a project container (kind:30621). */
export function CreateProjectContainerDialog({
  isCreating,
  onCreate,
  onOpenChange,
  open,
}: {
  isCreating: boolean;
  onCreate: (input: CreateProjectContainerInput) => Promise<void>;
  onOpenChange: (open: boolean) => void;
  open: boolean;
}) {
  const [name, setName] = React.useState("");
  const [description, setDescription] = React.useState("");
  const [errorMessage, setErrorMessage] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (!open) return;
    setName("");
    setDescription("");
    setErrorMessage(null);
  }, [open]);

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmedName = name.trim();
    if (!trimmedName) return;
    setErrorMessage(null);
    try {
      await onCreate({
        name: trimmedName,
        description: description.trim() || undefined,
      });
      onOpenChange(false);
    } catch (error) {
      setErrorMessage(
        error instanceof Error ? error.message : "Failed to create project.",
      );
    }
  }

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>New project</DialogTitle>
          <DialogDescription>
            A project groups agents, channels, code, shells, and forums in the
            sidebar.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit}>
          <div className="flex flex-col gap-3">
            <Input
              autoFocus
              aria-label="Project name"
              data-testid="create-project-container-name"
              onChange={(event) => setName(event.target.value)}
              placeholder="Project name"
              value={name}
            />
            <Textarea
              aria-label="Project description"
              data-testid="create-project-container-description"
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
              data-testid="create-project-container-submit"
              disabled={isCreating || name.trim() === ""}
              type="submit"
            >
              Create project
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
