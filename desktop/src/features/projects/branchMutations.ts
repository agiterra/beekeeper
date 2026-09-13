import { useMutation, useQueryClient } from "@tanstack/react-query";
import * as React from "react";
import { toast } from "sonner";

import type { Repository as Project } from "@/features/projects/hooks";
import {
  createProjectRemoteBranch,
  deleteProjectRemoteBranch,
} from "@/shared/api/projectGit";

/** Creates a remote branch from an observed branch commit. */
export function useCreateProjectRemoteBranchMutation(
  project: Project | null | undefined,
) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: {
      sourceBranch: string;
      expectedCommit: string;
      newBranch: string;
    }) => {
      if (!project?.cloneUrls[0]) throw new Error("No project selected.");
      return createProjectRemoteBranch({
        cloneUrl: project.cloneUrls[0],
        ...input,
      });
    },
    onSuccess: () => {
      // The remote operation already succeeded. Unrelated project reads must
      // not keep its mutation pending; query errors remain in their sections.
      void queryClient.invalidateQueries({
        queryKey: ["project", project?.id ?? "none"],
      });
    },
  });
}

/** Deletes a remote branch only if it still points at the observed commit. */
export function useDeleteProjectRemoteBranchMutation(
  project: Project | null | undefined,
) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { branch: string; expectedCommit: string }) => {
      if (!project?.cloneUrls[0]) throw new Error("No project selected.");
      return deleteProjectRemoteBranch({
        cloneUrl: project.cloneUrls[0],
        ...input,
      });
    },
    onSuccess: () => {
      // The remote operation already succeeded. Unrelated project reads must
      // not keep its mutation pending; query errors remain in their sections.
      void queryClient.invalidateQueries({
        queryKey: ["project", project?.id ?? "none"],
      });
    },
  });
}

function refreshAfterBranchMutation(
  refetch: () => Promise<unknown>,
  action: "created" | "deleted",
) {
  void refetch()
    .then((result) => {
      // React Query refetch normally resolves with an error result rather
      // than rejecting. Either form is a read failure after a successful write.
      if (
        result &&
        typeof result === "object" &&
        "error" in result &&
        result.error
      ) {
        throw result.error;
      }
    })
    .catch((error: unknown) => {
      const message = error instanceof Error ? error.message : "Unknown error.";
      toast.warning(
        `Branch ${action}, but repository refresh failed: ${message}`,
      );
    });
}

/** Coordinates branch dialogs and refreshes around the remote mutations. */
export function useProjectBranchActions(input: {
  project: Project | null | undefined;
  activeBranch: string | null;
  activeBranchCommit: string | null;
  activeRemoteBranch: { name: string; commit: string } | null;
  defaultBranch: string | null;
  deleteBranchReason: string | null;
  refetchRepoState: () => Promise<unknown>;
  rememberBranch: (branch: { name: string; commit: string }) => void;
  forgetBranch: (branch: string) => void;
  selectBranch: (branch: string | null) => void;
}) {
  const [createOpen, setCreateOpen] = React.useState(false);
  const [deleteOpen, setDeleteOpen] = React.useState(false);
  const createMutation = useCreateProjectRemoteBranchMutation(input.project);
  const deleteMutation = useDeleteProjectRemoteBranchMutation(input.project);
  const createBranch = createMutation.mutateAsync;
  const deleteBranch = deleteMutation.mutateAsync;

  const handleCreate = React.useCallback(
    async (newBranch: string) => {
      if (!input.activeBranch || !input.activeBranchCommit) {
        throw new Error("Refresh the source branch before creating a branch.");
      }
      const result = await createBranch({
        sourceBranch: input.activeBranch,
        expectedCommit: input.activeBranchCommit,
        newBranch,
      });
      input.rememberBranch({ name: result.branch, commit: result.commit });
      input.selectBranch(result.branch);
      toast.success(result.message);
      refreshAfterBranchMutation(input.refetchRepoState, "created");
    },
    [
      createBranch,
      input.activeBranch,
      input.activeBranchCommit,
      input.refetchRepoState,
      input.rememberBranch,
      input.selectBranch,
    ],
  );
  const handleDelete = React.useCallback(async () => {
    if (
      !input.activeBranch ||
      !input.activeRemoteBranch ||
      input.deleteBranchReason
    ) {
      throw new Error(input.deleteBranchReason ?? "Choose a remote branch.");
    }
    const result = await deleteBranch({
      branch: input.activeBranch,
      expectedCommit: input.activeRemoteBranch.commit,
    });
    input.forgetBranch(result.branch);
    input.selectBranch(input.defaultBranch);
    toast.success(result.message);
    refreshAfterBranchMutation(input.refetchRepoState, "deleted");
  }, [
    deleteBranch,
    input.activeBranch,
    input.activeRemoteBranch,
    input.defaultBranch,
    input.deleteBranchReason,
    input.forgetBranch,
    input.refetchRepoState,
    input.selectBranch,
  ]);

  return {
    createOpen,
    createPending: createMutation.isPending,
    deleteOpen,
    deletePending: deleteMutation.isPending,
    handleCreate,
    handleDelete,
    setCreateOpen,
    setDeleteOpen,
  };
}
