import { Smartphone } from "lucide-react";

import type {
  CodingSessionSurfaceAvailability,
  CodingSessionSurfaceCtx,
} from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceDefinition } from "./codingSessionSurfaceRegistry";
import { CodingSessionSurfacePlaceholder } from "./CodingSessionSurfaceDevicePlaceholder";

/** Device: when it can open, or the sentence why not (§3, SV-23). */
export function codingSessionSurfaceDeviceAvailability(
  _ctx: CodingSessionSurfaceCtx,
): CodingSessionSurfaceAvailability {
  return { available: false, reason: "Arrives with device support." };
}

/**
 * Device is listed, dimmed, so a person can see it is coming and why it is
 * not here yet. Its panel is the same reason, never an empty frame that
 * could pass for a broken one.
 */
export function CodingSessionSurfaceDevicePanel({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  const availability = codingSessionSurfaceDeviceAvailability(ctx);
  return (
    <CodingSessionSurfacePlaceholder
      dimmed
      icon={Smartphone}
      id="device"
      label="Device"
      reason={availability.available ? "" : availability.reason}
    />
  );
}

export const codingSessionSurfaceDevice: CodingSessionSurfaceDefinition = {
  id: "device",
  label: "Device",
  icon: Smartphone,
  shortcut: "M",
  order: 100,
  placement: "right",
  lenses: ["conversation", "mission"],
  availability: codingSessionSurfaceDeviceAvailability,
  Panel: CodingSessionSurfaceDevicePanel,
};
