import { Checkbox } from "@/shared/ui/checkbox";

/**
 * "Full access to this computer" at launch (ledger 303).
 *
 * Off by default. When ticked, the session's agent starts outside the
 * project boundary from its first turn — the grant is written for the
 * create's command id and the provider moves it to the session it creates.
 * Offered only for this computer's own provider: the grant is this
 * computer's to give.
 */
export function NewCodingSessionFullAccessField({
  checked,
  disabled = false,
  onCheckedChange,
}: {
  checked: boolean;
  disabled?: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <div className="flex flex-col gap-1">
      <label
        className="flex items-center gap-2 text-xs font-medium text-muted-foreground"
        htmlFor="coding-session-full-access-toggle"
      >
        <Checkbox
          checked={checked}
          data-testid="coding-session-full-access-toggle"
          disabled={disabled}
          id="coding-session-full-access-toggle"
          onCheckedChange={(next) => onCheckedChange(next === true)}
        />
        Full access to this computer
      </label>
      <p className="pl-6 text-2xs text-muted-foreground">
        {checked
          ? "The agent runs outside the sandbox: it can install tools and reach anything your account can, including other projects. You can turn it off from the session's menu."
          : "Off: the agent stays inside this project's sandbox. Tick to let it install tools or work outside the project."}
      </p>
    </div>
  );
}
