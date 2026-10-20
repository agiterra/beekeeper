/**
 * A provider write that landed only in part. A provider whose one logical
 * change is several records (native status is `item.done` then
 * `item.status`) has no transaction to hide behind; when a later record is
 * refused, this carries one sentence saying what was saved and what the
 * issue reads now, so the surface reports a partial change as partial — not
 * as a failure that saved nothing, and not as a success.
 */
export class PlanningPartialWriteError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PlanningPartialWriteError";
  }
}
