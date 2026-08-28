import { Ban } from "lucide-react";

import { contrastColorForBackground } from "@/features/profile/ui/ProfileAvatarEditor.utils";
import { ACCENT_COLORS, NEUTRAL_ACCENT } from "@/shared/theme/ThemeProvider";

/** Tint swatches: the accent palette minus the theme-relative neutral —
 * a project tint must be a concrete color that reads in both modes. */
export const PROJECT_COLOR_SWATCHES = ACCENT_COLORS.filter(
  (color) => color.value !== NEUTRAL_ACCENT,
);

/**
 * Swatch row for a project's background color. `null` means no tint — the
 * leading crossed-out swatch. Colors tint the project's sidebar group and
 * the content pane while its content is active.
 */
export function ProjectColorPickerField({
  color,
  onColorChange,
  disabled,
  testIdPrefix = "edit-project-container",
}: {
  color: string | null;
  onColorChange: (color: string | null) => void;
  disabled?: boolean;
  testIdPrefix?: string;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-sm text-muted-foreground">Color</span>
      <div
        className="flex flex-wrap gap-2"
        data-testid={`${testIdPrefix}-color-options`}
      >
        <button
          aria-label="No project color"
          aria-pressed={color === null}
          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-border text-muted-foreground transition-transform duration-200 ease-out hover:scale-[1.15] focus-visible:scale-[1.15] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50 motion-reduce:transform-none motion-reduce:transition-none"
          data-testid={`${testIdPrefix}-color-none`}
          disabled={disabled}
          onClick={() => onColorChange(null)}
          title="None"
          type="button"
        >
          <Ban className="h-4 w-4" />
          {color === null ? (
            <span className="absolute h-8 w-8 rounded-full border-2 border-ring" />
          ) : null}
        </button>
        {PROJECT_COLOR_SWATCHES.map((swatch) => {
          const value = swatch.value.toLowerCase();
          const isSelected = color === value;
          return (
            <button
              aria-label={`Use ${swatch.name} project color`}
              aria-pressed={isSelected}
              className="relative h-8 w-8 shrink-0 rounded-full border border-border transition-transform duration-200 ease-out hover:scale-[1.15] focus-visible:scale-[1.15] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50 motion-reduce:transform-none motion-reduce:transition-none"
              data-testid={`${testIdPrefix}-color-${swatch.name.toLowerCase()}`}
              disabled={disabled}
              key={swatch.value}
              onClick={() => onColorChange(value)}
              style={{ backgroundColor: swatch.value }}
              title={swatch.name}
              type="button"
            >
              {isSelected ? (
                <span
                  className="absolute inset-1 rounded-full border-[3px]"
                  style={{
                    borderColor: contrastColorForBackground(swatch.value),
                  }}
                />
              ) : null}
            </button>
          );
        })}
      </div>
    </div>
  );
}
