import { ImageIcon, Paperclip, X } from "lucide-react";

import type { CodingSessionAttachmentController } from "@/features/coding-sessions/lib/useCodingSessionImageAttachments";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { Spinner } from "@/shared/ui/spinner";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/shared/ui/tooltip";

type CodingSessionComposerAttachmentsProps = {
  /** Staged attachments and every entry point that adds to them. */
  controller: CodingSessionAttachmentController;
  /**
   * Whether this execution's runtime advertised image prompts (its 44223
   * `capabilities.promptImage`). `false` disables the control and says why
   * rather than hiding it: an operator who cannot find the button learns
   * nothing, and one who is told learns which runtime to use instead.
   */
  canAttach: boolean;
  /** Named in the disabled tooltip, so the reason is about *this* session. */
  runtimeLabel?: string | null;
  disabled: boolean;
};

/** Attach control plus the thumbnail strip for a coding-session turn. */
export function CodingSessionComposerAttachments({
  canAttach,
  controller,
  disabled,
  runtimeLabel,
}: CodingSessionComposerAttachmentsProps) {
  const { attachments, error } = controller;
  const atCapacity = !controller.isUploading && attachments.length >= 4;
  const attachDisabled = disabled || !canAttach || atCapacity;

  return (
    <div
      className="flex flex-col gap-2 px-3 pt-2"
      data-testid="coding-session-composer-attachments"
    >
      {attachments.length > 0 ? (
        <ul className="flex flex-wrap gap-2">
          {attachments.map((attachment) => (
            <li
              className={cn(
                "group relative h-16 w-16 overflow-hidden rounded-lg border",
                attachment.error ? "border-destructive/60" : "border-border/60",
              )}
              key={attachment.id}
            >
              <img
                alt={attachment.filename}
                className="h-full w-full object-cover"
                src={attachment.previewUrl}
              />
              {attachment.error ? (
                <span
                  className="absolute inset-0 flex items-center justify-center bg-destructive/25 text-2xs text-foreground"
                  title={attachment.error}
                >
                  Failed
                </span>
              ) : attachment.sha256 === undefined ? (
                <span className="absolute inset-0 flex items-center justify-center bg-background/60">
                  <Spinner className="h-4 w-4" />
                </span>
              ) : null}
              <button
                aria-label={`Remove ${attachment.filename}`}
                className="absolute top-0.5 right-0.5 rounded-full bg-background/85 p-0.5 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
                data-testid="coding-session-attachment-remove"
                onClick={() => controller.remove(attachment.id)}
                type="button"
              >
                <X aria-hidden="true" className="h-3 w-3" />
              </button>
            </li>
          ))}
        </ul>
      ) : null}

      <div className="flex items-center gap-2">
        <TooltipProvider>
          <Tooltip>
            <TooltipTrigger asChild>
              {/* A disabled button fires no pointer events, so the tooltip
                needs a wrapper that still receives them — otherwise the one
                state that most needs an explanation is the one that cannot
                show it. */}
              <span className="inline-flex">
                <Button
                  aria-label="Attach an image"
                  className="h-7 gap-1.5 px-2 text-2xs text-muted-foreground"
                  data-testid="coding-session-composer-attach"
                  disabled={attachDisabled}
                  onClick={controller.pick}
                  size="sm"
                  type="button"
                  variant="ghost"
                >
                  <Paperclip aria-hidden="true" className="h-3.5 w-3.5" />
                  Image
                </Button>
              </span>
            </TooltipTrigger>
            <TooltipContent className="max-w-72" side="top">
              {canAttach ? (
                atCapacity ? (
                  "A turn can carry at most 4 images."
                ) : (
                  <>
                    Attach a PNG, JPEG, GIF or WebP — or paste and drop one
                    straight into the composer.
                  </>
                )
              ) : (
                <>
                  {runtimeLabel
                    ? `${runtimeLabel} on this execution`
                    : "This execution's runtime"}{" "}
                  did not advertise image prompts, so an attached image would
                  never reach the agent.
                </>
              )}
            </TooltipContent>
          </Tooltip>
        </TooltipProvider>

        {controller.isUploading ? (
          <span className="inline-flex items-center gap-1.5 text-2xs text-muted-foreground">
            <Spinner className="h-3 w-3" />
            Uploading…
          </span>
        ) : null}

        {error ? (
          <span
            className="inline-flex items-center gap-1.5 text-2xs text-destructive"
            data-testid="coding-session-attachment-error"
          >
            <ImageIcon aria-hidden="true" className="h-3 w-3" />
            {error}
          </span>
        ) : null}
      </div>
    </div>
  );
}
