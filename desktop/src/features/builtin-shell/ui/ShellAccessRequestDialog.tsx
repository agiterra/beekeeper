import { FolderGit2, ShieldQuestion, Terminal } from "lucide-react";

import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";

import { useShellAccessRequests } from "../hooks/useShellAccessRequests";

/**
 * App-wide approval prompt for agent access to a built-in shell session. When
 * an agent runs `buzz session request-access` on a session it isn't a
 * collaborator on, its call blocks and this dialog surfaces the ask. The
 * owner can allow the single command once, enable full control (which adds
 * the agent's pubkey to the session's invite roster as collaborator), or
 * deny — the decision wakes the agent.
 *
 * The oldest pending request is shown; answering it reveals the next.
 */
export function ShellAccessRequestDialog() {
  const { requests, resolve } = useShellAccessRequests();
  const current = requests[0];

  if (!current) return null;

  const hasCommand = Boolean(current.command);

  return (
    <Dialog open onOpenChange={() => {}}>
      <DialogContent
        className="sm:max-w-md"
        data-testid="shell-access-request-dialog"
        onInteractOutside={(e) => e.preventDefault()}
        onEscapeKeyDown={(e) => e.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ShieldQuestion className="size-5 text-amber-500" />
            Agent access request
          </DialogTitle>
          <DialogDescription>
            {hasCommand
              ? "An agent is asking to run a command in a shell session it doesn't yet control."
              : "An agent is asking to control a shell session."}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-3">
          <div className="flex items-center gap-2 text-sm">
            <Terminal className="size-4 shrink-0 text-muted-foreground" />
            <span className="font-medium">
              {current.sessionTitle ?? "Shell session"}
            </span>
          </div>

          {current.command ? (
            <div>
              <p className="mb-1 text-2xs uppercase tracking-wide text-muted-foreground">
                Command
              </p>
              <pre className="overflow-x-auto rounded-md bg-muted px-3 py-2 font-mono text-xs">
                {current.command}
              </pre>
            </div>
          ) : null}

          {current.reason ? (
            <div className="flex items-start gap-2 text-2xs text-muted-foreground">
              <FolderGit2 className="mt-0.5 size-3 shrink-0" />
              <span>{current.reason}</span>
            </div>
          ) : null}

          {requests.length > 1 ? (
            <p className="text-2xs text-muted-foreground">
              {requests.length - 1} more request
              {requests.length - 1 === 1 ? "" : "s"} waiting.
            </p>
          ) : null}
        </div>

        <DialogFooter className="gap-2 sm:justify-between">
          <Button
            type="button"
            variant="ghost"
            onClick={() => resolve(current.id, "deny")}
            data-testid="shell-access-deny"
          >
            Deny
          </Button>
          <div className="flex gap-2">
            {hasCommand ? (
              <Button
                type="button"
                variant="outline"
                onClick={() => resolve(current.id, "once")}
                data-testid="shell-access-once"
              >
                Allow once
              </Button>
            ) : null}
            <Button
              type="button"
              onClick={() => resolve(current.id, "full")}
              data-testid="shell-access-full"
            >
              Enable full control
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
