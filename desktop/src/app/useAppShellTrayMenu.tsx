import type { Channel } from "@/shared/api/types";
import { isMacPlatform } from "@/shared/lib/platform";

import { useAgentActivityPush } from "@/app/useAgentActivityPush";

/**
 * Tells the agent host which managed agents are working, so the menu bar app
 * can list them.
 *
 * This used to drive a native tray menu from inside this app, which is why it
 * sits outside `AppShell`'s render cycle: the hook re-rendered once a second
 * to keep an elapsed label moving. It no longer ticks — the host is sent
 * absolute start times and the menu bar app formats them — but the boundary
 * stays, because the turn state it reads changes often and nothing in
 * `AppShell` needs to re-render when it does.
 */
export function AppShellTrayMenu({ channels }: { channels: Channel[] }) {
  if (!isMacPlatform()) return null;
  return <MacAppShellTrayMenu channels={channels} />;
}

function MacAppShellTrayMenu({ channels }: { channels: Channel[] }): null {
  useAgentActivityPush({ channels });
  return null;
}
