---
# REFUSED: duplicate-criterion-id — "cli-behaviour" appears twice.
schema: beekeeper-plan/v1
id: kettle-cli
status: in-force
title: Build and land kettle
code_repository: pivot-test
delivery_ref: refs/heads/main
criteria:
  - id: cli-behaviour
    accept: add appends and list numbers oldest-first from 1.
    proof: {kind: review}
  - id: cli-behaviour
    accept: done <n> marks that item done and list shows it with [x].
    proof: {kind: review}
retired_criteria: []
---
Two obligations cannot share one id: an evidence binding names a criterion id,
and a duplicate makes the binding ambiguous.
