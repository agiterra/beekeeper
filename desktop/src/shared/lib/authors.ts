import {
  type NativeSignatureVerdict,
  type NativeSignatureVerifier,
  nativeSignatureVerifier,
  type SignedEventFields,
} from "@/shared/api/eventSignatures";
import { normalizePubkey } from "@/shared/lib/pubkey";
import { getEventHash, validateEvent, verifyEvent } from "nostr-tools/pure";

const PUBKEY_HEX_RE = /^[0-9a-f]{64}$/i;

function normalizeValidPubkey(pubkey: string | null | undefined) {
  if (!pubkey) {
    return null;
  }

  const normalized = normalizePubkey(pubkey);
  return PUBKEY_HEX_RE.test(normalized) ? normalized : null;
}

function getTaggedPubkey(
  tags: string[][],
  tagName: string,
  options?: {
    firstTagOnly?: boolean;
  },
) {
  const candidates = options?.firstTagOnly ? tags.slice(0, 1) : tags;

  for (const tag of candidates) {
    const taggedPubkey = tag[0] === tagName ? tag[1]?.toLowerCase() : null;
    if (taggedPubkey && PUBKEY_HEX_RE.test(taggedPubkey)) {
      return taggedPubkey;
    }
  }

  return null;
}

type AuthorResolutionEvent = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
  sig: string;
};

// Cache only a cryptographic fact about exact signed bytes, never authority or
// community state. Keyed by the (id, sig) pair, not object identity: every IPC
// response builds fresh event objects, so an identity-keyed cache never hit and
// periodic re-queries of ~2,000 events re-ran BigInt Schnorr math each time
// (45-70% CPU bursts, ledger 310). The id is recomputed from the signed fields
// on every call before the cache is consulted, so a reused id+sig over altered
// content still fails; once the id matches, (id, sig) fixes the verdict.
const SIGNATURE_CACHE_LIMIT = 20_000;
const signatureChecks = new Map<string, boolean>();

function rememberSignatureCheck(key: string, valid: boolean) {
  if (signatureChecks.size >= SIGNATURE_CACHE_LIMIT) {
    const oldest = signatureChecks.keys().next();
    if (!oldest.done) signatureChecks.delete(oldest.value);
  }
  signatureChecks.set(key, valid);
}

/** Test seam: the number of cached (id, sig) verdicts. */
export function signatureCheckCacheSize() {
  return signatureChecks.size;
}

/** Test seam: forget every cached verdict. */
export function clearSignatureCheckCache() {
  signatureChecks.clear();
}

/**
 * Verify an event's signature, projecting the exact seven signed fields.
 *
 * Callers hand in richer objects — relay rows, cached events, projections —
 * that carry extra local bookkeeping. `verifyEvent` recomputes the id from the
 * object it is given, so passing one of those through unprojected makes
 * verification depend on properties that were never signed. Naming the seven
 * fields keeps the check on the wire event and nothing else.
 *
 * Exported because the coding-session trusted-ingress path needs the same
 * signature gate before it will accept a provider-authored event.
 */
export function hasValidSignature(event: AuthorResolutionEvent) {
  try {
    const projected = projectSignedFields(event);
    // Cheap (one sha256): binds the cache key to these exact signed bytes.
    if (getEventHash(projected) !== projected.id) return false;
    const key = `${projected.id}:${projected.sig}`;
    const previous = signatureChecks.get(key);
    if (previous !== undefined) {
      // Refresh recency so hot events survive eviction.
      signatureChecks.delete(key);
      signatureChecks.set(key, previous);
      return previous;
    }
    const valid = verifyEvent(projected);
    rememberSignatureCheck(key, valid);
    return valid;
  } catch {
    return false;
  }
}

function projectSignedFields(event: AuthorResolutionEvent): SignedEventFields {
  return {
    id: event.id,
    pubkey: event.pubkey,
    created_at: event.created_at,
    kind: event.kind,
    tags: event.tags,
    content: event.content,
    sig: event.sig,
  };
}

/**
 * Events per native call (the command refuses more than 20,000). Small enough
 * that preparing and serialising one chunk stays well under a frame budget's
 * worth of main-thread time; large enough that IPC overhead is noise.
 */
const NATIVE_BATCH_SIZE = 500;
/** JavaScript fallback: verify for at most this long before yielding. */
const FALLBACK_SLICE_MS = 8;

// Only events the JavaScript check could itself accept go to the native
// verifier — the same shape rules (`validateEvent`: lower-case hex pubkey,
// string tags, numeric kind and time), so the two paths cannot disagree about
// what is acceptable, only about how fast they answer. A string with an
// unpaired surrogate cannot cross the IPC boundary (serde rejects the escape
// and the whole call would fail), so those go to the JavaScript check too.
const LONE_SURROGATE_RE =
  /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/;

function suitsNativeCheck(event: SignedEventFields) {
  if (!validateEvent(event)) return false;
  if (LONE_SURROGATE_RE.test(event.content)) return false;
  return event.tags.every((tag) =>
    tag.every((value) => !LONE_SURROGATE_RE.test(value)),
  );
}

function yieldToEventLoop() {
  return new Promise<void>((resolve) => setTimeout(resolve, 0));
}

/**
 * Verify a batch of events off the main thread and remember the verdicts, so
 * the synchronous {@link hasValidSignature} calls that follow are cache hits.
 *
 * Returns one verdict per event, in input order, about each event's signed
 * fields *as they were when sent for checking*. A later
 * {@link hasValidSignature} re-hashes whatever it is handed, so mutating an
 * event afterwards cannot inherit this verdict.
 *
 * Inside the packaged app the batch goes to the native verifier
 * (`verify_event_signatures`), which recomputes every id from the fields it
 * received and checks the Schnorr signature in libsecp256k1 across cores.
 * Only two of its answers are remembered, both stable facts about an
 * `(id, sig)` pair whose id was proven to hash the signed fields: `valid`, and
 * `invalid-signature`. `unchecked` — an id that does not hash its fields — is
 * never cached, because a forger can pair a real id and signature with any
 * content, and caching that refusal would let them knock out the real event.
 * Anything the native path did not settle (unchecked, an IPC failure, or no
 * Tauri at all) is checked in JavaScript, in slices that yield to the event
 * loop between them.
 *
 * The trust rule is unchanged: an event is only ever reported valid after its
 * id was recomputed from its signed fields and its signature verified.
 */
export async function verifyEventSignatures(
  events: readonly AuthorResolutionEvent[],
  options: { verifier?: NativeSignatureVerifier | null } = {},
): Promise<boolean[]> {
  const verifier =
    options.verifier === undefined
      ? nativeSignatureVerifier()
      : options.verifier;
  const verdicts: Array<boolean | undefined> = new Array(events.length);

  if (verifier) {
    // Prepared and sent a chunk at a time, so the main thread's share — the
    // projection, the shape checks and the IPC serialisation — comes in
    // short stretches with the event loop free in between.
    const pendingKeys = new Set<string>();
    let chunk: Array<{ index: number; fields: SignedEventFields }> = [];
    const flush = async () => {
      const sent = chunk;
      chunk = [];
      if (sent.length === 0) return;
      let answers: NativeSignatureVerdict[];
      try {
        answers = await verifier(sent.map((entry) => entry.fields));
      } catch {
        return;
      }
      if (!Array.isArray(answers) || answers.length !== sent.length) return;
      sent.forEach((entry, offset) => {
        const answer = answers[offset];
        if (answer !== "valid" && answer !== "invalid-signature") return;
        const valid = answer === "valid";
        rememberSignatureCheck(`${entry.fields.id}:${entry.fields.sig}`, valid);
        verdicts[entry.index] = valid;
      });
    };
    for (let index = 0; index < events.length; index += 1) {
      const event = events[index];
      if (!event) continue;
      const fields = projectSignedFields(event);
      const key = `${fields.id}:${fields.sig}`;
      if (signatureChecks.has(key) || pendingKeys.has(key)) continue;
      if (!suitsNativeCheck(fields)) continue;
      // Snapshot the signed fields now: the verdict describes these bytes.
      chunk.push({
        index,
        fields: { ...fields, tags: fields.tags.map((tag) => [...tag]) },
      });
      pendingKeys.add(key);
      if (chunk.length >= NATIVE_BATCH_SIZE) await flush();
    }
    await flush();
  }

  // Everything else: cache hits re-hash and return at once; the rest run the
  // JavaScript check, a slice at a time.
  let sliceStart = performance.now();
  for (let index = 0; index < events.length; index += 1) {
    if (verdicts[index] !== undefined) continue;
    const event = events[index];
    verdicts[index] = event ? hasValidSignature(event) : false;
    if (performance.now() - sliceStart >= FALLBACK_SLICE_MS) {
      await yieldToEventLoop();
      sliceStart = performance.now();
    }
  }
  return verdicts.map((verdict) => verdict === true);
}

export function resolveEventAuthorPubkey(input: {
  event: AuthorResolutionEvent;
  preferActorTag?: boolean;
  relaySelfPubkey?: string | null;
  requireChannelTagForPTags?: boolean;
}) {
  const {
    event,
    preferActorTag = false,
    relaySelfPubkey,
    requireChannelTagForPTags = false,
  } = input;

  const signerPubkey = normalizePubkey(event.pubkey);
  const normalizedRelaySelf = normalizeValidPubkey(relaySelfPubkey);

  // `actor` and author-attributing `p` tags are delegated authorship claims.
  // The relay creates these for workflow-generated and legacy relay-signed
  // attributed events, so they are only authoritative when the event is signed
  // by the active relay advertised in NIP-11. Missing or malformed relay
  // identity data must leave the signer as the visible author.
  if (!normalizedRelaySelf || signerPubkey !== normalizedRelaySelf) {
    return signerPubkey;
  }

  let attributedPubkey: string | null = null;
  if (preferActorTag) {
    attributedPubkey = getTaggedPubkey(event.tags, "actor");
  }

  if (!attributedPubkey) {
    const canUseAttributedPTag =
      !requireChannelTagForPTags || event.tags.some((tag) => tag[0] === "h");
    if (canUseAttributedPTag) {
      attributedPubkey = getTaggedPubkey(event.tags, "p", {
        firstTagOnly: true,
      });
    }
  }

  if (!attributedPubkey || !hasValidSignature(event)) {
    return signerPubkey;
  }

  return attributedPubkey;
}
