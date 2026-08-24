import { CodingSessionCapacityCard } from "@/features/coding-sessions/ui/CodingSessionCapacityCard";

import { SettingsOptionGroup } from "./SettingsOptionGroup";

/** Settings for the coding sessions this computer runs. */
export function CodingSessionsSettingsPanel() {
  return (
    <SettingsOptionGroup
      data-testid="settings-coding-sessions"
      description="How many coding sessions this computer will run at once. Each live session is an agent process here — the limit is Bee Keeper's own, not your model provider's."
      title="Sessions"
    >
      <CodingSessionCapacityCard />
    </SettingsOptionGroup>
  );
}
