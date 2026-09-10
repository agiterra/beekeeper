type ReadinessFilter = {
  kinds?: readonly number[];
  "#h"?: readonly string[];
  "#e"?: readonly string[];
  "#p"?: readonly string[];
  authors?: readonly string[];
};

/** Match the actual channel consumer, never an unrelated global live REQ. */
export function mockLiveReadinessMatches(
  filters: readonly ReadinessFilter[],
  channelId: string,
  kind: number | undefined,
  activeChannel: boolean,
): boolean {
  return filters.some((filter) => {
    if (!filter.kinds?.includes(kind ?? 9)) return false;
    // Read-state documents are per identity, not per channel. Only an
    // explicit request for that kind may use its unscoped consumer.
    if (kind === 30078 && !filter["#h"]?.length) return true;
    if (!filter["#h"]?.includes(channelId)) return false;
    // Huddle lifecycle readiness names its lifecycle consumer, not the
    // broad sidebar feed which also receives these kinds for activity.
    if (
      kind !== undefined &&
      kind >= 48100 &&
      kind <= 48103 &&
      filter.kinds.some((value) => value < 48100 || value > 48103)
    )
      return false;
    // A scoped subset cannot promise delivery for arbitrary test messages.
    if (
      (kind === undefined || kind === 9 || kind === 40002) &&
      (filter["#e"]?.length || filter["#p"]?.length || filter.authors?.length)
    ) {
      return false;
    }
    // Only the active channel window consumes thread summaries and appends
    // timeline rows. An inactive channel instead uses its bundled live feed.
    return (
      kind !== undefined ||
      !activeChannel ||
      (filter["#h"].length === 1 && filter.kinds.includes(39005))
    );
  });
}
