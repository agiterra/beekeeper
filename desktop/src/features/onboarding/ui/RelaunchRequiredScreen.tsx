import { RecoveryScreen } from "./RecoveryScreen";

export function RelaunchRequiredScreen() {
  return (
    <RecoveryScreen
      testId="relaunch-required"
      title="Restart Beekeeper to finish recovery"
      body="Your identity was updated. Beekeeper needs to restart so syncing and agents run under it."
    />
  );
}
