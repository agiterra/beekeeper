import { FolderOpen } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import { expandTilde } from "@/features/communities/communityStorage";
import { useCommunities } from "@/features/communities/useCommunities";
import { HOST_DEFAULT_REPOS_ROOT_LABEL } from "@/features/projects-container/lib/projectCheckoutFolder";
import { defaultReposRoot } from "@/features/projects-container/lib/projectAgentsInit";
import { validateReposDir } from "@/shared/api/tauri";
import { pickCodingSessionWorkdir } from "@/shared/api/tauriCodingSessionWorkdirs";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";

/**
 * "Default repository folder" — the folder a new project's code repository
 * is cloned under (`<folder>/<slug>`), and the folder agents' `REPOS`
 * symlinks point at.
 *
 * This is the active community's `reposDir`, the same value Edit community →
 * Repositories folder edits (`EditCommunityDialog.tsx`): one value, two
 * doors. It is saved the same way — `~` expanded, the path validated by the
 * host, and the community updated (which re-applies the backend config, as
 * that dialog's save does). Blank means the host's own default, which the
 * placeholder names from the host rather than from memory.
 */
export function DefaultRepositoryFolderCard() {
  const { activeCommunity, updateCommunity } = useCommunities();
  const saved = activeCommunity?.reposDir ?? "";
  const [draft, setDraft] = React.useState(saved);
  const [error, setError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [hostDefault, setHostDefault] = React.useState<string | null>(null);

  React.useEffect(() => {
    setDraft(saved);
  }, [saved]);

  React.useEffect(() => {
    let cancelled = false;
    void defaultReposRoot()
      .then((root) => {
        if (!cancelled) setHostDefault(root);
      })
      .catch(() => {
        if (!cancelled) setHostDefault(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const commit = React.useCallback(
    async (value: string) => {
      if (!activeCommunity) return;
      setError(null);
      const expanded = await expandTilde(value);
      if ((expanded ?? undefined) === (activeCommunity.reposDir || undefined)) {
        setDraft(expanded ?? "");
        return;
      }
      setBusy(true);
      try {
        await validateReposDir(expanded ?? "");
      } catch (thrown) {
        setError(String(thrown));
        setBusy(false);
        return;
      }
      const result = updateCommunity(activeCommunity.id, {
        reposDir: expanded,
      });
      setBusy(false);
      if (result.kind !== "updated") {
        setError("The community could not be updated.");
        return;
      }
      setDraft(expanded ?? "");
      toast.success(
        expanded
          ? `Default repository folder set to ${expanded}.`
          : `Default repository folder cleared; new clones go under ${hostDefault ?? HOST_DEFAULT_REPOS_ROOT_LABEL}.`,
      );
    },
    [activeCommunity, hostDefault, updateCommunity],
  );

  const handleBrowse = React.useCallback(() => {
    void pickCodingSessionWorkdir()
      .then((picked) => {
        if (picked) {
          setDraft(picked);
          void commit(picked);
        }
      })
      .catch(() => {
        // A cancelled or unavailable picker leaves the text field in charge.
      });
  }, [commit]);

  const placeholder = hostDefault ?? HOST_DEFAULT_REPOS_ROOT_LABEL;

  return (
    <div
      className="flex flex-col gap-2 px-4 py-4"
      data-testid="settings-default-repository-folder"
    >
      <label
        className="text-sm font-medium"
        htmlFor="settings-default-repository-folder-input"
      >
        Default repository folder
      </label>
      <div className="flex items-center gap-2">
        <Input
          autoComplete="off"
          className="font-mono text-xs placeholder:text-muted-foreground/50"
          data-testid="settings-default-repository-folder-input"
          disabled={busy || activeCommunity === null}
          id="settings-default-repository-folder-input"
          onBlur={() => void commit(draft)}
          onChange={(event) => {
            setDraft(event.target.value);
            setError(null);
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              void commit(draft);
            }
          }}
          placeholder={placeholder}
          spellCheck={false}
          value={draft}
        />
        <Button
          data-testid="settings-default-repository-folder-browse"
          disabled={busy || activeCommunity === null}
          onClick={handleBrowse}
          size="sm"
          type="button"
          variant="outline"
        >
          <FolderOpen />
          Browse
        </Button>
      </div>
      {error ? (
        <p
          className="text-xs text-destructive"
          data-testid="settings-default-repository-folder-error"
          role="alert"
        >
          {error}
        </p>
      ) : null}
      <p className="text-xs text-muted-foreground">
        A new project&apos;s code repository is cloned to{" "}
        <code>{draft.trim() || placeholder}/&lt;project&gt;</code> and every
        seat&apos;s worktree is cut from it. Blank uses the host&apos;s default,{" "}
        <code>{placeholder}</code>. This is the same value as Edit community →
        Repositories folder for{" "}
        {activeCommunity ? activeCommunity.name : "the active community"}.
      </p>
    </div>
  );
}
