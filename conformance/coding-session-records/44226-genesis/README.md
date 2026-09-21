# kind:44226 — coding-session genesis

The smallest record the fork defines: the cryptographic origin of one umbrella
session. The signer is the founder; the payload never restates that.

## Closed key set — exactly two forms

| Form | Keys |
| --- | --- |
| Fresh founding | `sessionRef, v` |
| Legacy adoption | `sessionRef, v, adopts` where `adopts` is exactly `{createEventId, receiptEventId}` |

Nothing between, nothing beyond: not a third top-level key, not `adopts`
present but `null`, not an `adopts` object with a differently-named or
additional field. `v` is exactly `1`. `sessionRef` is a canonical lowercase
hyphenated UUID; both adoption references are lowercase 64-hex event ids.

Unlike the lifecycle command there is no *historical* form to accommodate —
genesis shipped with the authority chain — so these two shapes are the whole
contract.

## What the vectors do not cover

The relay's verification of an adoption claim (fetching the named create and
receipt, checking the receipt joins the create, the signer matches, and both
sit in this channel) is behaviour, not a key set. The `csg-session` tag is
re-derived from the payload at ingest and is never a selector; the desktop and
mobile tests here build it that way, and a consumer that queries by it is
already wrong for reasons no fixture can catch.
