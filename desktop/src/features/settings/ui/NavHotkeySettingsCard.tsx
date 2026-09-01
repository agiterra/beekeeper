import * as React from "react";
import { ChevronDown } from "lucide-react";

import {
  DEFAULT_NAV_HOTKEY_BINDINGS,
  findNavHotkeyConflicts,
  isBindableNavHotkeyCode,
  navHotkeyCodeLabel,
  navHotkeyModifierGlyph,
  navHotkeyModifierLabel,
  NAV_HOTKEY_ACTIONS,
  NAV_HOTKEY_ACTION_DESCRIPTIONS,
  NAV_HOTKEY_ACTION_LABELS,
  NAV_HOTKEY_MAX_POSITIONS,
  NAV_HOTKEY_MODIFIERS,
  type NavHotkeyAction,
  type NavHotkeyModifier,
} from "@/features/hotkeys/lib/navHotkeyBindings";
import { findNavHotkeyRegistryConflicts } from "@/features/hotkeys/lib/navHotkeyRegistryConflicts";
import {
  updateNavHotkeyBindings,
  useNavHotkeyBindings,
} from "@/features/hotkeys/lib/navHotkeyBindingsStore";
import { isMacPlatform } from "@/shared/lib/platform";
import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { Switch } from "@/shared/ui/switch";

import { SettingsOptionGroup, SettingsOptionRow } from "./SettingsOptionGroup";

const TRIGGER_CLASS =
  "h-7 min-w-28 justify-between gap-1.5 rounded-md border border-border/50 bg-muted/45 px-2.5 text-xs font-medium text-foreground shadow-none hover:bg-muted/70";

function ModifierSetting({
  description,
  disabled,
  label,
  onChange,
  testId,
  value,
}: {
  description: string;
  disabled: boolean;
  label: string;
  onChange: (next: NavHotkeyModifier) => void;
  testId: string;
  value: NavHotkeyModifier;
}) {
  const isMac = isMacPlatform();
  return (
    <SettingsOptionRow>
      <div className="min-w-0">
        <p className="text-sm font-medium">{label}</p>
        <p
          className="text-sm font-normal text-muted-foreground/70"
          data-settings-subcopy
        >
          {description}
        </p>
      </div>
      <DropdownMenu modal={false}>
        <DropdownMenuTrigger asChild>
          <Button
            className={TRIGGER_CLASS}
            data-testid={`${testId}-trigger`}
            disabled={disabled}
            size="sm"
            type="button"
            variant="ghost"
          >
            <span className="truncate">
              {navHotkeyModifierGlyph(value, isMac)}{" "}
              {navHotkeyModifierLabel(value, isMac)}
            </span>
            <ChevronDown className="h-4 w-4 text-muted-foreground" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-44 rounded-md">
          <DropdownMenuRadioGroup
            onValueChange={(next) => onChange(next as NavHotkeyModifier)}
            value={value}
          >
            {NAV_HOTKEY_MODIFIERS.map((modifier) => (
              <DropdownMenuRadioItem
                data-testid={`${testId}-${modifier}`}
                key={modifier}
                value={modifier}
              >
                {navHotkeyModifierGlyph(modifier, isMac)}{" "}
                {navHotkeyModifierLabel(modifier, isMac)}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsOptionRow>
  );
}

/**
 * Capture the next letter pressed and bind it.
 *
 * Reads `event.code`, never `event.key`: with Option held macOS rewrites the
 * key to its alternate glyph, so a binding captured from `key` would record
 * "∂" and never match again.
 */
function KeyCaptureRow({
  action,
  code,
  disabled,
  modifier,
  onCapture,
}: {
  action: NavHotkeyAction;
  code: string;
  disabled: boolean;
  modifier: NavHotkeyModifier;
  onCapture: (code: string) => void;
}) {
  const [listening, setListening] = React.useState(false);
  const isMac = isMacPlatform();

  React.useEffect(() => {
    if (!listening) return;

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        setListening(false);
        return;
      }
      if (!isBindableNavHotkeyCode(event.code)) return;
      event.preventDefault();
      onCapture(event.code);
      setListening(false);
    }

    window.addEventListener("keydown", handleKeyDown, { capture: true });
    return () => {
      window.removeEventListener("keydown", handleKeyDown, { capture: true });
    };
  }, [listening, onCapture]);

  return (
    <SettingsOptionRow>
      <div className="min-w-0">
        <p className="text-sm font-medium">
          {NAV_HOTKEY_ACTION_LABELS[action]}
        </p>
        <p
          className="text-sm font-normal text-muted-foreground/70"
          data-settings-subcopy
        >
          {listening
            ? "Press a letter, or Escape to cancel."
            : NAV_HOTKEY_ACTION_DESCRIPTIONS[action]}
        </p>
      </div>
      <Button
        className={TRIGGER_CLASS}
        data-testid={`nav-hotkey-capture-${action}`}
        disabled={disabled}
        onClick={() => setListening((current) => !current)}
        size="sm"
        type="button"
        variant="ghost"
      >
        <span className="font-mono">
          {listening
            ? "Listening…"
            : `${navHotkeyModifierGlyph(modifier, isMac)}${navHotkeyCodeLabel(code)}`}
        </span>
      </Button>
    </SettingsOptionRow>
  );
}

/**
 * The editable half of the shortcuts screen.
 *
 * Conflicts are reported, not prevented: a person who wants ⌘ for both
 * families, or wants the Dashboard on the letter that currently opens search,
 * is allowed to have it — but not by accident, and not without being told what
 * it costs.
 */
export function NavHotkeySettingsCard() {
  const bindings = useNavHotkeyBindings();
  const isMac = isMacPlatform();
  const conflicts = findNavHotkeyConflicts(bindings);
  const registryConflicts = findNavHotkeyRegistryConflicts(bindings, isMac);
  const isDefault =
    bindings.scopeModifier === DEFAULT_NAV_HOTKEY_BINDINGS.scopeModifier &&
    bindings.itemModifier === DEFAULT_NAV_HOTKEY_BINDINGS.itemModifier &&
    NAV_HOTKEY_ACTIONS.every(
      (action) =>
        bindings.codes[action] === DEFAULT_NAV_HOTKEY_BINDINGS.codes[action],
    );

  const scopeGlyph = navHotkeyModifierGlyph(bindings.scopeModifier, isMac);
  const itemGlyph = navHotkeyModifierGlyph(bindings.itemModifier, isMac);

  return (
    <SettingsOptionGroup
      data-testid="settings-nav-hotkeys"
      description="Hold a modifier to see where each key goes; press to jump. These are the only reassignable shortcuts."
      title="Navigation hotkeys"
    >
      <SettingsOptionRow data-testid="nav-hotkeys-enabled-row">
        <div className="min-w-0">
          <label className="text-sm font-medium" htmlFor="nav-hotkeys-switch">
            Enabled
          </label>
          <p
            className="text-sm font-normal text-muted-foreground/70"
            data-settings-subcopy
          >
            Off silences the chords and their badges together.
          </p>
        </div>
        <Switch
          checked={bindings.enabled}
          data-testid="nav-hotkeys-toggle"
          id="nav-hotkeys-switch"
          onCheckedChange={(enabled) =>
            updateNavHotkeyBindings({ ...bindings, enabled })
          }
        />
      </SettingsOptionRow>

      <ModifierSetting
        description="Reaches Dashboard, Projects, Direct messages, and each project by position."
        disabled={!bindings.enabled}
        label="Destination modifier"
        onChange={(scopeModifier) =>
          updateNavHotkeyBindings({ ...bindings, scopeModifier })
        }
        testId="nav-hotkey-scope-modifier"
        value={bindings.scopeModifier}
      />
      <ModifierSetting
        description="Reaches the numbered rows of the project or conversation you are already in."
        disabled={!bindings.enabled}
        label="Row modifier"
        onChange={(itemModifier) =>
          updateNavHotkeyBindings({ ...bindings, itemModifier })
        }
        testId="nav-hotkey-item-modifier"
        value={bindings.itemModifier}
      />

      {NAV_HOTKEY_ACTIONS.map((action) => (
        <KeyCaptureRow
          action={action}
          code={bindings.codes[action]}
          disabled={!bindings.enabled}
          key={action}
          modifier={bindings.scopeModifier}
          onCapture={(code) =>
            updateNavHotkeyBindings({
              ...bindings,
              codes: { ...bindings.codes, [action]: code },
            })
          }
        />
      ))}

      <SettingsOptionRow>
        <div className="min-w-0">
          <p className="text-sm font-medium">Positions</p>
          <p
            className="text-sm font-normal text-muted-foreground/70"
            data-settings-subcopy
          >
            {`${scopeGlyph}1–${scopeGlyph}${NAV_HOTKEY_MAX_POSITIONS} opens the nth project; ${itemGlyph}1–${itemGlyph}${NAV_HOTKEY_MAX_POSITIONS} opens the nth row. Fixed — 0 is left to zoom reset.`}
          </p>
        </div>
        <Button
          data-testid="nav-hotkeys-reset"
          disabled={isDefault}
          onClick={() =>
            updateNavHotkeyBindings({
              ...DEFAULT_NAV_HOTKEY_BINDINGS,
              enabled: bindings.enabled,
            })
          }
          size="sm"
          type="button"
          variant="outline"
        >
          Reset to defaults
        </Button>
      </SettingsOptionRow>

      {conflicts.length > 0 || registryConflicts.length > 0 ? (
        <SettingsOptionRow data-testid="nav-hotkeys-conflicts">
          <ul className="min-w-0 space-y-1 text-sm text-destructive">
            {conflicts.map((conflict) =>
              conflict.kind === "same-modifier" ? (
                <li key="same-modifier">
                  Destinations and rows share a modifier, so the positions
                  answer to the rows only — {scopeGlyph}1 will not open a
                  project.
                </li>
              ) : (
                <li key={`duplicate-${conflict.code}`}>
                  {conflict.actions
                    .map((action) => NAV_HOTKEY_ACTION_LABELS[action])
                    .join(" and ")}{" "}
                  are both on {scopeGlyph}
                  {navHotkeyCodeLabel(conflict.code)}; the first one wins.
                </li>
              ),
            )}
            {registryConflicts.map((conflict) => (
              <li key={`${conflict.source}-${conflict.chord}`}>
                {conflict.chord} would stop working as “
                {conflict.shortcut.label}”.
              </li>
            ))}
          </ul>
        </SettingsOptionRow>
      ) : null}
    </SettingsOptionGroup>
  );
}
