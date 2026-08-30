/**
 * Where a new coding session will live: the project it belongs to, or the
 * channel that will carry its signed transcript.
 *
 * Split out of `NewCodingSessionDialog.tsx` when that file crossed the
 * 1000-line ceiling. These two are pure presentation over props and have no
 * dependency on the dialog's form state, so they are the honest seam.
 */

import { FolderKanban } from "lucide-react";

/**
 * Where a project-scoped session will live, stated rather than asked. The
 * transport channel carrying the transcript is deliberately never named:
 * members reach sessions through the project, so surfacing the channel would
 * only advertise plumbing they cannot (and should not) interact with.
 */
export function NewCodingSessionProjectDestination({
  projectName,
}: {
  projectName: string;
}) {
  return (
    <div
      className="flex flex-col gap-1 rounded-lg border border-border/60 bg-muted/30 px-3 py-2.5"
      data-testid="new-coding-session-project-destination"
    >
      <p className="flex items-center gap-2 text-sm font-medium">
        <FolderKanban className="size-4 shrink-0 text-muted-foreground" />
        <span className="truncate">{projectName}</span>
      </p>
      <p className="text-2xs text-muted-foreground">
        The session and its signed transcript live in this project, visible to
        project members.
      </p>
    </div>
  );
}

export function NewCodingSessionChannelPicker({
  channels,
  disabled,
  onChange,
  value,
}: {
  channels: ReadonlyArray<{ id: string; name: string }>;
  disabled: boolean;
  onChange: (channelId: string) => void;
  value: string | null;
}) {
  return (
    <div className="flex flex-col gap-2">
      <label
        className="text-xs font-medium text-muted-foreground"
        htmlFor="coding-session-channel"
      >
        Channel
      </label>
      <select
        className="h-9 rounded-md border border-input bg-transparent px-3 text-sm disabled:opacity-50"
        data-testid="new-coding-session-channel"
        disabled={disabled}
        id="coding-session-channel"
        onChange={(event) => onChange(event.target.value)}
        value={value ?? ""}
      >
        {channels.length === 0 ? (
          <option value="">No channels available</option>
        ) : null}
        {channels.map((channel) => (
          <option key={channel.id} value={channel.id}>
            #{channel.name}
          </option>
        ))}
      </select>
      <p className="text-2xs text-muted-foreground">
        The session's signed transcript lives here, visible to this channel's
        members.
      </p>
    </div>
  );
}
