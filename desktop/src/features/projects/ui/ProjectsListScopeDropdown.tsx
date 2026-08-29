import { Check, ChevronDown } from "lucide-react";

import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";

export function ProjectsListScopeDropdown<T extends string>({
  label,
  modal = true,
  onChange,
  options,
  triggerTestId,
  value,
}: {
  label: string;
  /**
   * Radix's modal menu puts `pointer-events: none` on the body while it is
   * open, which keeps swallowing clicks for a beat after a selection closes
   * it. Callers embedded in a page the operator keeps clicking (rather than a
   * toolbar) pass `false`, as every other menu in the agents library does.
   */
  modal?: boolean;
  onChange: (value: T) => void;
  options: Array<{ label: string; value: T }>;
  /** Set by callers a test has to drive the trigger of. */
  triggerTestId?: string;
  value: T;
}) {
  const selectedLabel =
    options.find((option) => option.value === value)?.label ??
    options[0]?.label;

  return (
    <DropdownMenu modal={modal}>
      <DropdownMenuTrigger asChild>
        <Button
          aria-label={label}
          className="-ml-2 h-8 gap-1.5 px-2 text-xs font-medium"
          data-testid={triggerTestId}
          variant="ghost"
        >
          {selectedLabel}
          <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="min-w-44">
        {options.map((option) => (
          <DropdownMenuItem
            className="justify-between"
            key={option.value}
            onSelect={() => onChange(option.value)}
          >
            {option.label}
            {option.value === value ? <Check className="h-4 w-4" /> : null}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
