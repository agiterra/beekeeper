/**
 * The Planning contract: what the Planning surface reads and writes, in
 * terms no provider owns.
 *
 * A provider is where a project's issues actually live. Native Beekeeper
 * planning keeps them as NIP-TD to-do items (kind 44248) on the relay;
 * Linear, once a project can be connected to it, keeps them in Linear and
 * Beekeeper operates them through Linear's API. The UI reads and writes only
 * through this contract, so it never holds a provider's credentials and
 * never keeps a second, independently editable copy of a provider's records.
 *
 * Every id here is the provider's own — issue, container, status, priority
 * and person. A status carries its provider id (`in_progress` natively, a
 * workflow-state UUID in Linear) and a category the UI groups and colours
 * by; a priority likewise carries a level; neither is ever written back.
 */

/** The providers this build knows about. */
export type PlanningProviderId = "native" | "linear";

/**
 * What a status means, independent of its provider's name for it. These are
 * Linear's workflow-state types, which the native vocabulary maps onto one
 * to one.
 */
export type PlanningStatusCategory =
  | "triage"
  | "backlog"
  | "unstarted"
  | "started"
  | "completed"
  | "canceled";

export type PlanningStatus = {
  /** The provider's status id. */
  id: string;
  label: string;
  category: PlanningStatusCategory;
};

/**
 * What a priority means, independent of its provider's id for it: Linear's
 * five priority values, which the native vocabulary maps onto one to one.
 * The UI draws glyphs and colours from this, never from a provider's id.
 */
export type PlanningPriorityLevel =
  | "urgent"
  | "high"
  | "medium"
  | "low"
  | "none";

export type PlanningPriority = {
  /** The provider's priority id. */
  id: string;
  label: string;
  level: PlanningPriorityLevel;
  /** Sort position: 0 is most urgent; "no priority" sorts last. */
  order: number;
};

/**
 * A person as a provider names them. Natively the id *is* a Beekeeper
 * pubkey; in Linear it is a Linear user id, and `pubkey` is the Beekeeper
 * identity it is linked to, when a link is known — never guessed from a
 * name or an email.
 */
export type PlanningPerson = {
  /** The provider's id for the person. */
  id: string;
  /** Their Beekeeper pubkey, when the provider knows it. */
  pubkey: string | null;
  /** The provider's display name, for a person with no Beekeeper profile. */
  name: string | null;
};

/** Where an issue belongs inside its provider (a native list; a Linear team). */
export type PlanningContainer = {
  id: string;
  title: string;
};

export type PlanningIssue = {
  provider: PlanningProviderId;
  /** Unique within the provider and project: `<containerId>/<issueId>`. */
  key: string;
  containerId: string;
  /** The provider's issue id. */
  issueId: string;
  title: string;
  description: string | null;
  status: PlanningStatus;
  priority: PlanningPriority;
  /**
   * The assignee's provider person id (see {@link PlanningPerson}), if any.
   * Natively a Beekeeper pubkey.
   */
  assignee: string | null;
  /** `YYYY-MM-DD`, if any. */
  due: string | null;
  /** Manual order inside the container. */
  rank: string;
  createdAt: number;
  /** The creator's provider person id. Natively a Beekeeper pubkey. */
  createdBy: string;
  updatedAt: number;
  /** When the issue entered a closed status, if it is in one. */
  completedAt: number | null;
};

/** What the viewer may do here, decided before a control is drawn. */
export type PlanningAccess =
  | { kind: "loading" }
  | { kind: "writable" }
  | { kind: "read-only"; reason: string };

/** A read of one project's issues from one provider. */
export type PlanningRead = {
  provider: PlanningProviderId;
  issues: PlanningIssue[];
  containers: PlanningContainer[];
  /** Older history was not read; the list may be missing issues. */
  truncated: boolean;
  /** Records the provider returned that this build could not interpret. */
  unreadable: number;
  /**
   * The people the issues name, for a provider whose person ids are its
   * own. Empty natively, where every person id is already a pubkey.
   */
  people: readonly PlanningPerson[];
};

export type PlanningState =
  | { kind: "loading" }
  | { kind: "ready"; read: PlanningRead; refreshing: boolean }
  /** The latest read failed; `read` is the last good one, if there was one. */
  | { kind: "error"; read: PlanningRead | null; message: string };

export type PlanningCreateInput = {
  title: string;
  description: string | null;
  statusId: string;
  priorityId: string;
  assignee: string | null;
  /** An existing container, or `null` to create the provider's default one. */
  containerId: string | null;
};

/** The outcome of a create: the issue exists; some fields may not have saved. */
export type PlanningCreateResult = {
  key: string;
  /** Fields that failed after the issue itself was created. */
  failed: { field: string; message: string }[];
};

/**
 * Writes. Each resolves once the provider has accepted the change; one that
 * landed only in part rejects with a `PlanningPartialWriteError`.
 */
export type PlanningMutations = {
  createIssue: (input: PlanningCreateInput) => Promise<PlanningCreateResult>;
  setTitle: (issue: PlanningIssue, title: string) => Promise<void>;
  setDescription: (
    issue: PlanningIssue,
    description: string | null,
  ) => Promise<void>;
  setStatus: (issue: PlanningIssue, statusId: string) => Promise<void>;
  setPriority: (issue: PlanningIssue, priorityId: string) => Promise<void>;
  setAssignee: (issue: PlanningIssue, assignee: string | null) => Promise<void>;
};

/** What a provider can do at all, independent of the viewer's access. */
export type PlanningCapabilities = {
  statuses: readonly PlanningStatus[];
  priorities: readonly PlanningPriority[];
  /** Whether issues carry a comment thread this build can read and write. */
  comments: boolean;
  /** Whether a create may name no container and get the default one. */
  createsDefaultContainer: boolean;
  /**
   * What a person id is: a Beekeeper pubkey (native), or the provider's own
   * id, resolved through {@link PlanningRead.people}. A provider of the
   * second kind also needs an assignee picker over its own directory, which
   * this build does not have — one more reason Linear is unavailable.
   */
  personIds: "beekeeper-pubkey" | "provider";
  /** Byte limits the provider enforces, for honest counters. */
  limits: { titleBytes: number; descriptionBytes: number };
};

export type PlanningProviderAvailability =
  | { kind: "available" }
  | {
      kind: "unavailable";
      /** One sentence a person can act on. */
      reason: string;
    };

export type PlanningProviderDescriptor = {
  id: PlanningProviderId;
  label: string;
  description: string;
  availability: PlanningProviderAvailability;
};
