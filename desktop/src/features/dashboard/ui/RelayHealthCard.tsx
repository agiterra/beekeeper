import { Server } from "lucide-react";

import { useRelaySystemHealthQuery } from "@/features/dashboard/hooks";
import {
  classifyRelaySystemHealthError,
  relayContainerSentence,
  relayCpuSentence,
  relayDiskSentence,
  relayLoadSentence,
  relayMemorySentence,
  relaySampleAgeSentence,
  relaySystemHealthFailureSentence,
} from "@/features/dashboard/lib/relaySystemHealth";
import { useMyRelayMembershipQuery } from "@/features/community-members/hooks";
import { Card } from "@/shared/ui/card";
import { Skeleton } from "@/shared/ui/skeleton";

/**
 * What the relay says about the machine it runs on: CPU, memory and disk,
 * sampled by the relay and re-read every ten seconds.
 *
 * Shown to the community's stewards. A plain member never sees the card,
 * and an identity the relay refuses (403) sees nothing either — the relay's
 * rule decides, not this component's guess. Every other failure is printed
 * in words, and every number names its subject: the whole machine, the
 * relay process, or a container limit.
 */
export function RelayHealthCard() {
  const membershipQuery = useMyRelayMembershipQuery();
  const query = useRelaySystemHealthQuery();
  const role = membershipQuery.data?.role;
  if (membershipQuery.isPending || role === "member") return null;
  const failure = query.isError
    ? classifyRelaySystemHealthError(query.error)
    : null;
  if (failure === "forbidden") return null;
  const health = query.data;
  const load = health ? relayLoadSentence(health.cpu.loadAverage) : null;
  const container = health ? relayContainerSentence(health.memory) : null;
  return (
    <Card
      className="flex flex-col gap-3 p-4"
      data-testid="dashboard-card-relay-health"
    >
      <div className="flex items-center gap-2 text-sm font-semibold text-foreground">
        <span className="text-muted-foreground">
          <Server className="size-4" />
        </span>
        Relay machine
      </div>
      <div className="flex min-h-10 flex-col gap-1 text-sm text-muted-foreground">
        {query.isPending ? (
          <Skeleton className="h-4 w-48" />
        ) : failure !== null ? (
          <p data-testid="dashboard-card-relay-health-note">
            {relaySystemHealthFailureSentence(failure)}
          </p>
        ) : health ? (
          <>
            <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
              <dt className="font-medium text-foreground">CPU</dt>
              <dd data-testid="dashboard-card-relay-health-cpu">
                {relayCpuSentence(health.cpu)}
                {load ? <span className="text-2xs"> · {load}</span> : null}
              </dd>
              <dt className="font-medium text-foreground">Memory</dt>
              <dd data-testid="dashboard-card-relay-health-memory">
                {relayMemorySentence(health.memory)}
                {container ? (
                  <span className="text-2xs"> · {container}</span>
                ) : null}
              </dd>
              <dt className="font-medium text-foreground">Disk</dt>
              <dd className="flex flex-col gap-0.5">
                {health.disks.length === 0 ? (
                  <span data-testid="dashboard-card-relay-health-disk-none">
                    The relay could not measure a filesystem.
                  </span>
                ) : (
                  health.disks.map((disk, index) => (
                    <span
                      data-testid={`dashboard-card-relay-health-disk-${index}`}
                      key={disk.paths.join("|")}
                    >
                      {relayDiskSentence(disk)}
                    </span>
                  ))
                )}
              </dd>
            </dl>
            <p
              className="text-2xs"
              data-testid="dashboard-card-relay-health-age"
            >
              {health.host.name ? `${health.host.name} · ` : ""}
              {relaySampleAgeSentence(
                health.ageSeconds,
                health.intervalSeconds,
              )}
            </p>
          </>
        ) : null}
      </div>
    </Card>
  );
}
