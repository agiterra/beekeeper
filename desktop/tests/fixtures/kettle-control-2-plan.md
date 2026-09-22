---
schema: beekeeper-plan/v1
id: kettle-cli
status: in-force
title: Build and land kettle
code_repository: kettle-control-2
delivery_ref: refs/heads/main
criteria:
  - id: cli-behaviour
    accept: >-
      Dependency-free Python 3 package kettle runs as python3 -m kettle.
      add <text> appends; list numbers oldest-first from 1;
      done <n> marks that item done and list shows it with [x].
    proof: {kind: review}
  - id: shared-storage-and-parser
    accept: >-
      All commands use exactly one store module and one command parser.
      Data lives in ~/.kettle.json, overridden by KETTLE_FILE for tests.
    proof: {kind: review}
  - id: verified-landed-revision
    accept: >-
      python3 -m unittest discover -s tests passes from the code root.
      The agents-repository verify action is published and runs that command
      green on the exact code commit observed at the delivery destination.
    proof: {kind: action, name: verify, step: verify}
  - id: usage-documentation
    accept: README has a Usage section showing add, list and done.
    proof: {kind: review}
  - id: delivered-main
    accept: One coherent history on main contains the accepted implementation.
    proof: {kind: git-ref}
retired_criteria: []
---
# Kettle

Build the CLI under kettle/ with tests under tests/.

Suggested slices: A is store/add/list/tests; B is done/README/tests and uses
A's store; C is the verify action in this agents repository's actions.yml:
a manual action named verify with one step verify that runs
python3 -m unittest discover -s tests, with checkout: required so a run
always names the commit it tests. The lead may combine A and B and must
prevent a second store or parser.

The team may decide layout, output format and numbering after deletion
without asking Brian; record reasons in Pulse. This does not add a delete
command.
