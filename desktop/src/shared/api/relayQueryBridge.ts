/**
 * TS binding for the `query_relay_filters` Tauri command (Lane D2,
 * `desktop/src-tauri/src/commands/relay_query.rs`).
 *
 * Lives beside the relay transport rather than in `tauri.ts` because that
 * file is over the 1000-line ceiling and the differential file-size ratchet
 * forbids it growing by a single line. It still goes through `invokeTauri`,
 * so a relay 429 (`relay rate-limited:`) arms the shared rate-limit gate the
 * same way every other bridge call does.
 */
import { invokeTauri } from "@/shared/api/tauri";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";

/**
 * `POST /query` against the active relay with an array of NIP-01 filters:
 * one HTTP call (one unit of the relay's separate per-minute API budget)
 * regardless of filter count, capped only by the relay's 128 aggregate `#h`
 * values per request. Returns the union of every filter's results, unsorted
 * and undeduplicated — `relayQueryCoalescer.ts` demultiplexes per filter.
 * The Rust side waits out its own admission gate before sending.
 */
export async function queryRelayFilters(
  filters: RelaySubscriptionFilter[],
): Promise<RelayEvent[]> {
  return invokeTauri<RelayEvent[]>("query_relay_filters", { filters });
}
