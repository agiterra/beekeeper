import { Check, Database } from "lucide-react";
import * as React from "react";

import { Button } from "@/shared/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";

import { PLANNING_PROVIDERS } from "../lib/planningProviders";
import type { PlanningProviderId } from "../lib/planningTypes";

/**
 * Where this project's issues live, and what else could hold them. The
 * active provider is stated; an unavailable one is listed with why, and has
 * no control — choosing it would have nothing to connect to.
 */
export function PlanningProviderControl({
  active,
}: {
  active: PlanningProviderId;
}) {
  const [open, setOpen] = React.useState(false);
  const current =
    PLANNING_PROVIDERS.find((provider) => provider.id === active) ??
    PLANNING_PROVIDERS[0];
  return (
    <Popover onOpenChange={setOpen} open={open}>
      <PopoverTrigger asChild>
        <Button
          aria-label={`Planning provider: ${current?.label}`}
          data-testid="planning-provider"
          size="xs"
          variant="ghost"
        >
          <Database aria-hidden="true" />
          {current?.label}
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        className="w-80 p-2"
        data-testid="planning-provider-panel"
      >
        <p className="px-1 pb-2 text-xs font-medium text-foreground">
          Where this project's issues live
        </p>
        <ul className="flex flex-col gap-1">
          {PLANNING_PROVIDERS.map((provider) => {
            const isActive = provider.id === active;
            const unavailable = provider.availability.kind === "unavailable";
            return (
              <li
                aria-current={isActive ? "true" : undefined}
                className="rounded-md border border-border px-2 py-1.5"
                data-testid={`planning-provider-${provider.id}`}
                key={provider.id}
              >
                <div className="flex items-center gap-2 text-sm">
                  <span className="font-medium text-foreground">
                    {provider.label}
                  </span>
                  <span className="flex-1" />
                  {isActive ? (
                    <span className="inline-flex items-center gap-1 text-2xs text-foreground">
                      <Check aria-hidden="true" className="h-3 w-3" />
                      In use
                    </span>
                  ) : unavailable ? (
                    <span className="text-2xs text-muted-foreground">
                      Unavailable
                    </span>
                  ) : null}
                </div>
                <p className="mt-0.5 text-2xs text-muted-foreground">
                  {provider.description}
                </p>
                {provider.availability.kind === "unavailable" ? (
                  <p
                    className="mt-1 text-2xs text-foreground"
                    data-testid={`planning-provider-${provider.id}-reason`}
                  >
                    {provider.availability.reason}
                  </p>
                ) : null}
              </li>
            );
          })}
        </ul>
      </PopoverContent>
    </Popover>
  );
}
