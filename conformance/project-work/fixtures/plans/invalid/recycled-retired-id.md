---
# REFUSED: recycled-retired-id — "usage-documentation" is retired and active.
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
  - id: usage-documentation
    accept: README documents the new flags instead of the old ones.
    proof: {kind: review}
retired_criteria: [usage-documentation]
---
A retired id is never recycled within a plan. Reusing it would let evidence
bound to the retired obligation read as coverage of the new one.
