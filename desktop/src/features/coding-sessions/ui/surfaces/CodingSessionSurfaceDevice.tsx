import * as React from "react";
import { useQuery } from "@tanstack/react-query";
import { Smartphone } from "lucide-react";

import { useSurfaceStoredEvents } from "@/features/coding-sessions/hooks/useSurfaceObserver";
import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  type CodingSessionDeviceFold,
  EMPTY_CODING_SESSION_DEVICE_FOLD,
  foldCodingSessionDevice,
} from "@/features/coding-sessions/lib/codingSessionDevice";
import { getCodingSessionProviderStatus } from "@/shared/api/tauriSessionProvider";
import {
  KIND_SESSION_DEVICE_COMMAND,
  KIND_SESSION_DEVICE_RECORD,
  KIND_SURFACE_SNAPSHOT,
} from "@/shared/constants/kinds";

import { CodingSessionDeviceSurface } from "../CodingSessionDeviceSurface";
import type {
  CodingSessionSurfaceAvailability,
  CodingSessionSurfaceBaseCtx,
  CodingSessionSurfaceCtx,
} from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceDefinition } from "./codingSessionSurfaceRegistry";
import { CodingSessionSurfaceDeviceBadge } from "./CodingSessionSurfaceDeviceBadge";
import { CodingSessionSurfacePlaceholder } from "./CodingSessionSurfaceDevicePlaceholder";

export const CODING_SESSION_DEVICE_NO_SESSION_REASON =
  "Open a session to see its device.";

/**
 * Device: open whenever there is a session (SV-34). What the session's
 * provider offers — nothing, an unavailable simulator, no device yet, a live
 * one — is the panel's to say, from the provider's own 44255 records.
 */
export function codingSessionSurfaceDeviceAvailability(
  ctx: Pick<CodingSessionSurfaceCtx, "channelId">,
): CodingSessionSurfaceAvailability {
  return ctx.channelId
    ? { available: true }
    : { available: false, reason: CODING_SESSION_DEVICE_NO_SESSION_REASON };
}

/** What the Device surface reads once per view (`ctx.extensions.device`). */
export type CodingSessionDeviceExtension = {
  fold: CodingSessionDeviceFold;
  /** The generation the surface shows (its cs-target key), or null. */
  targetKey: string | null;
  /** The provider that runs it: the machine the surface names. */
  providerPubkey: string | null;
  /** This computer's provider key; `undefined` while unread. */
  localProviderPubkey: string | null | undefined;
  isLoading: boolean;
  errorMessage: string | null;
};

const DEVICE_KINDS = [
  KIND_SESSION_DEVICE_RECORD,
  KIND_SESSION_DEVICE_COMMAND,
  KIND_SURFACE_SNAPSHOT,
] as const;

/**
 * The Device surface's `readExtension` hook: the channel's stored 44255,
 * 44254 and 44253 records (one bounded read, then live), trusted only from
 * the provider that runs each named generation, folded once and shared by
 * the badge and the panel. React Query only — no module cache.
 */
export function useCodingSessionDeviceExtension(
  ctx: CodingSessionSurfaceBaseCtx,
): CodingSessionDeviceExtension {
  const channelId = ctx.channelId || null;
  const authorities = React.useMemo(() => {
    const map = new Map<string, string>();
    for (const execution of ctx.umbrella.executions) {
      for (const record of [
        ...execution.priorGenerations,
        execution.activeGeneration,
      ]) {
        const provider = record.providerAuthorityPubkey?.trim().toLowerCase();
        if (record.commandTarget && provider) {
          map.set(buildCodingSessionTargetKey(record.commandTarget), provider);
        }
      }
    }
    return map;
  }, [ctx.umbrella]);
  const read = useSurfaceStoredEvents({ channelId, kinds: DEVICE_KINDS });
  const fold = React.useMemo(
    () =>
      channelId
        ? foldCodingSessionDevice(read.events, {
            channelId,
            authorityFor: (key) => authorities.get(key) ?? null,
          })
        : EMPTY_CODING_SESSION_DEVICE_FOLD,
    [authorities, channelId, read.events],
  );
  const status = useQuery({
    queryKey: ["coding-session-provider-status"],
    queryFn: getCodingSessionProviderStatus,
    retry: false,
    staleTime: 60_000,
  });
  const localProviderPubkey =
    status.data === undefined
      ? status.isError
        ? null
        : undefined
      : status.data.providerPubkey?.trim().toLowerCase() || null;

  const focusedTarget =
    ctx.focusedRecord?.commandTarget ??
    ctx.focusedExecution?.activeGeneration.commandTarget ??
    (ctx.umbrella.executions.length === 1
      ? ctx.umbrella.executions[0].activeGeneration.commandTarget
      : null);
  let targetKey = focusedTarget
    ? buildCodingSessionTargetKey(focusedTarget)
    : null;
  if (targetKey === null) {
    // No one generation is focused: show the newest open device, if any.
    let newest: { createdAt: number; targetKey: string } | null = null;
    for (const slot of fold.slots.values()) {
      if (
        slot.state === "open" &&
        (!newest || slot.createdAt > newest.createdAt)
      ) {
        newest = slot;
      }
    }
    targetKey = newest?.targetKey ?? null;
  }
  const providerPubkey = targetKey
    ? (authorities.get(targetKey) ?? null)
    : null;
  return React.useMemo(
    () => ({
      fold,
      targetKey,
      providerPubkey,
      localProviderPubkey,
      isLoading: read.isLoading,
      errorMessage: read.errorMessage,
    }),
    [
      fold,
      localProviderPubkey,
      providerPubkey,
      read.errorMessage,
      read.isLoading,
      targetKey,
    ],
  );
}

/** The extension from `ctx`, or `null` where the surface's hook did not run. */
export function codingSessionDeviceExtension(
  ctx: Pick<CodingSessionSurfaceCtx, "extensions">,
): CodingSessionDeviceExtension | null {
  const value = ctx.extensions.device;
  return value && typeof value === "object"
    ? (value as CodingSessionDeviceExtension)
    : null;
}

/** The Device panel, or the reason it cannot open (no session). */
export function CodingSessionSurfaceDevicePanel({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  const availability = codingSessionSurfaceDeviceAvailability(ctx);
  const extension = codingSessionDeviceExtension(ctx);
  if (!availability.available || !extension) {
    return (
      <CodingSessionSurfacePlaceholder
        icon={Smartphone}
        id="device"
        label="Device"
        reason={
          availability.available
            ? "The Device surface did not read this session."
            : availability.reason
        }
      />
    );
  }
  return <CodingSessionDeviceSurface ctx={ctx} extension={extension} />;
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
  Badge: CodingSessionSurfaceDeviceBadge,
  Panel: CodingSessionSurfaceDevicePanel,
  readExtension: useCodingSessionDeviceExtension,
};
