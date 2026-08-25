import { CodingSessionCapacityCard } from "@/features/coding-sessions/ui/CodingSessionCapacityCard";
import { CodingSessionNamingCard } from "@/features/coding-sessions/ui/CodingSessionNamingCard";
import { CodingSessionPaddingCard } from "@/features/coding-sessions/ui/CodingSessionPaddingCard";

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
        description="How many coding sessions this computer will run at once. Each live session is an agent process here — the limit is Bee Keeper's own, not your model provider's."
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
      <SettingsOptionGroup
        data-testid="settings-coding-session-padding-group"
        description="How much of the window a session spends on its left and right margins. Applies to the transcript and the composer together, on this computer only."
        title="Session padding"
      >
        <CodingSessionPaddingCard />
      </SettingsOptionGroup>
    </SettingsOptionGroupList>
  );
}
