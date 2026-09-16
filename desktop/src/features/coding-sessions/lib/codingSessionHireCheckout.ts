/**
 * Which repository a hired seat's worktree is cut from.
 *
 * **This is the fix for a seat that worked on the wrong codebase.** On
 * 2026-09-16 a Tank Loop lead hired two seats and both were cut from
 * `/Users/brian/Projects/beekeeper/beekeeper` at Beekeeper's tip, because the
 * hire host resolved its checkout as `byChannel[channel] ?? mru[0] ?? null`
 * (`ui/CodingSessionHireHost.tsx:113-119` before this change): Tank Loop's
 * transport channel had no `byChannel` entry and the most recently used
 * directory on the machine was Beekeeper's. The project's *own* recorded
 * checkout — `byProject`, set minutes earlier in Project settings → This
 * computer → Repository folder — was never consulted by a hire at all
 * (ledger 135(a)).
 *
 * Two rules, and both are about refusing to guess:
 *
 * 1. **A project's session is cut from the project's checkout or from
 *    nothing.** The most recently used directory is never an answer. It is
 *    not a fact about this session; it is a fact about whatever the person
 *    last opened, and using it hands a seat a repository nobody named.
 * 2. **Nothing recorded is a refusal, not a seat without a worktree.** A
 *    seated agent with no tree looks hired and can do no work; it is told to
 *    build and has nowhere to build. The refusal names the exact control that
 *    fixes it.
 *
 * The channel's remembered directory survives as the answer for a session
 * that belongs to **no** project, which is what it was always for. Where a
 * project *is* named and its checkout is recorded, the project's checkout
 * wins outright, and a channel directory that sits somewhere else is reported
 * — not silently dropped — because "the folder this channel last used" is
 * exactly what produced the wrong tree and a person reading the outcome
 * deserves to see it was passed over.
 */

/**
 * The refusal a hire earns when this computer has no checkout to cut from.
 *
 * **Not yet one of the contract codes.** `CODING_SESSION_HIRE_REFUSAL_CODES`
 * is pinned byte-for-byte to buzz-core's `HIRE_REFUSAL_CODES`, and that list
 * is in turn pinned to `bee`'s `hire_refusal_remedy` table — neither of which
 * this lane owns. So the code ships here first and travels on the wire in the
 * ordinary `hire refused: <CODE> — <reason>` shape, which `bee` parses
 * structurally: it prints the code and the host's whole reason, and simply
 * offers no generic remedy line of its own (`crew.rs:1999`, "a guessed remedy
 * for an unknown refusal is worse than none"). The reason below carries the
 * remedy, so nothing a lead needs is missing; adding the code to buzz-core
 * and a remedy to `bee` is a follow-up, not a prerequisite.
 */
export const HIRE_CHECKOUT_NOT_RECORDED = "HIRE_CHECKOUT_NOT_RECORDED" as const;

/** Where the path came from. Rendered beside it, never inferred later. */
export type CodingSessionHireCheckoutSource = "project" | "channel";

/** A directory remembered for a project coordinate or a channel. */
export type CodingSessionHireCheckoutEntry = { path: string };

export type CodingSessionHireCheckoutResolution =
  | {
      kind: "resolved";
      /** The checkout the seat's worktree is cut from. */
      path: string;
      source: CodingSessionHireCheckoutSource;
      /**
       * A directory this host read and did **not** use, with the reason. Null
       * when nothing was passed over.
       */
      passedOver: string | null;
    }
  | {
      kind: "unrecorded";
      code: typeof HIRE_CHECKOUT_NOT_RECORDED;
      /** The whole sentence, remedy included, published to the lead. */
      reason: string;
    };

export type ResolveCodingSessionHireCheckoutInput = {
  /** The umbrella's project coordinate, or null for a projectless session. */
  projectRef: string | null;
  /** The project's name, when this host knows it. Used only in prose. */
  projectLabel?: string | null;
  /** The hire's transport channel. */
  channelId: string;
  /** `byProject` from the host's working-directory store. */
  byProject: Readonly<Record<string, CodingSessionHireCheckoutEntry>>;
  /** `byChannel` from the same store. */
  byChannel: Readonly<Record<string, CodingSessionHireCheckoutEntry>>;
};

/**
 * Resolve the checkout one hire is cut from, or refuse it.
 *
 * Pure, and deliberately takes the store's two maps rather than the store:
 * `mru` is not a parameter here, so no later edit can reach for it without
 * changing this signature and this doc.
 */
export function resolveCodingSessionHireCheckout(
  input: ResolveCodingSessionHireCheckoutInput,
): CodingSessionHireCheckoutResolution {
  const projectRef = input.projectRef?.trim() ?? "";
  const project = readEntry(input.byProject, projectRef);
  const channel = readEntry(input.byChannel, input.channelId.trim());

  // 1. The project's own recorded checkout, first and outright.
  if (project !== null) {
    return {
      kind: "resolved",
      path: project,
      source: "project",
      passedOver:
        channel === null || isInside(channel, project)
          ? null
          : `the folder this channel last used, ${channel}, is not inside ` +
            `${project} and was not used`,
    };
  }

  // 2. The channel's remembered directory, only for a session that belongs to
  //    no project. With a project named and no checkout recorded for it there
  //    is nothing to compare the channel's folder against — it cannot be shown
  //    to be the project's repository — so it is refused rather than guessed.
  if (projectRef.length === 0) {
    if (channel !== null) {
      return {
        kind: "resolved",
        path: channel,
        source: "channel",
        passedOver: null,
      };
    }
    return {
      kind: "unrecorded",
      code: HIRE_CHECKOUT_NOT_RECORDED,
      reason:
        "this computer has no folder recorded for this session, and a seat " +
        "is never cut from whatever directory was most recently used. Open " +
        "the session's Folder control and choose the repository it works in, " +
        "then hire again.",
    };
  }

  // 3. A project session with no recorded checkout: name the exact control.
  const label = describeProject(projectRef, input.projectLabel);
  return {
    kind: "unrecorded",
    code: HIRE_CHECKOUT_NOT_RECORDED,
    reason:
      `this computer has no repository folder recorded for ${label}, and a ` +
      "seat is never cut from whatever directory was most recently used — " +
      "that is how two seats were cut from the wrong repository on " +
      `2026-09-16. Set the repository folder for ${label} in Project ` +
      "settings → This computer → Repository folder, then hire again." +
      (channel === null
        ? ""
        : ` (The folder this channel last used, ${channel}, is not the ` +
          "project's recorded checkout and cannot be shown to be its " +
          "repository, so it was not used.)"),
  };
}

/**
 * The line the umbrella shows for a seat that was cut.
 *
 * Says where from **and** which record answered, because "cut from
 * /Users/…/beekeeper" alone is exactly what a person read on 2026-09-16
 * without being able to tell that no project record had been consulted.
 */
export function codingSessionHireCheckoutLine(input: {
  role: string;
  path: string;
  source: CodingSessionHireCheckoutSource;
  passedOver?: string | null;
}): string {
  const said = `Hired a ${input.role} — worktree cut from ${input.path} (${describeSource(input.source)})`;
  const passedOver = input.passedOver?.trim();
  return passedOver ? `${said}; ${passedOver}` : said;
}

/** How a source is named to a person. Never a bare token. */
export function describeSource(
  source: CodingSessionHireCheckoutSource,
): string {
  return source === "project"
    ? "project checkout"
    : "this session's remembered folder";
}

function describeProject(
  projectRef: string,
  projectLabel: string | null | undefined,
): string {
  const label = projectLabel?.trim();
  return label && label.length > 0 ? label : `project ${projectRef}`;
}

function readEntry(
  map: Readonly<Record<string, CodingSessionHireCheckoutEntry>>,
  key: string,
): string | null {
  if (key.length === 0) return null;
  const path = map[key]?.path?.trim() ?? "";
  return path.length > 0 ? path : null;
}

/**
 * Whether `candidate` sits inside `root`, by path only.
 *
 * Containment proves the same repository without asking git anything. It is
 * deliberately one-directional and conservative: a sibling linked worktree of
 * the same repository is *not* proven by this and is reported as passed over
 * rather than used. Nothing is decided by it — the project's checkout wins
 * either way — so a conservative answer costs a sentence, never a tree.
 */
function isInside(candidate: string, root: string): boolean {
  const base = trimTrailingSeparator(root);
  const inner = trimTrailingSeparator(candidate);
  if (base.length === 0) return false;
  return inner === base || inner.startsWith(`${base}/`);
}

function trimTrailingSeparator(path: string): string {
  let end = path.length;
  while (end > 1 && path[end - 1] === "/") end -= 1;
  return path.slice(0, end);
}
