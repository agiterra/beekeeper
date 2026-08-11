import * as React from "react";

import { PersonaShareRecipients } from "@/features/agents/ui/PersonaShareRecipients";
import { useUsersBatchQuery } from "@/features/profile/hooks";
import type { UserSearchResult } from "@/shared/api/types";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/shared/ui/alert-dialog";
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
import { GENERAL_PROJECT_DTAG } from "../lib/projectContainerModel";
import { ProjectVisibilitySettings } from "./ProjectVisibilitySettings";

/** Modal for renaming a project container / editing its description,
 * visibility, and invited members. */
export function EditProjectContainerDialog({
  project,
  isSaving,
  onSave,
  onOpenChange,
}: {
  project: ProjectContainer | null;
  isSaving: boolean;
  onSave: (input: {
    name: string;
    description?: string;
    visibility?: ProjectContainer["visibility"];
    memberPubkeys?: string[];
  }) => Promise<void>;
  onOpenChange: (open: boolean) => void;
}) {
  const [name, setName] = React.useState("");
  const [description, setDescription] = React.useState("");
  const [visibility, setVisibility] =
    React.useState<ProjectContainer["visibility"]>("public");
  const [members, setMembers] = React.useState<UserSearchResult[]>([]);
  const [errorMessage, setErrorMessage] = React.useState<string | null>(null);
  const [pendingVisibility, setPendingVisibility] = React.useState<
    ProjectContainer["visibility"] | null
  >(null);

  const isGeneral = project?.dtag === GENERAL_PROJECT_DTAG;

  React.useEffect(() => {
    if (!project) return;
    setName(project.name);
    setDescription(project.description);
    setVisibility(project.visibility);
    setMembers(
      project.members.map((pubkey) => ({
        pubkey,
        displayName: null,
        avatarUrl: null,
        nip05Handle: null,
        ownerPubkey: null,
        isAgent: false,
      })),
    );
    setErrorMessage(null);
    setPendingVisibility(null);
  }, [project]);

  // Enrich the pre-filled member chips with resolved profile info once it
  // arrives, without touching which pubkeys are selected.
  const memberPubkeys = React.useMemo(
    () => members.map((member) => member.pubkey),
    [members],
  );
  const memberProfilesQuery = useUsersBatchQuery(memberPubkeys);
  React.useEffect(() => {
    const profiles = memberProfilesQuery.data?.profiles;
    if (!profiles) return;
    setMembers((current) =>
      current.map((member) => {
        const summary = profiles[member.pubkey.toLowerCase()];
        if (!summary) return member;
        return {
          ...member,
          displayName: summary.displayName ?? member.displayName,
          avatarUrl: summary.avatarUrl ?? member.avatarUrl,
          nip05Handle: summary.nip05Handle ?? member.nip05Handle,
          ownerPubkey: summary.ownerPubkey ?? member.ownerPubkey,
          isAgent: summary.isAgent ?? member.isAgent,
        };
      }),
    );
  }, [memberProfilesQuery.data]);

  async function doSave() {
    setErrorMessage(null);
    try {
      await onSave({
        name: name.trim(),
        description: description.trim() || undefined,
        visibility,
        memberPubkeys:
          visibility === "private"
            ? members.map((member) => member.pubkey)
            : [],
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
    if (project && visibility !== project.visibility) {
      setPendingVisibility(visibility);
      return;
    }
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
            {isGeneral ? (
              <div
                className="flex min-h-12 items-center justify-between gap-4 rounded-xl border border-input bg-muted/30 px-3 py-3 text-sm text-muted-foreground"
                data-testid="edit-project-container-visibility-locked"
              >
                <span>Visibility</span>
                <span>Public</span>
              </div>
            ) : (
              <ProjectVisibilitySettings
                onVisibilityChange={setVisibility}
                testIdPrefix="edit-project-container"
                visibility={visibility}
              />
            )}
            {isGeneral ? (
              <p className="text-xs text-muted-foreground">
                The General project is always public.
              </p>
            ) : null}
            {!isGeneral && visibility === "private" ? (
              <PersonaShareRecipients
                allowDirectPubkeyEntry
                disabled={false}
                onSelectionChange={setMembers}
                open={project !== null}
                selectedUsers={members}
                testIdPrefix="edit-project-container-members"
              />
            ) : null}
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

      <AlertDialog
        onOpenChange={(open) => {
          if (!open) setPendingVisibility(null);
        }}
        open={pendingVisibility !== null}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {pendingVisibility === "private"
                ? "Make this project private?"
                : "Make this project public?"}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {pendingVisibility === "private"
                ? "Only you and the people you invite will be able to see this project — its channels, forums, and code repositories."
                : "Everyone in the community will be able to see this project. Its invited member list will be cleared."}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel
              disabled={isSaving}
              onClick={() => setPendingVisibility(null)}
            >
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              data-testid="edit-project-container-visibility-confirm"
              disabled={isSaving}
              onClick={(event) => {
                event.preventDefault();
                setPendingVisibility(null);
                void doSave();
              }}
            >
              {pendingVisibility === "private" ? "Make private" : "Make public"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Dialog>
  );
}
