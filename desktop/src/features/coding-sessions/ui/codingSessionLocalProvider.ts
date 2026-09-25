import {
  ensureCodingSessionProviderRunning,
  getCodingSessionProviderStatus,
  provisionCodingSessionProvider,
  type CodingSessionProviderStatus,
} from "@/shared/api/tauriSessionProvider";
import type { NewCodingSessionHostPhase } from "../lib/newCodingSessionModel";

/**
 * Make sure this computer's provider exists and is running.
 *
 * Only for the local provider. A target signed by some other machine's
 * provider is that machine's business — provisioning here would mint a second
 * identity for no reason.
 */
export async function ensureLocalProvider(input: {
  isLocalProvider: (pubkey: string) => boolean;
  signerPubkey: string;
  setHostPhase: (phase: NewCodingSessionHostPhase) => void;
  onTrustMutated: () => void;
}): Promise<CodingSessionProviderStatus | null> {
  const status = await getCodingSessionProviderStatus().catch(() => null);
  if (status && !status.provisioned) {
    input.setHostPhase("provisioning");
    const provisioned = await provisionCodingSessionProvider();
    input.onTrustMutated();
    return provisioned;
  }
  if (!status || !input.isLocalProvider(input.signerPubkey)) {
    return status;
  }
  if (!status.running) {
    input.setHostPhase("starting");
    // Starting also re-seeds trust (`supervisor.rs` re-asserts the entry on
    // every start), so readers must refetch here too.
    const running = await ensureCodingSessionProviderRunning();
    input.onTrustMutated();
    return running;
  }
  return status;
}
