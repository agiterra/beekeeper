import { Globe } from "lucide-react";

import type {
  CodingSessionSurfaceAvailability,
  CodingSessionSurfaceCtx,
} from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceDefinition } from "./codingSessionSurfaceRegistry";
import { CodingSessionSurfacePlaceholder } from "./CodingSessionSurfaceDevicePlaceholder";

/** Browser: when it can open, or the sentence why not (§3, SV-23). */
export function codingSessionSurfaceBrowserAvailability(
  _ctx: CodingSessionSurfaceCtx,
): CodingSessionSurfaceAvailability {
  return { available: false, reason: "Arrives with live preview." };
}

/**
 * Browser is listed, dimmed, so a person can see it is coming and why it is
 * not here yet. Its panel is the same reason, never an empty frame that
 * could pass for a broken one.
 */
export function CodingSessionSurfaceBrowserPanel({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  const availability = codingSessionSurfaceBrowserAvailability(ctx);
  return (
    <CodingSessionSurfacePlaceholder
      dimmed
      icon={Globe}
      id="browser"
      label="Browser"
      reason={availability.available ? "" : availability.reason}
    />
  );
}

export const codingSessionSurfaceBrowser: CodingSessionSurfaceDefinition = {
  id: "browser",
  label: "Browser",
  icon: Globe,
  shortcut: "B",
  order: 90,
  placement: "right",
  lenses: ["conversation", "mission"],
  availability: codingSessionSurfaceBrowserAvailability,
  Panel: CodingSessionSurfaceBrowserPanel,
};
