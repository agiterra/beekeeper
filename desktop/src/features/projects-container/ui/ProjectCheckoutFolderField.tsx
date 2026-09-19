import { FolderOpen } from "lucide-react";
import * as React from "react";

import { pickCodingSessionWorkdir } from "@/shared/api/tauriCodingSessionWorkdirs";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";

import {
  checkoutParentFromPath,
  HOST_DEFAULT_REPOS_ROOT_LABEL,
  projectCheckoutPath,
} from "../lib/projectCheckoutFolder";
import { useDefaultRepositoryFolder } from "../lib/useDefaultRepositoryFolder";

/** What the row resolved: the parent to send the host, and the destination. */
export type ProjectCheckoutFolderChoice = {
  /** The folder the clone goes under; `null` = the host's default root. */
  parent: string | null;
  /** `<parent>/<slug>` as the row shows it, `""` when nothing is known yet. */
  destination: string;
};

/**
 * "Repository folder" — where this computer clones the project's code.
 *
 * Pre-filled live with `<default repository folder>/<slug>` (the active
 * community's Repositories folder, else the host's default root) until the
 * person types or browses; then the text is theirs. The helper line always
 * names the real destination the host will use, because the host takes the
 * PARENT and names the folder after the repository: text ending in
 * `/<slug>` is read as the destination, anything else as the parent.
 */
export function ProjectCheckoutFolderField({
  disabled = false,
  onChange,
  slug,
  testIdPrefix = "project-checkout-folder",
}: {
  disabled?: boolean;
  /** Fires on every change, with the parent to hand `project_agents_init`. */
  onChange: (choice: ProjectCheckoutFolderChoice) => void;
  /** The project's `d` tag — the clone's own folder name. */
  slug: string;
  testIdPrefix?: string;
}) {
  const defaultFolder = useDefaultRepositoryFolder();
  const [typed, setTyped] = React.useState<string | null>(null);
  const prefilled = projectCheckoutPath(defaultFolder.path, slug);
  const text = typed ?? prefilled;
  const parent =
    typed === null ? defaultFolder.path : checkoutParentFromPath(typed, slug);
  const destination = projectCheckoutPath(parent, slug);

  // One call per resolved choice, so a parent sitting in the caller's state
  // is always the one the row currently shows.
  const lastReported = React.useRef<string | null>(null);
  React.useEffect(() => {
    const key = `${parent ?? ""} ${destination}`;
    if (lastReported.current === key) return;
    lastReported.current = key;
    onChange({ parent, destination });
  }, [destination, onChange, parent]);

  const handleBrowse = React.useCallback(() => {
    void pickCodingSessionWorkdir()
      .then((picked) => {
        if (picked) setTyped(projectCheckoutPath(picked, slug) || picked);
      })
      .catch(() => {
        // A cancelled or unavailable picker leaves the text field in charge.
      });
  }, [slug]);

  const fallbackLabel = `${HOST_DEFAULT_REPOS_ROOT_LABEL}/${slug || "<project>"}`;
  const hint =
    destination.length > 0
      ? `The project's code repository is cloned to ${destination}; every seat's worktree is cut from it.`
      : slug.length === 0
        ? "Name the project to see where its code repository is cloned; every seat's worktree is cut from it."
        : `No folder chosen: the project's code repository is cloned under the host's default, ${fallbackLabel}; every seat's worktree is cut from it.`;

  return (
    <div className="flex flex-col gap-1.5">
      <label
        className="text-xs font-medium text-muted-foreground"
        htmlFor={`${testIdPrefix}-input`}
      >
        Repository folder
      </label>
      <div className="flex items-center gap-2">
        <Input
          autoComplete="off"
          className="font-mono text-xs placeholder:text-muted-foreground/50"
          data-checkout-parent={parent ?? ""}
          data-testid={testIdPrefix}
          disabled={disabled}
          id={`${testIdPrefix}-input`}
          onChange={(event) => setTyped(event.target.value)}
          placeholder={fallbackLabel}
          spellCheck={false}
          value={text}
        />
        <Button
          data-testid={`${testIdPrefix}-browse`}
          disabled={disabled}
          onClick={handleBrowse}
          size="sm"
          type="button"
          variant="outline"
        >
          <FolderOpen />
          Browse
        </Button>
      </div>
      <p
        className="text-2xs text-muted-foreground"
        data-testid={`${testIdPrefix}-hint`}
      >
        {hint}
      </p>
    </div>
  );
}
