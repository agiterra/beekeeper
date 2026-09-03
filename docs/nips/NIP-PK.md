# NIP-PK — Project packs

`draft` `optional` `client` `relay`

`kind:30624` is one addressable record per project saying **where that
project's persona packs live**: which git repository, at which commit or ref,
under which path. A seat's `kind:44223` metadata then carries a `packRef`
saying which of those bytes it was actually staged from.

The direction this answers (Brian, 2026-09-03):

> I should be able to log into beekeeper from any machine and have my agents
> and their packs. If a role evolves it should change on Andy's machine and any
> other team member's.

## Why a repository, not the wire

A role pack is a tree of text — seven roles is roughly 200 KB — that people
review, diff and revert. That is a git repository's job, not an event's. So
packs live in a repository on the relay's own git hosting and **the wire
carries the pointer and the proof**: a signed record naming the repository and
the commit, and a signed seat metadata record naming the commit that was
actually staged. A person can take a `packRef` and check out exactly the prompt
an agent ran.

Nothing about a pack's *contents* is specified here. This NIP is only about
saying where they are and proving what ran.

## Allocation

`30624` is the lowest unused parameterized-replaceable value in this fork's
project range. The scan checked this tree and `vanilla/main`:

- `30617`/`30618` are NIP-34 repository announcement and ref state.
- `30620` is the workflow definition, `30621` the NIP-MP project,
  `30622` the NIP-DV per-viewer DM visibility snapshot, `30623` the NIP-ST
  shared-terminal announce.
- `30624` matched nothing but lockfile hashes in either tree before this NIP.

Fork-local allocation, not a claim on the global Nostr registry.

## Envelope

A regular parameterized-replaceable event. Tags, in any order, each exactly two
fields:

```json
[
  ["d",    "30621:<owner-hex>:<slug>"],
  ["repo", "30617:<owner-hex>:<id>"],
  ["sha",  "<40-hex>"],
  ["path", "personas/roles"]
]
```

* `d` — **required, singleton.** The project's own NIP-MP coordinate, so a
  project has exactly one pack source and newest-per-`d` wins by NIP-33.
  Removal is the kind-5 tombstone the relay already honours for addressables;
  there is deliberately no "none" sentinel value to mis-read.
* `repo` — **required, singleton.** The packs repository's NIP-34 coordinate.
* `ref` / `sha` — **exactly one, singleton.** `ref` is a fully qualified ref
  name (`refs/heads/main`) whose tip is staged; `sha` is a 40-hex commit that
  is staged verbatim. Both is invalid, neither is invalid. A record carrying
  both would let two honest hosts stage two different trees from one signed
  event; a record carrying neither points at nothing.
* `path` — **optional, singleton.** A repository-relative directory holding one
  directory per role. Defaults to `personas/roles`. Absolute paths, `..`
  segments, backslashes and colons are refused: a host joins this value to a
  tree it fetched over the network, so a path that escapes is a path that reads
  the operator's disk.

Unknown tag names are **refused, not ignored**. This kind has no signed history
to protect, and a silently dropped tag on a record that decides which code a
seat runs is the wrong tolerance — a mistyped `sha` landing as an unknown tag
would stage a ref's tip while its author believed they had pinned a commit.
Widening the record means a new content schema version.

## Content

```json
{
  "schema": "buzz-project-pack-source/v1",
  "note": "pinned for run 5"
}
```

`schema` is required. `note` is optional, at most 512 bytes, and **omitted when
absent** — an explicit `null` is refused naming the key, matching the
`routing`/`beeStamp` discipline on kind 44223: a null in a producer's output
means the producer invented a shape. Content is capped at 2048 bytes.

## Who may write one

A pack source decides which prompt bytes every seat on a project runs, so the
relay's gate is **closed by default**. It admits an author who is one of:

1. the project coordinate's own pubkey (the creator, an implicit Owner
   everywhere the roster is read);
2. a roster **Owner** of that project;
3. a **founder** of a repository belonging to that project — the kind:30617
   signer, its NIP-34 `maintainers`, and the project's roster Owners
   (`docs/nips/NIP-GS.md`, finding 33).

Clause 3 is what finding 33 bought. `agiterra-beekeeper` is signed by one human
and co-owned by two; a rule keyed to the signer alone would let one founder set
the team's packs and refuse the other.

A refusal is HTTP 403 (CLI exit 3) and says what was checked, including how
many of the project's repositories were searched and whether the roster could
be read at all — the newest 500 repository announcements in the community are
read and filtered by their `project` back-reference, and a bound that could
have hidden an answer is disclosed rather than reported as "not a founder".

A storage failure refuses the write as an internal error. Narrowing the founder
set on a Postgres blip would hand one key silent control of the team's packs.

## `packRef` on kind 44223

A seat's metadata gains one additive, **read-optional** key:

```json
"packRef": {
  "repo": "30617:<owner-hex>:<id>",
  "sha":  "<40-hex>",
  "role": "builder",
  "path": "personas/roles/builder"
}
```

All four fields are required and non-null inside the object: a repository with
no commit, or a commit with no path, is a worse answer than no answer. `sha` is
always the **resolved** commit even when the pack source pinned a ref — the
host records what it fetched, not what it asked for. `path` must end in the
role it staged, and `role` must equal the seat's own `role`: the **seat's** role
picks the pack, and the actor's home role is never consulted for staging.

The key is **absent**, never `null`, when no pack was staged — which is exactly
today's behaviour and remains correct when a project has published no pack
source. Readers show `no pack staged`; none of them invents a default.

This is finding 31's rule applied on the way in rather than after the fact:
every reader accepts absence, and each implementation ships a "signed before
the key existed" decode test
(`crates/buzz-core/src/coding_session_payload.rs`,
`decode_metadata_reads_a_44223_signed_before_pack_ref_existed`).

## Shipped defaults — the third rung

A team that has published nothing still gets working roles, and the wire says
so. A host stages, in order:

1. the packs repository the project's kind:30624 names;
2. the session checkout's own `personas/roles/<role>/`, overlaid file by file
   (the checkout wins, so a lane can iterate on a role without publishing);
3. the packs the running app's own build bundles.

When rung 3 answers, the seat's `packRef` carries `"repo": "app:shipped"` and
the **app version** as `sha`:

```json
"packRef": { "repo": "app:shipped", "sha": "0.5.16",
             "role": "builder", "path": "personas/roles/builder" }
```

`app:shipped` is deliberately not coordinate-shaped. A build's bundled packs are
not a repository anyone can fetch, and dressing them as `30617:…` would invite a
reader to go looking for one. A blank version is refused: *shipped, from a build
that will not say which* is exactly the unknown-as-empty this key exists to
prevent.

Surfaces name the three with one vocabulary — `packs repository`,
`session checkout`, `shipped defaults` — spelled once in
`buzz_core::project_pack_source` so the CLI, the seat chip and Project settings
cannot disagree.

## Staging

A host resolving a seat:

1. reads the session's project → the newest kind:30624 for that coordinate;
2. clones or fetches the packs repository from the relay's git hosting through
   the existing `git-credential-nostr` wiring, into its packs cache
   (`<app data>/packs/<owner8>-<id>/`, the name
   `ProjectPackSource::cache_dir_name` derives);
3. checks out the pinned `sha`, or the ref's tip while **recording** the sha it
   resolved;
4. stages `<path>/<seat role>/`, then overlays the session checkout's own
   `personas/roles/<role>/` file by file — the checkout wins, so a lane can
   iterate on a role without publishing;
5. publishes the resulting `packRef` on the seat's 44223.

With no kind:30624 the host stages the checkout's packs alone and publishes no
`packRef`. A pack that cannot be fetched **refuses the hire** with a sentence;
it never becomes a silent bare persona.

## Reading it

```bash
bee packs init       --project 30621:<owner-hex>:agiterra
bee packs get-source --project 30621:<owner-hex>:agiterra
bee packs status     --project 30621:<owner-hex>:agiterra --role builder
bee packs set-source --project 30621:<owner-hex>:agiterra \
                     --repo 30617:<owner-hex>:agiterra-packs \
                     --ref refs/heads/main
bee sessions explain pack
```

`bee packs init` is the setup path, and does the same three steps as the app's
**Create packs repository**, in the order that makes them safe: announce the
`30617` (`<slug>-packs`, under the signer's key, with the project
back-reference) so the relay's git gate will admit the push; seed it from the
role packs on disk with one DCO-signed commit pushed to `refs/heads/main`; then
publish the 30624 **pinned to the sha that actually landed**. A failure at any
step stops the sequence and prints what already landed, so nothing ever points
at a repository with no packs in it. A project that already has a pack source is
refused — replacing one re-points every seat on the project, and that is a
deliberate `set-source`.

`bee packs status` separates the two questions it can answer: what the **wire**
says would be staged, and what is on **this disk**. `cache_present: null` means
this machine has never fetched these packs — a different fact from a role the
repository does not carry.

## Conformance

`conformance/project-pack-source/fixtures/pack-source-vectors.json` carries
both halves: `packSourceVectors` (tags + content for kind 30624) and
`metadataVectors` (kind 44223 content, including the pre-`packRef` shape that
must keep decoding). Every implementation reads the same file.

## See also

- `docs/nips/NIP-MP.md` — projects and their rosters
- `docs/nips/NIP-GS.md` — repository founders and the push gate
- `docs/design/singularity/SURFACES.md` — W17, the seat chip's pack line
