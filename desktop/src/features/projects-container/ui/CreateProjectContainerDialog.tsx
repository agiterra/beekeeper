import * as React from "react";

import type { UserSearchResult } from "@/shared/api/types";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";

import { PersonaShareRecipients } from "@/features/agents/ui/PersonaShareRecipients";
import {
  DEFAULT_VERIFY_COMMAND_TEXT,
  parseVerifyCommand,
} from "../lib/projectVerifySetup";
import {
  type CreateProjectContainerInput,
  slugFromName,
} from "../useCreateProjectContainer";
import {
  ProjectCheckoutFolderField,
  type ProjectCheckoutFolderChoice,
} from "./ProjectCheckoutFolderField";
import { ProjectColorPickerField } from "./ProjectColorPickerField";
import { ProjectIconPickerField } from "./ProjectIconPickerField";
import { ProjectVisibilitySettings } from "./ProjectVisibilitySettings";

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
  const [visibility, setVisibility] = React.useState<"public" | "private">(
    "private",
  );
  const [members, setMembers] = React.useState<UserSearchResult[]>([]);
  const [icon, setIcon] = React.useState("");
  const [color, setColor] = React.useState<string | null>(null);
  const [errorMessage, setErrorMessage] = React.useState<string | null>(null);
  // The seeded `verify` action's command (ledger 248): project-specific, so
  // asked for here, and the one command setup asks consent to run.
  const [verifyCommand, setVerifyCommand] = React.useState(
    DEFAULT_VERIFY_COMMAND_TEXT,
  );
  // The parent folder the code repository is cloned under; `null` hands the
  // host its default root. The row is keyed on `open` so a folder chosen
  // for one project never leaks into the next.
  const [checkoutParent, setCheckoutParent] = React.useState<string | null>(
    null,
  );
  const handleCheckoutChange = React.useCallback(
    (choice: ProjectCheckoutFolderChoice) => setCheckoutParent(choice.parent),
    [],
  );

  React.useEffect(() => {
    if (!open) return;
    setName("");
    setDescription("");
    setVisibility("private");
    setMembers([]);
    setIcon("");
    setColor(null);
    setErrorMessage(null);
    setCheckoutParent(null);
    setVerifyCommand(DEFAULT_VERIFY_COMMAND_TEXT);
  }, [open]);

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmedName = name.trim();
    if (!trimmedName) return;
    setErrorMessage(null);
    try {
      // Initial invitees join as Collaborator; roles are managed afterward
      // on the project page's Members card.
      await onCreate({
        name: trimmedName,
        description: description.trim() || undefined,
        visibility,
        members: members.map((member) => ({
          pubkey: member.pubkey,
          role: "collaborator" as const,
        })),
        icon: icon.trim() || null,
        color,
        checkoutParent,
        verifyCommand: parseVerifyCommand(verifyCommand),
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
      <DialogContent aria-describedby={undefined} className="max-w-sm">
        <DialogHeader>
          <DialogTitle>New project</DialogTitle>
        </DialogHeader>
        <form onSubmit={handleSubmit}>
          <div className="flex flex-col gap-3">
            <div className="flex items-center gap-2">
              <ProjectIconPickerField
                icon={icon}
                onIconChange={setIcon}
                testIdPrefix="create-project-container"
              />
              <Input
                autoFocus
                aria-label="Project name"
                className="flex-1"
                data-testid="create-project-container-name"
                onChange={(event) => setName(event.target.value)}
                placeholder="Project name"
                value={name}
              />
            </div>
            <Textarea
              aria-label="Project description"
              data-testid="create-project-container-description"
              onChange={(event) => setDescription(event.target.value)}
              placeholder="What is this project about? (optional)"
              rows={3}
              value={description}
            />
            <ProjectCheckoutFolderField
              disabled={isCreating}
              key={open ? "open" : "closed"}
              onChange={handleCheckoutChange}
              slug={slugFromName(name)}
            />
            <div className="flex flex-col gap-1 text-xs text-muted-foreground">
              <span id="create-project-verify-command-hint">
                Verify command — what agents run to test a commit, on your
                computer once you allow it
              </span>
              <Input
                aria-describedby="create-project-verify-command-hint"
                aria-label="Verify command"
                className="font-mono"
                data-testid="create-project-container-verify-command"
                disabled={isCreating}
                onChange={(event) => setVerifyCommand(event.target.value)}
                placeholder={DEFAULT_VERIFY_COMMAND_TEXT}
                value={verifyCommand}
              />
            </div>
            <ProjectColorPickerField
              color={color}
              onColorChange={setColor}
              testIdPrefix="create-project-container"
            />
            <ProjectVisibilitySettings
              onVisibilityChange={setVisibility}
              testIdPrefix="create-project-container"
              visibility={visibility}
            />
            <PersonaShareRecipients
              allowDirectPubkeyEntry
              disabled={false}
              onSelectionChange={setMembers}
              open={open}
              selectedUsers={members}
              testIdPrefix="create-project-container-members"
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
