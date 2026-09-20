---
# REFUSED: empty-accept — "shared-storage-and-parser" has no acceptance text.
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
  - id: shared-storage-and-parser
    accept: "   "
    proof: {kind: review}
retired_criteria: []
---
A criterion with no acceptance text cannot be judged, so it cannot be covered
and must not be adopted.
