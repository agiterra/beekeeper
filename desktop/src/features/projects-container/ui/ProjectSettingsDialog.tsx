import * as React from "react";

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
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/shared/ui/tabs";
import { Textarea } from "@/shared/ui/textarea";

import type { ProjectContainer } from "../hooks";
import {
  GENERAL_PROJECT_DTAG,
  LOCAL_GENERAL_ID,
} from "../lib/projectContainerModel";
import { useProjectCapabilities } from "../lib/projectPermissions";
import { ProjectColorPickerField } from "./ProjectColorPickerField";
import { ProjectIconPickerField } from "./ProjectIconPickerField";
import { ProjectMembersManager } from "./ProjectMembersManager";
import { ProjectPacksSettingsSection } from "./ProjectPacksSettingsSection";
import { ProjectRepositoryProtectionSection } from "./ProjectRepositoryProtectionSection";
import { ProjectSettingsLocalSection } from "./ProjectSettingsLocalSection";
import { ProjectVisibilitySettings } from "./ProjectVisibilitySettings";

export type ProjectSettingsSaveInput = {
  name: string;
  description?: string;
  visibility?: ProjectContainer["visibility"];
  /** null clears the project icon. */
  icon: string | null;
  /** null clears the project color. */
  color: string | null;
};

/**
 * Project settings modal. The General tab edits the relay-synced kind:30621
 * fields (name, description, icon, color, visibility) behind the Save
 * button; only the project owner can change them — everyone else sees them
 * read-only. The Members tab manages the invite roster through the same
 * roster ops as the project page's Members card, applying immediately.
 */
export function ProjectSettingsDialog({
  project,
  isSaving,
  onSave,
  onOpenChange,
  onRequestDelete,
}: {
  project: ProjectContainer | null;
  isSaving: boolean;
  onSave: (input: ProjectSettingsSaveInput) => Promise<void>;
  onOpenChange: (open: boolean) => void;
  /**
   * Hand this project to the caller's own delete confirmation.
   *
   * Deliberately a callback rather than a `DeleteProjectDialog` mounted
   * here: both callers already render one beside this dialog with their own
   * open-state, so mounting a second would put two confirmations for the
   * same project in the tree, each running its own cascade inventory. This
   * closes settings and lets the caller open the dialog it already owns —
   * which is also the mount `onDeleted` navigation and the e2e drive.
   *
   * Omitted by a caller that has no delete affordance; the danger zone is
   * then not rendered at all.
   */
  onRequestDelete?: () => void;
}) {
  const [name, setName] = React.useState("");
  const [description, setDescription] = React.useState("");
  const [visibility, setVisibility] =
    React.useState<ProjectContainer["visibility"]>("public");
  const [icon, setIcon] = React.useState("");
  const [color, setColor] = React.useState<string | null>(null);
  const [errorMessage, setErrorMessage] = React.useState<string | null>(null);
  const [pendingVisibility, setPendingVisibility] = React.useState<
    ProjectContainer["visibility"] | null
  >(null);

  // These fields live on the project's kind:30621 head, which NIP-01
  // addresses by (kind, *creator pubkey*, d). A roster Owner republishing it
  // would mint a different project rather than edit this one, so this is the
  // one capability the creator does not share — a fact about the protocol,
  // not a permission we withhold. The dialog stays open for everyone so
  // non-editors can still read the settings and reach the Members tab.
  const capabilities = useProjectCapabilities(project);
  const canEditProject = capabilities.canEditHead;

  const isGeneral = project?.dtag === GENERAL_PROJECT_DTAG;
  // The unpublished local placeholder has no kind:30621 to tombstone, so
  // there is nothing to delete. Both `⋮` call sites already gate on this;
  // this dialog did not compute it until the danger zone needed it.
  const isFallback = project?.id === LOCAL_GENERAL_ID;
  // Deliberately NOT `canEditProject`. A roster Owner who did not create the
  // head cannot republish it but can delete it, which is exactly what the
  // read-only note above promises — gating this on edit rights would make
  // that note lie.
  const canDelete =
    onRequestDelete !== undefined &&
    capabilities.canDeleteProject &&
    !isGeneral &&
    !isFallback;

  React.useEffect(() => {
    if (!project) return;
    setName(project.name);
    setDescription(project.description);
    setVisibility(project.visibility);
    setIcon(project.icon ?? "");
    setColor(project.color);
    setErrorMessage(null);
    setPendingVisibility(null);
  }, [project]);

  async function doSave() {
    setErrorMessage(null);
    try {
      await onSave({
        name: name.trim(),
        description: description.trim() || undefined,
        visibility,
        icon: icon.trim() || null,
        color,
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
      {/* Wider and scrollable: the Packs tab prints the host's own verdict —
          repository coordinates, a seed commit, per-role notes — and at
          `max-w-lg` a 64-hex coordinate wrapped across three lines and the
          result pushed the buttons off the bottom. */}
      <DialogContent className="max-h-[85vh] max-w-3xl overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Project settings</DialogTitle>
          <DialogDescription>
            The project id ({project?.dtag}) stays the same.
          </DialogDescription>
        </DialogHeader>
        <Tabs defaultValue="general">
          <TabsList>
            <TabsTrigger
              data-testid="project-settings-tab-general"
              value="general"
            >
              General
            </TabsTrigger>
            <TabsTrigger
              data-testid="project-settings-tab-members"
              value="members"
            >
              Members
            </TabsTrigger>
            <TabsTrigger data-testid="project-settings-tab-local" value="local">
              This computer
            </TabsTrigger>
            <TabsTrigger data-testid="project-settings-tab-packs" value="packs">
              Packs
            </TabsTrigger>
            <TabsTrigger
              data-testid="project-settings-tab-repository"
              value="repository"
            >
              Repository
            </TabsTrigger>
          </TabsList>

          <TabsContent value="general">
            <form onSubmit={handleSubmit}>
              <div className="flex flex-col gap-3">
                {canEditProject ? null : (
                  <p
                    className="text-xs text-muted-foreground"
                    data-testid="edit-project-container-readonly-note"
                  >
                    {capabilities.isOwner
                      ? // Saying "only the owner can change these" to
                        // somebody who *is* an owner names the wrong reason
                        // and reads as a bug. Name the real one.
                        "These settings live on the project's own event, and only the key that created it can republish that. You can still manage members and delete the project."
                      : "Only a project owner can change these settings."}
                  </p>
                )}
                <div className="flex items-center gap-2">
                  <ProjectIconPickerField
                    disabled={!canEditProject}
                    icon={icon}
                    onIconChange={setIcon}
                  />
                  <Input
                    aria-label="Project name"
                    autoFocus
                    className="flex-1"
                    data-testid="edit-project-container-name"
                    disabled={!canEditProject}
                    onChange={(event) => setName(event.target.value)}
                    placeholder="Project name"
                    value={name}
                  />
                </div>
                <Textarea
                  aria-label="Project description"
                  data-testid="edit-project-container-description"
                  disabled={!canEditProject}
                  onChange={(event) => setDescription(event.target.value)}
                  placeholder="What is this project about? (optional)"
                  rows={3}
                  value={description}
                />
                <ProjectColorPickerField
                  color={color}
                  disabled={!canEditProject}
                  onColorChange={setColor}
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
                    disabled={!canEditProject}
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
                {errorMessage ? (
                  <p className="text-sm text-destructive">{errorMessage}</p>
                ) : null}
                {canDelete ? (
                  <div
                    className="mt-2 flex flex-col gap-2 rounded-xl border border-destructive/40 p-3"
                    data-testid="project-settings-danger-zone"
                  >
                    <p className="text-sm font-medium">Delete this project</p>
                    <p className="text-xs text-muted-foreground">
                      Removes the project for everyone, and offers to take its
                      channels, repositories and the agents it created on this
                      computer with it. You choose what goes on the next screen.
                    </p>
                    <div>
                      <Button
                        data-testid="project-settings-delete"
                        // `capabilities.isLoading` too: the roster decides
                        // who may delete, and mid-load it has collapsed
                        // toward empty. The affordance is already hidden for
                        // a non-owner; this stops the one case where the
                        // roster has not said yet.
                        disabled={isSaving || capabilities.isLoading}
                        onClick={() => {
                          // Close settings first, so the confirmation the
                          // caller opens is not stacked on top of a dialog
                          // describing a project that is about to be gone.
                          onOpenChange(false);
                          onRequestDelete?.();
                        }}
                        type="button"
                        variant="destructive"
                      >
                        Delete project…
                      </Button>
                    </div>
                  </div>
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
                {canEditProject ? (
                  <Button
                    data-testid="edit-project-container-save"
                    disabled={isSaving || name.trim() === ""}
                    type="submit"
                  >
                    Save
                  </Button>
                ) : null}
              </DialogFooter>
            </form>
          </TabsContent>

          <TabsContent value="members">
            {project ? <ProjectMembersManager project={project} /> : null}
          </TabsContent>

          <TabsContent value="local">
            {project ? <ProjectSettingsLocalSection project={project} /> : null}
          </TabsContent>

          <TabsContent value="packs">
            {project ? <ProjectPacksSettingsSection project={project} /> : null}
          </TabsContent>

          <TabsContent value="repository">
            {project ? (
              <ProjectRepositoryProtectionSection project={project} />
            ) : null}
          </TabsContent>
        </Tabs>
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
                : "Everyone in the community will be able to see this project. Members keep their roles."}
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
