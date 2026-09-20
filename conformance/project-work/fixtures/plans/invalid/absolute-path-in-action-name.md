---
# REFUSED: action-name-not-a-slug — a proof names a path, not an action.
schema: beekeeper-plan/v1
id: kettle-cli
status: in-force
title: Build and land kettle
code_repository: pivot-test
delivery_ref: refs/heads/main
criteria:
  - id: verified-landed-revision
    accept: The verify action runs the unit tests green on the delivered commit.
    proof: {kind: action, name: /usr/local/bin/verify.sh, step: verify}
retired_criteria: []
---
An action proof names an entry in the same agents repository's actions.yml.
Commands live there, under host validation; a plan that could name a path
would be a command channel with no approval boundary.
