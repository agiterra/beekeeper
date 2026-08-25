import { toast } from "sonner";

import {
  enableGitTerminalAccess,
  type GitTerminalAccessStatus,
  gitTerminalAccessStatus,
} from "@/shared/api/gitTerminalAccess";

/**
 * Offer terminal git access after an import has wired up a Bee Keeper remote.
 *
 * The import writes a remote into `.git/config` that only the app can
 * authenticate to — its git credentials are ephemeral and env-only, so they do
 * not survive into a terminal. Without this prompt the user is left with a
 * remote whose first `git push` fails at a username prompt that can never be
 * satisfied, and nothing on screen connects the two.
 *
 * Deliberately an offer, never automatic: accepting writes the identity key to
 * a file, and the toast names that file before the user agrees to it.
 */
export async function offerTerminalGitAccess(): Promise<void> {
  let status: GitTerminalAccessStatus;
  try {
    status = await gitTerminalAccessStatus();
  } catch {
    // Best-effort. A failure to *check* must not make a successful import look
    // like a failed one.
    return;
  }
  if (status.configured) return;

  const keyfile = status.keyfile ?? "your home directory";
  toast("Terminal git access isn't set up", {
    description:
      `Bee Keeper can push to this repo, but \`git push\` from a terminal ` +
      `can't authenticate yet. Enabling writes your identity key to ` +
      `${keyfile}, readable only by you.`,
    duration: 20_000,
    action: {
      label: "Enable",
      onClick: () => {
        void enableGitTerminalAccess()
          .then((next) => {
            if (next.configured) {
              // Deliberately not "ready" — this says the local config is
              // complete, which is all the app can know without asking the
              // relay. `bee git check` is what answers the rest.
              toast.success(
                `Terminal git access configured (${next.keyfile}). ` +
                  `Run \`bee git check\` to confirm the relay accepts this key.`,
              );
            } else {
              // Say which half is missing rather than claiming success.
              toast.error(
                next.keyfile_problem ??
                  "Configured, but the key file is still missing — pushes will fail.",
              );
            }
          })
          .catch((error: unknown) => {
            toast.error(
              error instanceof Error
                ? error.message
                : "Could not enable terminal git access.",
            );
          });
      },
    },
  });
}
