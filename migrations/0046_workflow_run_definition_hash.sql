-- A run is bound to the action definition it was created from
-- (docs/UNIFIED_WORK_PLAN.md § 4 W4; ledger 193).
--
-- An operator's approval of a host step is consent for *one* definition to
-- run a command on their machine. Before this column a run carried no record
-- of which definition it was started from, so the approval matched on run id
-- and step id alone and every resume re-read the *current* `workflows` row:
-- editing the action between the request and the grant substituted the new
-- command under the old consent. The hash is written in the same INSERT as
-- the run, read from the `workflows` row the run references, so no code path
-- can create an unbound run.
--
-- NULL is not "no opinion": it means the run predates this column. Such runs
-- stay readable and stop with `definition_unknown` if anything tries to
-- resume them, because nothing can say what their operator agreed to.
-- Additive migration: previously applied files must not change checksum.

ALTER TABLE workflow_runs
    ADD COLUMN definition_hash BYTEA;
