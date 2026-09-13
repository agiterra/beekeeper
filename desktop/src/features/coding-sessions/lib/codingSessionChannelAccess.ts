import type { Channel } from "@/shared/api/types";
import type { EntityRole } from "@/shared/lib/entityRoles";

/**
 * Whether the signed-in identity may write coding-session events (and the
 * session lane's messages) into the channel a session lives in.
 *
 * The relay's rule, mirrored — it decides, this only predicts
 * (`crates/buzz-relay/src/handlers/ingest.rs`, `check_coding_session_membership`
 * and the ordinary channel write gate):
 *
 * 1. A `channel_members` row admits.
 * 2. Otherwise, on a **session-transport** channel only, the project gate
 *    admits the project's creator (the pubkey in its `30621:<owner>:<d>`
 *    address) and roster members whose role is `owner` or `collaborator`.
 *    The gate joins the project ACL on the channel's `project_ref` exactly
 *    (`get_channel_transport_gate`), so a deleted or unknown project — or an
 *    address whose owner is not lowercase hex — has no gate at all.
 *    A project `viewer` reads the transport and never writes. A per-session
 *    invitee reads the transport through the session grant, which is a read
 *    scope, not transport write access.
 * 3. Anything else — an ordinary channel, a transport with no project — is
 *    the strict membership denial.
 *
 * Only `member` and `project-writer` allow sending. Every other state fails
 * closed, and `unresolved` is its own state so the copy never tells a project
 * owner to "join this channel" while their roster is still loading.
 *
 * Membership here is channel write access, not steering authority: the
 * founder / operator-grant check is separate and stays separate.
 */
export type CodingSessionChannelAccess =
  | { kind: "member" }
  | { kind: "project-writer"; role: "owner" | "collaborator" }
  | {
      kind: "project-reader";
      /** `viewer` on the roster, or `null` when not on the roster at all. */
      projectRole: "viewer" | null;
    }
  | {
      kind: "unresolved";
      /**
       * `list-truncated`: the project list came back at its fetch limit and
       * this project is not in it, so its absence proves nothing.
       */
      reason: "loading" | "error" | "list-truncated";
    }
  | { kind: "not-member" };

/** A read that is still in flight, failed, or answered. */
export type CodingSessionAccessReadStatus = "loading" | "error" | "ready";

export type CodingSessionChannelAccessInput = {
  /** The session's channel as the channel list has it, or null if absent. */
  channel: Pick<Channel, "channelType" | "isMember" | "projectRef"> | null;
  channelsStatus: CodingSessionAccessReadStatus;
  /** The signed-in identity; null while it is unknown. */
  currentUserPubkey: string | null;
  /**
   * The project head, looked up in the loaded project list by an address
   * equal to the channel's `project_ref` — byte for byte, as the relay joins.
   */
  project: {
    status: CodingSessionAccessReadStatus;
    found: boolean;
    /** The list reached its fetch limit; see {@link PROJECT_LIST_FETCH_LIMIT}. */
    truncated: boolean;
  };
  /**
   * The transport's project roster (owner / collaborator / viewer rows). The
   * project creator is never on it; it is read from the address instead.
   */
  roster: {
    status: CodingSessionAccessReadStatus;
    members: readonly { pubkey: string; role: EntityRole }[];
  };
};

const HEX64 = /^[0-9a-f]{64}$/;

/**
 * The project list's relay fetch limit (`fetchProjectContainers`,
 * `features/projects-container/hooks.ts`). A list this long may be cut off, so
 * a project missing from it is not known to be missing. The list is deduped
 * and deletion-filtered after the fetch, so this detects a truncated list
 * only when enough heads survive to reach it.
 */
export const PROJECT_LIST_FETCH_LIMIT = 200;

/**
 * The creator pubkey of a `30621:<owner>:<d>` project address, or null.
 *
 * Not normalized: the relay matches the channel's `project_ref` exactly, and
 * an address spelled with uppercase hex resolves to no project there.
 */
export function projectCreatorFromAddress(
  address: string | null | undefined,
): string | null {
  if (!address) return null;
  const [kind, owner, ...rest] = address.split(":");
  if (kind !== "30621" || rest.length === 0 || owner === undefined) {
    return null;
  }
  return HEX64.test(owner) ? owner : null;
}

/** Resolve channel write access; see {@link CodingSessionChannelAccess}. */
export function resolveCodingSessionChannelAccess(
  input: CodingSessionChannelAccessInput,
): CodingSessionChannelAccess {
  const { channel } = input;
  if (channel === null) {
    if (input.channelsStatus === "ready") return { kind: "not-member" };
    return {
      kind: "unresolved",
      reason: input.channelsStatus === "error" ? "error" : "loading",
    };
  }
  if (channel.isMember) return { kind: "member" };
  if (channel.channelType !== "transport") return { kind: "not-member" };
  // A transport with no (well-formed) project has no gate: strict membership.
  const creator = projectCreatorFromAddress(channel.projectRef);
  if (creator === null) return { kind: "not-member" };

  const self = input.currentUserPubkey?.toLowerCase() ?? null;
  if (self === null) return { kind: "unresolved", reason: "loading" };

  if (input.project.status !== "ready") {
    return {
      kind: "unresolved",
      reason: input.project.status === "error" ? "error" : "loading",
    };
  }
  if (!input.project.found) {
    if (input.project.truncated) {
      return { kind: "unresolved", reason: "list-truncated" };
    }
    // A creator can always read their own head, so its absence from a
    // complete list means the project is gone and the relay has no gate.
    if (self === creator) return { kind: "not-member" };
    // Anyone else may simply not be allowed to see it; either way they are
    // no project writer.
    return { kind: "project-reader", projectRole: null };
  }
  if (self === creator) return { kind: "project-writer", role: "owner" };

  if (input.roster.status !== "ready") {
    return {
      kind: "unresolved",
      reason: input.roster.status === "error" ? "error" : "loading",
    };
  }
  const row = input.roster.members.find(
    (member) => member.pubkey.toLowerCase() === self,
  );
  if (row?.role === "owner" || row?.role === "collaborator") {
    return { kind: "project-writer", role: row.role };
  }
  return {
    kind: "project-reader",
    projectRole: row?.role === "viewer" ? "viewer" : null,
  };
}

/** Only an explicit member or a project writer may send. */
export function codingSessionChannelAccessAllowsSend(
  access: CodingSessionChannelAccess,
): boolean {
  return access.kind === "member" || access.kind === "project-writer";
}

/** The sentences each refused state uses; null where sending is allowed. */
export type CodingSessionChannelAccessCopy = {
  /** The deck's access chip. */
  label: string;
  /** The editor's placeholder. */
  placeholder: string;
  /** The deck popover's explanation. */
  explanation: string;
  /** Why Reconnect is off. */
  reconnect: string;
  /** Why Stop execution is off. */
  stop: string;
  /** The compact composer's notice above the editor. */
  notice: string;
};

export function describeCodingSessionChannelAccess(
  access: CodingSessionChannelAccess,
): CodingSessionChannelAccessCopy | null {
  switch (access.kind) {
    case "member":
    case "project-writer":
      return null;
    case "not-member":
      return {
        label: "View only",
        placeholder: "Join this channel to send a message.",
        explanation:
          "Join this channel, then ask the session owner for an operator grant if the provider requires one.",
        reconnect: "Join this channel to reconnect this execution.",
        stop: "Join this channel to stop this execution.",
        notice:
          "Join this channel for native control. Compatibility control also requires an allowlisted operator.",
      };
    case "project-reader": {
      const why =
        access.projectRole === "viewer"
          ? "your project role (Viewer) does not allow steering"
          : "you are not a project owner or collaborator, so your project role does not allow steering";
      return {
        label: "Read only",
        placeholder: `You can read this project session, but ${why}.`,
        explanation: `You can read this project session, but ${why}. A project owner can make you a collaborator.`,
        reconnect:
          "Your project role does not allow reconnecting this execution.",
        stop: "Your project role does not allow stopping this execution.",
        notice: `You can read this project session, but ${why}.`,
      };
    }
    case "unresolved":
      if (access.reason === "list-truncated") {
        return {
          label: "Access unknown",
          placeholder:
            "Couldn't confirm your project role from the loaded project list.",
          explanation:
            "This project is not in the loaded project list, and that list reached its size limit, so your project role could not be confirmed. Controls stay off rather than guess.",
          reconnect:
            "Couldn't confirm your project role, so this execution cannot be reconnected from here.",
          stop: "Couldn't confirm your project role, so this execution cannot be stopped from here.",
          notice:
            "Couldn't confirm your project role from the loaded project list.",
        };
      }
      return access.reason === "error"
        ? {
            label: "Access unknown",
            placeholder:
              "Could not check your access to this session. Controls stay off until it can be read.",
            explanation:
              "The channel list or the project roster could not be read, so whether this identity may write here is unknown. Controls stay off rather than guess.",
            reconnect: "Your access to this session could not be checked.",
            stop: "Your access to this session could not be checked.",
            notice: "Could not check your access to this session.",
          }
        : {
            label: "Checking access",
            placeholder: "Checking your access to this session…",
            explanation:
              "Checking this identity's channel membership and project role. Controls stay off until the answer arrives.",
            reconnect:
              "Checking your access before this execution can be reconnected.",
            stop: "Checking your access before this execution can be stopped.",
            notice: "Checking your access to this session…",
          };
  }
}
