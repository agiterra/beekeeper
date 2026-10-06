/**
 * Mock-relay seeds for generated session titles (kind 44252, SV-31).
 *
 * A 44252 is verified signature-first and counted only when its signer has
 * standing in the session — a 44223 for the title's `cs-target` and a 44224
 * receipt confirming that generation, both signed by the same key
 * (`features/coding-sessions/lib/codingSessionTitle.ts`). So nothing here is
 * synthesized by the bridge: a spec builds the whole signed event with these
 * helpers, from the execution signer's key (a title with standing) or from a
 * key that runs nothing in the session (a foreign title, which readers set
 * aside), and seeds it through `__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__`.
 *
 * The mock relay already serves the kind: 44252 is in
 * `CODING_SESSION_EVENT_KINDS`, which drives the single-channel history path
 * and `respondToMockMultiChannelSessionFacts` (`e2eBridgeSessionFacts.ts`)
 * alike. Kept out of `e2eBridge.ts`, which must not grow.
 */
import { finalizeEvent } from "nostr-tools/pure";

import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_GENERATED_TITLE } from "@/shared/constants/kinds";
import {
  CODING_SESSION_TITLE_SCHEMA,
  CODING_SESSION_TITLE_TAG_VERSION,
} from "@/shared/coordination/sessionCoordinationNames";

/** What one generated title says, before it is signed. */
export type MockGeneratedTitleInput = {
  channelId: string;
  sessionRef: string;
  /** The `cs-target` key of the execution the signer claims to have run. */
  targetKey: string;
  title: string;
  model: string;
  /** The 44221 create the title belongs to (lowercase 64-hex). */
  createEventId: string;
  /** The 44220 that carried the first message, or null for a create's own. */
  sourceCommand?: string | null;
  createdAt: number;
};

/** The exact 44252 envelope: `h`, `d`, `cstl-v`, `cs-target`, in order. */
export function mockGeneratedTitleTemplate(input: MockGeneratedTitleInput): {
  kind: number;
  created_at: number;
  tags: string[][];
  content: string;
} {
  return {
    kind: KIND_CODING_SESSION_GENERATED_TITLE,
    created_at: input.createdAt,
    tags: [
      ["h", input.channelId],
      ["d", input.sessionRef],
      ["cstl-v", CODING_SESSION_TITLE_TAG_VERSION],
      ["cs-target", input.targetKey],
    ],
    // Key order is the schema's; the reader rejects extra or missing keys.
    content: JSON.stringify({
      schema: CODING_SESSION_TITLE_SCHEMA,
      title: input.title,
      model: input.model,
      basis: "first-message",
      sourceCommand: input.sourceCommand ?? null,
      createEventId: input.createEventId,
    }),
  };
}

/**
 * A 44252 signed by `signerSecret`. Pass the execution signer's key for a
 * title with standing, any other key for a foreign one.
 */
export function signMockGeneratedTitle(
  input: MockGeneratedTitleInput,
  signerSecret: Uint8Array,
): RelayEvent {
  return finalizeEvent(
    mockGeneratedTitleTemplate(input),
    signerSecret,
  ) as unknown as RelayEvent;
}
