import type { CodingSessionCostBasis } from "@/features/agents/ui/agentSessionTypes";

/**
 * The wire's `costBasis`, with the pre-272(d) spellings read as what they
 * always meant: `billed` was the adapter's own figure (an estimate, never an
 * invoice) and `estimated` the price table's. Anything else is unattributed.
 */
export function readCostBasis(value: unknown): CodingSessionCostBasis | null {
  switch (value) {
    case "adapter_estimate":
    case "billed":
      return "adapter_estimate";
    case "table_estimate":
    case "estimated":
      return "table_estimate";
    default:
      return null;
  }
}
