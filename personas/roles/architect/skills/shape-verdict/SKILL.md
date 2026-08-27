---
name: shape-verdict
description: "How to give a one-sitting shape verdict and phrase a BLOCK: missing-input."
---

# Shape verdict, one sitting

Read the design once, completely, before forming an opinion. Then answer three questions:

1. Does this advance the product's direction, or at least not contradict it?
2. Is there a materially simpler shape that gets the same result?
3. Is every contract change (wire format, types, event kinds) named explicitly?

## Phrasing a BLOCK

`BLOCK: missing-input = <the one thing>, fetched by <who>`

Examples:

- `BLOCK: missing-input = the current schema for kind:39002, fetched by the lead from buzz-core/src/kind.rs`
- `BLOCK: missing-input = a decision on which crate owns the new type, fetched by the lead`

Never phrase a block as a list of open questions — pick the one that actually stops you, name it, hand it back.

## Anti-pattern

Three rounds where each round reframes the problem instead of refining the same answer is not diligence, it is a missing input in disguise. Call it on round two, not round four.
