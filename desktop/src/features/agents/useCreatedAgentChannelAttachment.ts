import { toast } from "sonner";

import { attachManagedAgentToChannel } from "./channelAgents";
import type { Channel, CreateManagedAgentResponse } from "@/shared/api/types";

type TargetChannel = Pick<Channel, "id" | "name">;

async function attach(
  created: CreateManagedAgentResponse,
  targetChannel: TargetChannel,
) {
  const attached = await attachManagedAgentToChannel(targetChannel.id, {
    agent: created.agent,
    role: "bot",
    ensureRunning: true,
  });
  created.agent = attached.agent;
}

function showAttachmentFailure(
  created: CreateManagedAgentResponse,
  targetChannel: TargetChannel,
  cause: unknown,
  toastId?: string | number,
) {
  const error = cause instanceof Error ? cause.message : "Failed to add agent.";
  const id = toast.warning("Agent created", {
    description: `${created.agent.name} couldn’t be added to #${targetChannel.name}. ${error}`,
    id: toastId,
    // The retry is the only remedy offered for an agent that did not join its
    // channel, and on sonner's default four-second timer it withdrew itself
    // whether or not anyone had read it — so whether the user could act on
    // their own failure depended on how busy the machine was. A toast carrying
    // an action waits for the person; the plain success toast still does not.
    duration: Number.POSITIVE_INFINITY,
    action: {
      label: "Try again",
      onClick: (event) => {
        event.preventDefault();
        toast.loading("Agent created", {
          description: `Adding ${created.agent.name} to #${targetChannel.name}…`,
          id,
        });
        void attach(created, targetChannel).then(
          () => {
            // Replaced rather than updated in place: updating would carry the
            // withheld `Try again` action and its unbounded duration onto a
            // toast that has nothing left to retry, offering a remedy for a
            // problem that is over.
            toast.dismiss(id);
            toast.success("Agent created", {
              description: `Added ${created.agent.name} to #${targetChannel.name}`,
            });
          },
          (retryCause: unknown) => {
            showAttachmentFailure(created, targetChannel, retryCause, id);
          },
        );
      },
    },
  });
}

/** Keeps creation successful when its optional channel attachment fails. */
export function useCreatedAgentChannelAttachment() {
  async function presentCreatedAgent(
    created: CreateManagedAgentResponse,
    targetChannel?: TargetChannel | null,
  ) {
    if (created.spawnError || !targetChannel) {
      toast.success("Agent created");
      return;
    }

    try {
      await attach(created, targetChannel);
      toast.success("Agent created");
    } catch (cause) {
      showAttachmentFailure(created, targetChannel, cause);
    }
  }

  return { presentCreatedAgent };
}
