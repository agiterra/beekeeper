import { ChevronDown, LoaderCircle } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";

import type { ProjectContainer } from "../hooks";

/** Radio dropdown for a project container's `visibility` (public/private).
 * Modeled on `ChannelPermissionsSettings`. */
export function ProjectVisibilitySettings({
  disabled,
  isPending = false,
  onVisibilityChange,
  testIdPrefix,
  visibility,
}: {
  disabled?: boolean;
  isPending?: boolean;
  onVisibilityChange: (visibility: ProjectContainer["visibility"]) => void;
  testIdPrefix: string;
  visibility: ProjectContainer["visibility"];
}) {
  const visibilityLabel = visibility === "private" ? "Private" : "Public";

  return (
    <div
      className={cn(
        "flex min-h-12 items-center justify-between gap-4 rounded-xl border border-input bg-background px-3 py-3",
        disabled && "opacity-50",
      )}
      data-testid={`${testIdPrefix}-visibility-container`}
    >
      <span className="text-sm font-medium text-foreground">Visibility</span>
      <DropdownMenu modal={false}>
        <DropdownMenuTrigger asChild>
          <Button
            aria-busy={isPending}
            aria-label={
              isPending
                ? "Updating visibility"
                : `Visibility: ${visibilityLabel}`
            }
            className="-mr-2.5 ml-auto h-9 w-fit justify-end px-2.5 text-right text-sm font-medium text-foreground hover:bg-muted/50"
            data-testid={`${testIdPrefix}-visibility`}
            disabled={disabled}
            type="button"
            variant="ghost"
          >
            <span aria-live="polite" className="text-right">
              {isPending ? "Updating…" : visibilityLabel}
            </span>
            {isPending ? (
              <LoaderCircle
                aria-hidden="true"
                className="size-4 shrink-0 text-muted-foreground/70 motion-safe:animate-spin"
              />
            ) : (
              <ChevronDown className="size-4 shrink-0 text-muted-foreground/70" />
            )}
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          onCloseAutoFocus={(event) => event.preventDefault()}
          style={{
            minWidth: "var(--radix-dropdown-menu-trigger-width)",
          }}
        >
          <DropdownMenuRadioGroup
            onValueChange={(nextVisibility) =>
              onVisibilityChange(
                nextVisibility === "private" ? "private" : "public",
              )
            }
            value={visibility}
          >
            <DropdownMenuRadioItem
              data-testid={`${testIdPrefix}-visibility-option-public`}
              value="public"
            >
              Public
            </DropdownMenuRadioItem>
            <DropdownMenuRadioItem
              data-testid={`${testIdPrefix}-visibility-option-private`}
              value="private"
            >
              Private
            </DropdownMenuRadioItem>
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
