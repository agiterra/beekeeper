import { Check, Database } from "lucide-react";
import * as React from "react";

import { Button } from "@/shared/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";

import type {
  PlanningProviderDescriptor,
  PlanningProviderId,
} from "../lib/planningTypes";

/**
 * Where this project's issues live, and what else could hold them: the
 * Planning plugins this relay runs, then providers this build knows of but
 * cannot connect. The provider in use is stated; an unavailable one is
 * listed with why and has no control. With more than one available, the
 * viewer may switch what they are looking at — for this view only.
 */
export function PlanningProviderControl({
  providers,
  active,
  onChoose,
}: {
  providers: readonly PlanningProviderDescriptor[];
  active: PlanningProviderId | null;
  onChoose?: (id: PlanningProviderId) => void;
}) {
  const [open, setOpen] = React.useState(false);
  const current = providers.find((provider) => provider.id === active) ?? null;
  const available = providers.filter(
    (p) => p.availability.kind === "available",
  );
  return (
    <Popover onOpenChange={setOpen} open={open}>
      <PopoverTrigger asChild>
        <Button
          aria-label={`Planning provider: ${current?.label ?? "none"}`}
          data-testid="planning-provider"
          size="xs"
          variant="ghost"
        >
          <Database aria-hidden="true" />
          {current?.label ?? "No provider"}
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
        {available.length > 1 ? (
          <p
            className="px-1 pb-2 text-2xs text-muted-foreground"
            data-testid="planning-provider-several"
          >
            This relay runs {available.length} Planning providers. Choosing one
            changes what you see here, not the project: a shared choice for
            every member is not built yet.
          </p>
        ) : null}
        <ul className="flex flex-col gap-1">
          {providers.map((provider) => {
            const isActive = provider.id === active;
            const unavailable = provider.availability.kind === "unavailable";
            const choosable =
              !isActive && !unavailable && onChoose !== undefined;
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
                  ) : choosable ? (
                    <Button
                      data-testid={`planning-provider-${provider.id}-choose`}
                      onClick={() => {
                        onChoose?.(provider.id);
                        setOpen(false);
                      }}
                      size="xs"
                      variant="outline"
                    >
                      View
                    </Button>
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
