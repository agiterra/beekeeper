import { RecoveryScreen } from "./RecoveryScreen";

export function RelaunchRequiredScreen() {
  return (
    <RecoveryScreen
      testId="relaunch-required"
      title="Restart Bee Keeper to finish recovery"
      body="Your identity was updated. Bee Keeper needs to restart so syncing and agents run under it."
    />
  );
}
