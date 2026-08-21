# Buzz Domain Context

This file names domain concepts whose distinctions are easy to erase in code.
It records product meaning, not implementation structure.

## Coding sessions and provider reachability

A **coding session** is the durable collaboration umbrella represented by its
session genesis and later shared lifecycle facts. It may span several provider
executions and reconnect generations. A provider process stopping, an actor
exiting, or a generation disconnecting does not by itself close the coding
session.

An **execution generation** is one exact provider-owned runtime identity. It is
named by `(driver, instanceId, sessionId, generation)` and is minted by an
accepted lifecycle command plus its successful provider receipt. Generation 1
comes from `session.create`; each later generation comes from `session.resume`.

The **provider authority** for an execution generation is the
`providerAuthorityPubkey` selected by the accepted lifecycle command and bound
to that exact target by the command's successful provider-signed receipt.
Metadata authorship never establishes provider authority.

A **session lease** is a short-lived, provider-signed assertion that the
provider still owns a live actor for one exact execution generation. It proves
provider reachability only. It does not prove human attention, active token
generation, network continuity for the lease's whole lifetime, an intention to
edit a particular file, or a coordination conflict.

Generation reachability has three states:

- `provider_reachable`: a current, authority-valid `live` lease exists and the
  generation is not durably terminal.
- `unverified`: there is no current valid live lease. This is unknown liveness,
  not proof that the provider or human is gone.
- `terminal`: durable generation metadata says the execution stopped or
  disconnected. Terminal generation state outranks a contradictory live lease.

The coding-session umbrella independently has two lifecycle states:

- `open`: no effective durable closure says otherwise.
- `closed`: the durable closure fold says the collaboration session is closed.

Project Pulse combines these axes without collapsing them:

- `provider_reachable`: the umbrella is open and at least one current
  generation is provider-reachable.
- `open_unverified`: the umbrella is open and no current generation has a valid
  live lease, including when the latest generation is terminal.
- `closed`: the durable umbrella is closed, regardless of surviving leases.

An open unverified session remains coordination-relevant. Absence of a lease is
never evidence that it is safe to proceed. Automatic `wait` advice requires a
valid live lease plus concrete conflict or dependency evidence; overlapping
open-but-unverified work supports `consult`, not `wait` or `proceed`.
