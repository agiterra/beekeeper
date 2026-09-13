import { CircleAlert } from "lucide-react";

import type { resolveCodingSessionWorkspace } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import { FuzzyLogo } from "@/shared/ui/buzz-logo/FuzzyLogo";

import { CodingSessionHeader } from "./CodingSessionHeader";

/**
 * The workspace before a generation resolves: loading, untrusted or not found.
 * Split out of `CodingSessionWorkspace.tsx` to keep that file under the
 * repository's 1,000-line ceiling; nothing about its markup changed.
 */
export function CodingSessionWorkspaceState({
  channelName,
  generationId,
  onClose,
  resolution,
}: {
  channelName: string | null;
  generationId: string;
  /** Closes the pop-out window. Absent in the main window, where the app's
   * own back/forward in the top chrome is the way out of a session. */
  onClose?: () => void;
  resolution: Exclude<
    ReturnType<typeof resolveCodingSessionWorkspace>,
    { kind: "ready" }
  >;
}) {
  const loading = resolution.kind === "loading";
  return (
    <main
      className="flex h-full min-h-0 flex-1 flex-col bg-background"
      data-testid={`coding-session-workspace-${resolution.kind}`}
    >
      <CodingSessionHeader
        channelName={channelName}
        generationLabel={shortGenerationId(generationId)}
        onClose={onClose}
        status={{ kind: "unknown", label: "Status unknown" }}
      />
      <div className="flex min-h-0 flex-1 items-center justify-center px-6 py-10 text-center">
        <div className="max-w-md">
          {loading ? (
            <FuzzyLogo
              ariaLabel="Loading coding session"
              className="mx-auto text-muted-foreground"
              fuzz={false}
              loop
            />
          ) : (
            <CircleAlert className="mx-auto h-5 w-5 text-muted-foreground" />
          )}
          <h2 className="mt-4 text-base font-semibold">
            {loading
              ? "Loading coding session"
              : resolution.kind === "untrusted"
                ? "Generation not trusted"
                : "Generation not found"}
          </h2>
          <p className="mt-2 text-sm text-muted-foreground">
            {loading
              ? "Resolving the exact signed generation from the relay catalog."
              : resolution.description}
          </p>
        </div>
      </div>
    </main>
  );
}

function shortGenerationId(value: string): string {
  return value.length <= 28 ? value : `${value.slice(0, 28)}…`;
}
