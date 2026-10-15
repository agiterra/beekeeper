# External conversation participants

This document covers a process that takes part in Beekeeper conversations
while holding its own keys outside Beekeeper custody: a connector to another
tool (the T3 Code bridge), or an agent hosted elsewhere. It uses only the
relay's public contracts. It complements the host-step contract in
[headless-extensions.md](headless-extensions.md#external-participation-and-proof).
It is not a provider: an external participant publishes no provider catalog,
session metadata or receipts.

## Identity and owner attestation

- **Its own key.** The participant generates and keeps its own key; Beekeeper
  never holds it.
- **Owner attestation.** The owner attests that key with NIP-OA, signing with
  their own key from bee's existing custody. The owner key is never passed as
  an argument:

  ```sh
  bee agents attest <PUBKEY> --expires-at <unix> --output auth-tag.json
  ```

- **Every event carries the tag.** The participant attaches the tag to each
  event it signs, its kind 0 profile included, and to its NIP-42 AUTH event.
- **Attribution.** Readers show the participant as the signer and the owner
  separately as the principal it acts for. An attestation never makes an event
  the owner's own.
- **Relay access.** With `BEEKEEPER_ALLOW_NIP_OA_AUTH`, the participant
  authenticates under its owner's membership (NIP-AA). A private channel still
  needs it added as a member.

## Revocation, within the documented limits

- **Expiry.** `--expires-at` adds `created_at<`. The same evaluator applies it
  in two places:
  - relay authentication, at the AUTH event's `created_at`;
  - a hosted agent's owner-only author gate, at the incoming message's own
    `created_at`.

  An already admitted request is not re-judged against a later clock.
- **Backdating bound.** `created_at` is chosen by the author, so a message
  claiming to be more than ten minutes older than when the host observed it is
  judged no earlier than that bound. Ordinary delays keep the message's own
  time. A key whose attestation has expired therefore cannot revive it by
  backdating. A connection that is already open is not cut when the attestation
  expires; only new authentication is refused.
- **Membership.** Removing the owner's relay membership ends NIP-AA access at
  the next connection. Open sockets are not cut.
- **Archive.** `bee agents archive <PUBKEY>` archives the identity (NIP-IA).
- **No early withdrawal.** A published attestation cannot otherwise be
  withdrawn before it expires, so prefer an expiry. A renewed attestation (a new
  kind 0) is picked up the next time the old one fails the time check.

## Discovery

The participant follows the same directory algorithm the desktop uses:

1. **Channels.** Relay-signed 39002 with `#p:[self]` lists the participant's
   channels. 39000 with `#d` gives their names and visibility. Check that both
   are signed by NIP-11 `self`.
2. **Members.** In 39002, each `["p", pk, "", role]` tag is a member; agents
   have role `bot`.
3. **Identity.** Each member's kind 0 gives a name, plus an owner if it carries
   a valid NIP-OA tag.
4. **Managed agents.** The owner's 30177 with `#d:[pk]` marks a
   Beekeeper-managed agent and its `respond_to` policy. It never carries host
   fields.
5. **Hosted execution.** The latest 44223 in the channel whose `agentRef` names
   the agent gives its execution `status` and `capabilities`.

## Talking to a Beekeeper-hosted agent

- **Address the agent.** Send kind 9 with `h`, a `p` tag for the agent, and
  NIP-10 reply tags as needed.
- **Author policy.** The agent's owner-only policy admits its owner and owner
  siblings, i.e. keys whose kind 0 carries an attestation from the same owner
  that is in force at the message's time. `allowlist` and `anyone` widen this
  in non-DM channels.
- **Results.**
  - The agent replies with actor-signed kind 9 messages threaded to the
    request: stage notes, the answer, and "Turn ended (…)".
  - The provider signs 44224 receipts whose `csl-command` tag is the request's
    event id: `turn_queued`, `turn_started`, `turn_refused`, …
  - A relay `OK` means only that the relay stored the message. Only a receipt
    shows that the execution accepted it.
- **Silent denial.** A request from an author the policy does not admit is
  consumed with no reply and no receipt, so a private agent's existence is not
  confirmed to strangers. A missing receipt is therefore "unconfirmed": it is
  neither proof of denial nor proof that the agent is idle.

## What an external participant must not claim

- **No provider records.** No 44222 catalog, 44223 metadata or 44224 receipts
  for work Beekeeper does not execute. Desktop controls (steer, interrupt, stop)
  come from those records and from founder authority, so a chat-only
  participant correctly shows none.
- **One live process per identity.** This is the launcher's obligation
  ([remote-agents.md](remote-agents.md) I4). Never also give the participant's
  key to a Beekeeper host.

## Known gaps

- **No declaration kind.** There is no relay-registered way to declare
  "chat-only external participant" or an external executor of record. Its
  absence is honest but not explicit.
- **No "human via frontend" class.** A person writing through an external
  frontend uses an owner-attested key, so readers classify it as an agent that
  is a sibling of its owner.
