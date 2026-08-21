import assert from "node:assert/strict";
import test from "node:test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { KIND_CODING_SESSION_PROVIDER_CATALOG } from "@/shared/constants/kinds.ts";
import {
  buildCodingSessionProviderCatalogFilter,
  buildCodingSessionProviderCatalogHistoryFilters,
} from "../useCodingSessionProviderCatalog.ts";
import { resolveCodingSessionIngressAuthority } from "./codingSessionIngressAuthority.ts";
import {
  classifyCodingSessionProviderCatalogEvent,
  CODING_SESSION_PROVIDER_CATALOG_SCHEMA,
  CODING_SESSION_PROVIDER_CATALOG_TAG_VERSION,
  codingSessionProviderCatalogSemanticKey,
  CodingSessionProviderCatalogStore,
  codingSessionProvidersForProject,
  parseCodingSessionProviderCatalog,
} from "./codingSessionProviderCatalog.ts";

const CHANNEL = "catalog-channel";
const PROVIDER_SECRET = generateSecretKey();
const OTHER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const OTHER_PUBKEY = getPublicKey(OTHER_SECRET);
const AUTHORITY = resolveCodingSessionIngressAuthority([
  { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
]);
const PLURAL_AUTHORITY = resolveCodingSessionIngressAuthority([
  { pubkey: PROVIDER_PUBKEY, label: "This computer" },
  { pubkey: OTHER_PUBKEY, label: "Second computer" },
]);

function provider(overrides = {}) {
  return {
    providerInstanceRef: "claude-primary",
    driver: "claude-agent-acp",
    runtime: "claude",
    defaultModel: "claude-opus-5",
    allowedModels: ["claude-opus-5", "claude-sonnet-4-6"],
    capabilities: {
      threadTurnStart: true,
      threadTurnInterrupt: true,
      threadSteer: false,
      context: false,
      diff: false,
      plan: true,
    },
    ...overrides,
  };
}

function catalog(overrides = {}) {
  return {
    schema: CODING_SESSION_PROVIDER_CATALOG_SCHEMA,
    revision: 7,
    providers: [provider()],
    ...overrides,
  };
}

function catalogWithProjects(overrides = {}) {
  return catalog({
    projects: [
      {
        projectRef: "30621:owner:agiterra",
        repoRef: null,
        providers: ["claude-primary"],
      },
      {
        projectRef: "30621:owner:buzz",
        repoRef: null,
        providers: ["claude-primary"],
      },
    ],
    ...overrides,
  });
}

function catalogEvent(
  value = catalog(),
  {
    secret = PROVIDER_SECRET,
    channelId = CHANNEL,
    content = JSON.stringify(value),
    tags,
    createdAt = 1_800_000_000,
    kind = KIND_CODING_SESSION_PROVIDER_CATALOG,
  } = {},
) {
  return finalizeEvent(
    {
      kind,
      created_at: createdAt,
      tags: tags ?? [
        ["h", channelId],
        ["cspc-v", CODING_SESSION_PROVIDER_CATALOG_TAG_VERSION],
        ["cspc-revision", String(value.revision)],
        [
          "cspc-key",
          codingSessionProviderCatalogSemanticKey(
            channelId,
            value.revision,
            content,
          ),
        ],
      ],
      content,
    },
    secret,
  );
}

test("the canonical fork catalog is providers-first, with projects optional", () => {
  const bare = parseCodingSessionProviderCatalog(JSON.stringify(catalog()));
  assert.deepEqual(bare, catalog());
  assert.equal("projects" in bare, false, "an absent narrowing stays absent");

  assert.deepEqual(
    parseCodingSessionProviderCatalog(JSON.stringify(catalogWithProjects())),
    catalogWithProjects(),
  );
});

test("an empty projects array is not canonical — the producer omits the key", () => {
  assert.equal(
    parseCodingSessionProviderCatalog(
      JSON.stringify(catalog({ projects: [] })),
    ),
    null,
  );
});

test("a top-level providers list is required and bounded", () => {
  for (const invalid of [
    { schema: CODING_SESSION_PROVIDER_CATALOG_SCHEMA, revision: 1 },
    catalog({ providers: [] }),
    catalog({ providers: "not-an-array" }),
    catalog({ revision: 0 }),
    catalog({ revision: 1.5 }),
    catalog({ schema: "something-else/v1" }),
  ]) {
    assert.equal(
      parseCodingSessionProviderCatalog(JSON.stringify(invalid)),
      null,
    );
  }
});

test("codec rejects stale models, unknown capabilities, and duplicate provider refs", () => {
  for (const invalid of [
    catalog({
      providers: [provider({ allowedModels: ["claude-sonnet-4-6"] })],
    }),
    catalog({
      providers: [
        provider({
          allowedModels: ["claude-opus-5", "z-model", "a-model"],
        }),
      ],
    }),
    catalog({
      providers: [
        provider({
          capabilities: { ...provider().capabilities, terminal: true },
        }),
      ],
    }),
    catalog({ providers: [provider(), provider()] }),
    catalog({
      providers: [
        provider({ providerInstanceRef: "z-provider" }),
        provider({ providerInstanceRef: "a-provider" }),
      ],
    }),
  ]) {
    assert.equal(
      parseCodingSessionProviderCatalog(JSON.stringify(invalid)),
      null,
    );
  }
});

test("a project narrowing must reference declared providers, sorted and deduplicated", () => {
  for (const projects of [
    [
      {
        projectRef: "30621:owner:agiterra",
        repoRef: null,
        providers: ["never-declared"],
      },
    ],
    [
      {
        projectRef: "30621:owner:agiterra",
        repoRef: null,
        providers: ["claude-primary", "claude-primary"],
      },
    ],
    [
      {
        projectRef: "30621:owner:agiterra",
        repoRef: null,
        providers: [],
      },
    ],
    [
      {
        projectRef: "30621:owner:buzz",
        repoRef: null,
        providers: ["claude-primary"],
      },
      {
        projectRef: "30621:owner:agiterra",
        repoRef: null,
        providers: ["claude-primary"],
      },
    ],
    [
      {
        projectRef: "30621:owner:agiterra",
        repoRef: null,
        providers: ["claude-primary"],
      },
      {
        projectRef: "30621:owner:agiterra",
        repoRef: null,
        providers: ["claude-primary"],
      },
    ],
  ]) {
    assert.equal(
      parseCodingSessionProviderCatalog(JSON.stringify(catalog({ projects }))),
      null,
      JSON.stringify(projects),
    );
  }
});

test("key order is load-bearing, because the digest is over the exact bytes", () => {
  const reordered = JSON.stringify({
    revision: 7,
    schema: CODING_SESSION_PROVIDER_CATALOG_SCHEMA,
    providers: [provider()],
  });
  assert.equal(parseCodingSessionProviderCatalog(reordered), null);
  assert.equal(
    parseCodingSessionProviderCatalog(
      JSON.stringify(
        catalog({
          providers: [
            {
              driver: "claude-agent-acp",
              providerInstanceRef: "claude-primary",
              runtime: "claude",
              defaultModel: "claude-opus-5",
              allowedModels: ["claude-opus-5"],
              capabilities: provider().capabilities,
            },
          ],
        }),
      ),
    ),
    null,
  );
});

test("a catalog with no narrowing offers every provider to every project", () => {
  const bare = parseCodingSessionProviderCatalog(JSON.stringify(catalog()));
  assert.deepEqual(codingSessionProvidersForProject(bare, null), [provider()]);
  assert.deepEqual(codingSessionProvidersForProject(bare, "30621:owner:any"), [
    provider(),
  ]);

  const narrowed = parseCodingSessionProviderCatalog(
    JSON.stringify(catalogWithProjects()),
  );
  assert.deepEqual(
    codingSessionProvidersForProject(narrowed, "30621:owner:agiterra"),
    [provider()],
  );
  assert.deepEqual(
    codingSessionProvidersForProject(narrowed, "30621:owner:unlisted"),
    [],
  );
  assert.deepEqual(codingSessionProvidersForProject(narrowed, null), [
    provider(),
  ]);
});

test("classifier requires the native kind, exact channel, signer, signature, tags, and content hash", () => {
  const allowed = new Set([CHANNEL]);
  const classified = classifyCodingSessionProviderCatalogEvent(
    catalogEvent(),
    allowed,
    AUTHORITY,
  );
  assert.equal(classified.kind, "catalog");
  assert.equal(classified.entry.signerPubkey, PROVIDER_PUBKEY);
  assert.deepEqual(classified.entry.catalog, catalog());

  assert.equal(
    classifyCodingSessionProviderCatalogEvent(
      catalogEvent(catalog(), { kind: 9 }),
      allowed,
      AUTHORITY,
    ).kind,
    "irrelevant",
    "kind 9 is not a catalog transport in this fork",
  );
  assert.equal(
    classifyCodingSessionProviderCatalogEvent(
      catalogEvent(catalog(), { secret: OTHER_SECRET }),
      allowed,
      AUTHORITY,
    ).kind,
    "rejected-author",
  );
  assert.equal(
    classifyCodingSessionProviderCatalogEvent(
      { ...catalogEvent(), sig: "bad" },
      allowed,
      AUTHORITY,
    ).kind,
    "invalid-signature",
  );
  assert.equal(
    classifyCodingSessionProviderCatalogEvent(
      catalogEvent(),
      allowed,
      resolveCodingSessionIngressAuthority([]),
    ).kind,
    "malformed",
  );
  for (const event of [
    catalogEvent(catalog(), { channelId: "other-channel" }),
    catalogEvent(catalog(), {
      tags: [
        ["h", CHANNEL],
        ["cspc-v", CODING_SESSION_PROVIDER_CATALOG_TAG_VERSION],
        ["cspc-revision", "8"],
        [
          "cspc-key",
          codingSessionProviderCatalogSemanticKey(
            CHANNEL,
            7,
            JSON.stringify(catalog()),
          ),
        ],
      ],
    }),
    catalogEvent(catalog(), {
      tags: [
        ["h", CHANNEL],
        ["cspc-v", CODING_SESSION_PROVIDER_CATALOG_TAG_VERSION],
        ["cspc-revision", "7"],
        ["cspc-key", "wrong"],
      ],
    }),
  ]) {
    assert.equal(
      classifyCodingSessionProviderCatalogEvent(event, allowed, AUTHORITY).kind,
      "malformed",
    );
  }
});

test("store keeps only the newest trusted catalog revision per channel and signer", () => {
  const store = new CodingSessionProviderCatalogStore();
  store.ingestRelayEvents(
    [
      catalogEvent(catalog({ revision: 7 })),
      catalogEvent(
        catalog({
          revision: 9,
          providers: [
            provider({
              defaultModel: "claude-opus-6",
              allowedModels: ["claude-opus-6"],
            }),
          ],
        }),
        { createdAt: 1_800_000_100 },
      ),
      catalogEvent(catalog({ revision: 8 }), { createdAt: 1_800_000_200 }),
    ],
    [CHANNEL],
    AUTHORITY,
  );
  const snapshot = store.snapshot([CHANNEL]);
  assert.equal(snapshot.entries.length, 1);
  assert.equal(snapshot.entries[0].catalog.revision, 9);
  assert.equal(
    snapshot.entries[0].catalog.providers[0].defaultModel,
    "claude-opus-6",
  );
  assert.equal(snapshot.conflictCount, 0);
  assert.deepEqual(store.snapshot(["another-channel"]).entries, []);
});

test("independent authorities union non-overlapping provider availability", () => {
  const store = new CodingSessionProviderCatalogStore();
  store.ingestRelayEvents(
    [
      catalogEvent(catalog()),
      catalogEvent(
        catalog({
          revision: 1,
          providers: [provider({ providerInstanceRef: "claude-secondary" })],
        }),
        { secret: OTHER_SECRET, createdAt: 1_800_000_050 },
      ),
    ],
    [CHANNEL],
    PLURAL_AUTHORITY,
  );
  const snapshot = store.snapshot([CHANNEL]);
  assert.equal(snapshot.conflictCount, 0);
  assert.deepEqual(
    snapshot.entries
      .flatMap((entry) =>
        entry.catalog.providers.map((offered) => offered.providerInstanceRef),
      )
      .sort(),
    ["claude-primary", "claude-secondary"],
  );
});

test("two signers describing one providerInstanceRef differently is a withheld conflict", () => {
  const store = new CodingSessionProviderCatalogStore();
  store.ingestRelayEvents(
    [
      catalogEvent(catalog()),
      catalogEvent(
        catalog({
          revision: 1,
          providers: [
            provider({
              defaultModel: "claude-sonnet-4-6",
              allowedModels: ["claude-sonnet-4-6"],
            }),
          ],
        }),
        { secret: OTHER_SECRET },
      ),
    ],
    [CHANNEL],
    PLURAL_AUTHORITY,
  );
  const snapshot = store.snapshot([CHANNEL]);
  assert.deepEqual(snapshot.conflicts, [
    { channelId: CHANNEL, providerInstanceRef: "claude-primary" },
  ]);
  assert.deepEqual(snapshot.entries, []);
});

test("a newer signed event wins across signers regardless of local revision numbers", () => {
  const store = new CodingSessionProviderCatalogStore();
  store.ingestRelayEvents(
    [
      catalogEvent(
        catalog({
          revision: 57,
          providers: [
            provider({
              defaultModel: "stale-model",
              allowedModels: ["stale-model"],
            }),
          ],
        }),
        { createdAt: 1_800_000_000 },
      ),
      catalogEvent(
        catalog({
          revision: 1,
          providers: [
            provider({
              defaultModel: "fresh-model",
              allowedModels: ["fresh-model"],
            }),
          ],
        }),
        { secret: OTHER_SECRET, createdAt: 1_800_000_500 },
      ),
    ],
    [CHANNEL],
    PLURAL_AUTHORITY,
  );
  const snapshot = store.snapshot([CHANNEL]);
  assert.equal(snapshot.conflictCount, 0);
  assert.equal(snapshot.entries.length, 1);
  assert.equal(snapshot.entries[0].signerPubkey, OTHER_PUBKEY);
  assert.equal(
    snapshot.entries[0].catalog.providers[0].defaultModel,
    "fresh-model",
  );
});

test("identical descriptors at a tied timestamp pick one deterministically", () => {
  const build = (order) => {
    const store = new CodingSessionProviderCatalogStore();
    store.ingestRelayEvents(order, [CHANNEL], PLURAL_AUTHORITY);
    return store.snapshot([CHANNEL]);
  };
  const mine = catalogEvent(catalog());
  const theirs = catalogEvent(catalog({ revision: 1 }), {
    secret: OTHER_SECRET,
  });
  const forward = build([mine, theirs]);
  const reversed = build([theirs, mine]);
  assert.equal(forward.conflictCount, 0);
  assert.equal(forward.entries.length, 1);
  assert.equal(
    forward.entries[0].signerPubkey,
    reversed.entries[0].signerPubkey,
  );
});

test("a narrowing naming only withheld providers is dropped from the reconciled entry", () => {
  const store = new CodingSessionProviderCatalogStore();
  store.ingestRelayEvents(
    [
      catalogEvent(
        catalogWithProjects({
          providers: [
            provider(),
            provider({ providerInstanceRef: "claude-secondary" }),
          ],
          projects: [
            {
              projectRef: "30621:owner:agiterra",
              repoRef: null,
              providers: ["claude-secondary"],
            },
            {
              projectRef: "30621:owner:buzz",
              repoRef: null,
              providers: ["claude-primary", "claude-secondary"],
            },
          ],
        }),
      ),
      catalogEvent(
        catalog({
          revision: 1,
          providers: [
            provider({
              providerInstanceRef: "claude-secondary",
              runtime: "other",
            }),
          ],
        }),
        { secret: OTHER_SECRET },
      ),
    ],
    [CHANNEL],
    PLURAL_AUTHORITY,
  );
  const snapshot = store.snapshot([CHANNEL]);
  assert.deepEqual(snapshot.conflicts, [
    { channelId: CHANNEL, providerInstanceRef: "claude-secondary" },
  ]);
  assert.equal(snapshot.entries.length, 1);
  assert.deepEqual(
    snapshot.entries[0].catalog.providers.map(
      (offered) => offered.providerInstanceRef,
    ),
    ["claude-primary"],
  );
  assert.deepEqual(snapshot.entries[0].catalog.projects, [
    {
      projectRef: "30621:owner:buzz",
      repoRef: null,
      providers: ["claude-primary"],
    },
  ]);
});

test("catalog relay filters are native-only, author-governed, and channel-scoped", () => {
  const filter = {
    kinds: [KIND_CODING_SESSION_PROVIDER_CATALOG],
    "#h": [CHANNEL],
    authors: [PROVIDER_PUBKEY],
    limit: 1000,
  };
  assert.deepEqual(
    buildCodingSessionProviderCatalogFilter([CHANNEL], [PROVIDER_PUBKEY], 1000),
    filter,
  );
  assert.deepEqual(
    buildCodingSessionProviderCatalogHistoryFilters(
      [CHANNEL],
      [PROVIDER_PUBKEY],
      1000,
    ),
    [filter],
  );
});
