import * as React from "react";

import {
  registerHotkeyTargets,
  useActiveHotkeyScope,
} from "@/features/hotkeys/lib/hotkeyTargetRegistry";
import { NAV_HOTKEY_MAX_POSITIONS } from "@/features/hotkeys/lib/navHotkeyBindings";
import type { Channel } from "@/shared/api/types";

/**
 * Number the direct-message rows for the item hotkey while a DM is on screen.
 *
 * Registers in the exact order the section renders — which is the sort mode
 * the person chose, not a second ordering invented here — and returns the
 * position map the rows badge themselves with, so the number and the chord are
 * the same fact.
 *
 * Lives outside `AppSidebar` because that file sits close to the repository's
 * file-size ceiling; the call site there is one line.
 */
export function useDmHotkeyTargets(
  directMessages: readonly Channel[],
  onSelectChannel: (channelId: string) => void,
): ReadonlyMap<string, number> {
  const isActiveScope = useActiveHotkeyScope() === "dms";

  const reachable = React.useMemo(
    () => directMessages.slice(0, NAV_HOTKEY_MAX_POSITIONS),
    [directMessages],
  );

  const indexByChannelId = React.useMemo(() => {
    const map = new Map<string, number>();
    if (!isActiveScope) return map;
    reachable.forEach((channel, index) => {
      map.set(channel.id, index);
    });
    return map;
  }, [isActiveScope, reachable]);

  const selectChannel = React.useEffectEvent((channelId: string) => {
    onSelectChannel(channelId);
  });

  React.useEffect(() => {
    if (!isActiveScope) return;
    return registerHotkeyTargets(
      "dms",
      reachable.map((channel) => ({
        key: channel.id,
        label: channel.name,
        activate: () => selectChannel(channel.id),
      })),
    );
  }, [isActiveScope, reachable]);

  return indexByChannelId;
}
