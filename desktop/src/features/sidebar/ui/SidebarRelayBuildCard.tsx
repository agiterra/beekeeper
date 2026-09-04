import { GitCommitHorizontal } from "lucide-react";

import type { RelayBuildDrift } from "@/features/settings/relayBuildDrift";
import { relayBuildDriftNotice } from "@/features/settings/relayBuildDrift";
import { SidebarCompactActionCard } from "@/shared/ui/sidebar-action-card";

type SidebarRelayBuildCardProps = {
  drift: RelayBuildDrift;
  /**
   * Whether an app update is actually installable right now. Changes the
   * copy rather than the visibility: the relay routinely runs ahead of the
   * newest release, and telling someone they are behind while offering
   * nothing to install would be a prompt that lies by implication.
   */
  updateAvailable: boolean;
  onSelectSettings: () => void;
  onDismiss: () => void;
  className?: string;
};

/**
 * "Your app is behind the relay", when — and only when — that is both true
 * and sayable.
 *
 * All of the deciding lives in `relayBuildDriftNotice`; this renders whatever
 * it returns and nothing when it returns `null`. That split is deliberate and
 * matches `sidebarUpdateCardVisibility`: the honesty rules are unit-tested
 * without React, and no state gets a number invented for it here.
 */
export function SidebarRelayBuildCard({
  drift,
  updateAvailable,
  onSelectSettings,
  onDismiss,
  className,
}: SidebarRelayBuildCardProps) {
  const notice = relayBuildDriftNotice(drift, updateAvailable);
  if (!notice) return null;

  return (
    <SidebarCompactActionCard
      className={className}
      description={notice.description}
      dismissLabel="Dismiss build notice"
      icon={<GitCommitHorizontal aria-hidden="true" className="h-5 w-5" />}
      onAction={onSelectSettings}
      actionAriaLabel="Open software update settings"
      onDismiss={onDismiss}
      // `status`, not `alert`: being behind is worth saying, but it is not an
      // outage and must not interrupt a screen reader mid-task the way the
      // relay-connection card legitimately does.
      role="status"
      testId="sidebar-relay-build-card"
      title={notice.title}
      tone="neutral"
    />
  );
}
