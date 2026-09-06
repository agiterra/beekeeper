import type * as React from "react";

import { Button } from "@/shared/ui/button";
import type { NewCodingSessionTarget } from "../lib/newCodingSessionModel";

/** Summarize the selected runtime without claiming foreign targets are local. */
export function formatNewCodingSessionSetupProvider(
  target: NewCodingSessionTarget | null,
) {
  if (!target) return "No provider selected";
  const name = target.availability?.label ?? target.provider.runtime;
  if (target.availability?.state === "missing")
    return `${name} · not installed`;
  if (target.availability?.state === "needs_auth") {
    return `${name} · sign-in needed`;
  }
  return name;
}

/**
 * The launcher's compact setup summary and its mounted advanced controls.
 * Keeping the disclosure shell separate leaves the form responsible for the
 * existing state, readiness, and submit behavior.
 */
export function NewCodingSessionSetupDisclosure({
  children,
  configurationOpen,
  governed,
  onConfigurationOpenChange,
  setupLead,
  setupModel,
  setupProvider,
}: {
  children: React.ReactNode;
  configurationOpen: boolean;
  governed: boolean;
  onConfigurationOpenChange: (open: boolean) => void;
  setupLead: string;
  setupModel: string;
  setupProvider: string;
}) {
  return (
    <>
      <section
        aria-label="Current session setup"
        className="flex flex-col gap-2 rounded-lg border border-border/60 bg-muted/20 px-3 py-2.5"
        data-testid="new-coding-session-setup-summary"
      >
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div className="min-w-0">
            <p className="text-sm font-medium">Current setup</p>
            <p className="truncate text-2xs text-muted-foreground">
              {setupLead} · {setupProvider} · {setupModel}
            </p>
          </div>
          <Button
            data-testid="new-coding-session-edit-setup"
            onClick={() => onConfigurationOpenChange(true)}
            size="sm"
            type="button"
            variant="outline"
          >
            Change setup
          </Button>
        </div>
        <p className="text-2xs text-muted-foreground">
          Using the setup shown above. Change it below if needed; advanced
          settings are optional.
        </p>
      </section>

      <details
        className="rounded-lg border border-border/60 bg-muted/20 px-3 py-2.5"
        data-testid="new-coding-session-configuration"
        onToggle={(event) =>
          onConfigurationOpenChange(event.currentTarget.open)
        }
        open={configurationOpen}
      >
        <summary className="cursor-pointer text-xs font-medium text-muted-foreground">
          Setup and advanced options
          {governed
            ? " · lead, runtime, bench and policy"
            : " · lead and runtime"}
        </summary>
        <div className="mt-3 flex flex-col gap-5">{children}</div>
      </details>
    </>
  );
}
