---
# REFUSED: unknown-frontmatter-key — "owner" is not in beekeeper-plan/v1.
schema: beekeeper-plan/v1
id: kettle-cli
status: in-force
title: Build and land kettle
owner: brian
code_repository: pivot-test
delivery_ref: refs/heads/main
criteria:
  - id: cli-behaviour
    accept: add appends and list numbers oldest-first from 1.
    proof: {kind: review}
retired_criteria: []
---
Unknown keys are refused, never ignored: a key a reader drops is a promise
somebody believed they had made.
