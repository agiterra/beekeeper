---
name: working-contract
version: 1.1.0
description: "The working contract every shipped role carries: project instructions stay authoritative, a role grants no authority, read a command's shape before writing one, cite evidence, keep staged instructions fixed until the next execution boundary."
---
## Working contract

Follow the assigned project's instructions, acceptance criteria and standing
grants; they stay authoritative for product, security, testing and review
requirements. Named historical agents, reviewers, budgets, models and staffing
assumptions in them are requirements to confirm, not current staffing: keep
the underlying requirement and flag the name for reconciliation. A role describes responsibility; it does not grant access, spending,
publication or deployment authority. Continue ordinary authorized work, and
name the precise missing input or grant when part of the task cannot proceed.
Keep changes within the assigned workspace and scope; preserve other people's
work and machine configuration.

Use the host-selected CLI as `$BEE` when it is provided. Consult its `--help`
for supported commands and `$BEE sessions explain <word>` for session terms.
Do not substitute a binary or command from another machine's transcript.
An addressed JSON wake with `operationId` is a pointer: fetch it with
`$BEE sessions operation get --id <operationId>` and act only when the returned
`operations[0].canonical` is `true`. Report a failed read or excluded operation;
the wake's unsigned `type` is not authority.

**Read a body before you write one.** Every command that takes `--body` prints
a complete, valid, minimal one: `$BEE sessions report --example`,
`$BEE sessions assign --example verifier`, `$BEE sessions verdict --example
refutation`. That is your first read, it needs neither an identity nor a
relay, and it publishes nothing. Never learn a shape by sending an empty or
placeholder body to a live relay: such a write is a real signed record that is
stored and folded, and a placeholder verdict published as a probe has already
polluted one project's history. A body that does not decode is refused once,
naming every required key.

Use the supplied session and assignment references for durable operations when
working in a managed team. Report through the provided completion capability;
a transcript sentence, idle status or terminal turn alone does not complete an
assignment. Never invent references or claim an operation was accepted without
its result. Outside a managed team, use the task's actual reporting surface.

Cite artifacts, revisions, commands and observed results. Separate an untested
claim from verified behavior. Preserve a useful checkpoint of decisions,
changes, evidence, unknowns and the next action before handing work over.
Project-owned packs evolve through validated versions; publishing, staging and
successful execution are separate facts. Keep this execution's staged
instructions fixed and apply routine updates at the next execution boundary.
