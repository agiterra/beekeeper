import { FolderKanban, X } from "lucide-react";
import * as React from "react";

import { EmojiPicker } from "@/features/custom-emoji/ui/EmojiPicker";
import { StatusEmoji } from "@/features/user-status/ui/StatusEmoji";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";

/**
 * Emoji picker button for a project's display icon. Empty value renders the
 * stock FolderKanban glyph — the same fallback the sidebar shows for
 * icon-less projects.
 */
export function ProjectIconPickerField({
  icon,
  onIconChange,
  disabled,
  testIdPrefix = "edit-project-container",
}: {
  icon: string;
  onIconChange: (icon: string) => void;
  disabled?: boolean;
  testIdPrefix?: string;
}) {
  const [pickerOpen, setPickerOpen] = React.useState(false);

  return (
    <Popover onOpenChange={setPickerOpen} open={pickerOpen}>
      <div className="relative shrink-0">
        <PopoverTrigger asChild>
          <button
            aria-label="Choose project icon"
            className="flex h-9 w-9 items-center justify-center rounded-md border border-input text-lg transition-colors hover:bg-accent disabled:pointer-events-none disabled:opacity-50"
            data-testid={`${testIdPrefix}-icon`}
            disabled={disabled}
            type="button"
          >
            {icon ? (
              <StatusEmoji className="h-5 w-5" value={icon} />
            ) : (
              <FolderKanban className="h-4 w-4 text-muted-foreground" />
            )}
          </button>
        </PopoverTrigger>
        {icon && !disabled ? (
          <button
            aria-label="Clear project icon"
            className="absolute -right-1 -top-1 flex h-4 w-4 items-center justify-center rounded-full border border-background bg-muted text-muted-foreground hover:bg-accent hover:text-foreground"
            data-testid={`${testIdPrefix}-icon-clear`}
            onClick={(event) => {
              event.stopPropagation();
              onIconChange("");
            }}
            type="button"
          >
            <X className="h-3 w-3" />
          </button>
        ) : null}
      </div>
      <PopoverContent
        align="start"
        className="w-auto overflow-hidden rounded-2xl p-0"
        sideOffset={4}
      >
        <EmojiPicker
          onSelect={(emoji) => {
            onIconChange(emoji);
            setPickerOpen(false);
          }}
        />
      </PopoverContent>
    </Popover>
  );
}
