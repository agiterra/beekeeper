import { useQuery } from "@tanstack/react-query";

import {
  type SessionPreviewServer,
  sessionPreviewServers,
} from "@/shared/api/tauriSessionPreview";

/** Rust polls lsof every 3 s while asked; asking at the same pace keeps it on. */
export const SESSION_PREVIEW_SERVERS_POLL_MS = 3_000;

/**
 * This computer's listening local servers, read only while the empty state is
 * on screen (`enabled`). Machine-scoped, not community data: the query key
 * names no community and nothing about it reaches the relay.
 */
export function useSessionPreviewServers(enabled: boolean): {
  servers: SessionPreviewServer[];
  loading: boolean;
  error: string | null;
} {
  const query = useQuery({
    queryKey: ["session-preview-servers"],
    queryFn: sessionPreviewServers,
    enabled,
    refetchInterval: enabled ? SESSION_PREVIEW_SERVERS_POLL_MS : false,
    retry: false,
  });
  return {
    servers: query.data ?? [],
    loading: query.isLoading,
    error: query.isError
      ? query.error instanceof Error
        ? query.error.message
        : "This computer's server list could not be read."
      : null,
  };
}
