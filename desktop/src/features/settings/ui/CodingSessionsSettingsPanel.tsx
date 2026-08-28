import { CodingSessionCapacityCard } from "@/features/coding-sessions/ui/CodingSessionCapacityCard";
import { CodingSessionNamingCard } from "@/features/coding-sessions/ui/CodingSessionNamingCard";

import {
  SettingsOptionGroup,
  SettingsOptionGroupList,
} from "./SettingsOptionGroup";

/** Settings for the coding sessions this computer runs. */
export function CodingSessionsSettingsPanel() {
  return (
    <SettingsOptionGroupList>
      <SettingsOptionGroup
        data-testid="settings-coding-sessions"
        description="How many coding sessions this computer will run at once, how long one turn may go silent, and how many turns a team session may take before its agents are refused. Each live session is an agent process here — these limits are Beekeeper's own, not your model provider's."
        title="Sessions"
      >
        <CodingSessionCapacityCard />
      </SettingsOptionGroup>
      <SettingsOptionGroup
        data-testid="settings-coding-session-naming"
        description="Name a new session from its first message. Off unless you choose a model — the message leaves this computer only for the endpoint you name."
        title="Session names"
      >
        <CodingSessionNamingCard />
      </SettingsOptionGroup>
    </SettingsOptionGroupList>
  );
}
