import { Pencil, Save, X } from "lucide-react";
import * as React from "react";

import { DiffViewer } from "@/features/messages/ui/DiffViewer";
import { formatRelativeTime } from "@/features/forum/lib/time";
import type { AgentsRepoFile } from "@/shared/api/agentsRepoTypes";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { Markdown } from "@/shared/ui/markdown";
import { Textarea } from "@/shared/ui/textarea";

import type { AgentsRepoAccess } from "../lib/agentsRepoAccess";
import { agentsRepoCopy as copy } from "../lib/agentsRepoCopy";
import { draftPatch } from "../lib/agentsRepoDiff";
import type { DraftPath } from "../lib/agentsRepoDraftFold";
import { archiveCounterpart, draftPathClass } from "../lib/agentsRepoDraftOp";
import { isDraftablePath, isMarkdownPath } from "../lib/agentsRepoPaths";

export type EditorTab = "edit" | "preview" | "diff";

/**
 * What the editor knows about the file: `main`'s text (or why not), the open
 * draft head (if any), and the tip the listing was read at.
 */
export type EditorSubject = {
  path: string;
  main: AgentsRepoFile | null;
  mainError: string | null;
  draft: DraftPath | null;
  /** The `main` tip the listing was read at. */
  tip: string | null;
};

function short(sha: string | null): string {
  return sha ? sha.slice(0, 8) : "?";
}

/** The text the editor opens on: the draft head's, else main's. */
export function editorInitialText(subject: EditorSubject): string {
  const head = subject.draft?.head;
  if (head) {
    if (head.op === "file.put") return head.text ?? "";
    if (head.op === "file.move" && head.to === subject.path)
      return subject.main?.text ?? "";
    return "";
  }
  return subject.main?.text ?? "";
}

/** The disclosure strip's facts, computed once per subject. */
export function editorDisclosures(
  subject: EditorSubject,
  personName: (pubkey: string) => string,
  identicalToMain: boolean,
): { tone: "info" | "warn"; text: string; testId: string }[] {
  const out: { tone: "info" | "warn"; text: string; testId: string }[] = [];
  const head = subject.draft?.head;
  if (head) {
    out.push({
      tone: "info",
      text: copy.draftBy(
        personName(head.author),
        formatRelativeTime(head.createdAt),
      ),
      testId: "agents-repo-disclosure-draft",
    });
    if (subject.draft && subject.draft.superseded.length > 0) {
      out.push({
        tone: "info",
        text: copy.supersededCount(subject.draft.superseded.length),
        testId: "agents-repo-disclosure-superseded",
      });
    }
    if (subject.draft?.diverged) {
      out.push({
        tone: "warn",
        text: copy.diverged,
        testId: "agents-repo-disclosure-diverged",
      });
    }
    const mainBlob = subject.main?.blob ?? null;
    if (head.base !== mainBlob) {
      out.push({
        tone: "warn",
        text: copy.baseChanged,
        testId: "agents-repo-disclosure-base-changed",
      });
    } else if (
      head.baseCommit &&
      subject.tip &&
      head.baseCommit !== subject.tip
    ) {
      out.push({
        tone: "info",
        text: copy.mainMovedSince(short(head.baseCommit), short(subject.tip)),
        testId: "agents-repo-disclosure-main-moved",
      });
    }
    if (identicalToMain) {
      out.push({
        tone: "info",
        text: copy.identicalToMain,
        testId: "agents-repo-disclosure-identical",
      });
    }
  }
  return out;
}

export function AgentsRepoEditor({
  subject,
  access,
  personName,
  identicalToMain,
  onSave,
  onArchive,
  onDelete,
  onWithdraw,
  isSelf,
  busy,
}: {
  subject: EditorSubject;
  access: AgentsRepoAccess;
  personName: (pubkey: string) => string;
  identicalToMain: boolean;
  /** `openedOn` is the head this editor opened on — the `prev` a save must carry; the mutation refuses when it is no longer the head. */
  onSave: (
    text: string,
    message: string | null,
    openedOn: string | null,
  ) => Promise<void>;
  onArchive: (message: string | null, openedOn: string | null) => Promise<void>;
  onDelete: (message: string | null, openedOn: string | null) => Promise<void>;
  onWithdraw: (draftId: string) => Promise<void>;
  isSelf: (pubkey: string) => boolean;
  busy: boolean;
}) {
  // A path that is neither on main nor drafted (a plan just named in the
  // New plan dialog) has nothing to preview: it opens straight into editing.
  const startsBlank =
    subject.draft === null &&
    (subject.main === null || subject.main.state === "not-on-main") &&
    access.kind === "writable" &&
    isDraftablePath(subject.path);
  const [tab, setTab] = React.useState<EditorTab>(
    startsBlank ? "edit" : "preview",
  );
  const [editing, setEditing] = React.useState(startsBlank);
  const [text, setText] = React.useState(() => editorInitialText(subject));
  const [note, setNote] = React.useState("");
  const [error, setError] = React.useState<string | null>(null);
  const openedOn = React.useRef<string | null>(subject.draft?.head.id ?? null);

  // A new subject (another file, or a newer head arriving while not editing)
  // resets the text; while editing, the person's text is kept and the
  // disclosure strip says a newer draft exists.
  React.useEffect(() => {
    if (editing) return;
    setText(editorInitialText(subject));
    openedOn.current = subject.draft?.head.id ?? null;
    setError(null);
  }, [editing, subject]);

  const canWrite = access.kind === "writable" && isDraftablePath(subject.path);
  const isMarkdown = isMarkdownPath(subject.path);
  const mainText = subject.main?.text ?? null;
  const draftText = subject.draft?.head.op === "file.delete" ? null : text;
  const removedByDraft =
    subject.draft?.head.op === "file.delete" ||
    (subject.draft?.head.op === "file.move" &&
      subject.draft.head.path === subject.path);
  const newerHeadThanOpened =
    (subject.draft?.head.id ?? null) !== openedOn.current;
  const disclosures = editorDisclosures(subject, personName, identicalToMain);
  const counterpart = archiveCounterpart(subject.path);
  const classified = draftPathClass(subject.path);
  const isArchived =
    classified.ok &&
    (classified.class === "archived-role" ||
      classified.class === "archived-plan");

  const submit = async () => {
    setError(null);
    try {
      await onSave(text, note.trim() ? note.trim() : null, openedOn.current);
      setEditing(false);
      setNote("");
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    }
  };

  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-3"
      data-testid="agents-repo-editor"
    >
      <div className="flex flex-wrap items-center gap-2">
        <h2
          className="min-w-0 flex-1 truncate font-mono text-sm text-foreground"
          title={subject.path}
        >
          {subject.path}
        </h2>
        {subject.main?.state === "not-on-main" || subject.main === null ? (
          <span className="text-2xs text-muted-foreground">
            {copy.notOnMain}
          </span>
        ) : null}
        <div className="flex gap-1" role="tablist">
          {(
            [
              ["preview", copy.preview],
              ["diff", copy.diff],
              ["edit", copy.editTab],
            ] as const
          ).map(([id, label]) => (
            <button
              aria-selected={tab === id}
              className={cn(
                "rounded-md px-2 py-1 text-xs",
                tab === id
                  ? "bg-accent text-accent-foreground"
                  : "text-muted-foreground hover:bg-muted/60",
              )}
              data-testid={`agents-repo-editor-tab-${id}`}
              key={id}
              onClick={() => setTab(id)}
              role="tab"
              type="button"
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      {disclosures.length > 0 ? (
        <ul className="space-y-1" data-testid="agents-repo-disclosures">
          {disclosures.map((d) => (
            <li
              className={cn(
                "rounded-md px-2 py-1 text-xs",
                d.tone === "warn"
                  ? "bg-amber-500/10 text-amber-800 dark:text-amber-200"
                  : "bg-muted/50 text-muted-foreground",
              )}
              data-testid={d.testId}
              key={d.testId}
            >
              {d.text}
            </li>
          ))}
        </ul>
      ) : null}
      {editing && newerHeadThanOpened ? (
        <p
          className="rounded-md bg-amber-500/10 px-2 py-1 text-xs text-amber-800 dark:text-amber-200"
          data-testid="agents-repo-newer-head"
        >
          {subject.draft
            ? `${personName(subject.draft.head.author)} saved a newer draft while you were editing; saving now will be refused until you reload.`
            : "The draft you were editing was committed or withdrawn; reload to start from what is there now."}
        </p>
      ) : null}
      {subject.mainError ? (
        <p
          className="rounded-md bg-destructive/10 px-2 py-1 text-xs text-destructive"
          data-testid="agents-repo-main-error"
        >
          {copy.mainUnreadable(subject.mainError)}
        </p>
      ) : null}
      {subject.main?.state === "too-large" ? (
        <p className="text-xs text-muted-foreground">{copy.fileTooLarge}</p>
      ) : null}
      {subject.main?.state === "not-text" ? (
        <p className="text-xs text-muted-foreground">{copy.fileNotText}</p>
      ) : null}

      {tab === "preview" ? (
        <div
          className="min-h-0 flex-1 overflow-auto rounded-xl border border-border/70 bg-muted/20 px-4 py-3"
          data-testid="agents-repo-preview"
        >
          {removedByDraft ? (
            <p className="text-sm text-muted-foreground">
              {subject.draft?.head.op === "file.move"
                ? `Draft moves this file to ${subject.draft.head.to}.`
                : "Draft removes this file."}
            </p>
          ) : isMarkdown ? (
            <Markdown content={text} />
          ) : (
            <pre className="whitespace-pre-wrap font-mono text-sm">{text}</pre>
          )}
        </div>
      ) : null}
      {tab === "diff" ? (
        <div
          className="min-h-0 flex-1 overflow-auto"
          data-testid="agents-repo-diff"
        >
          <DiffViewer
            content={draftPatch(subject.path, mainText, draftText)}
            fallbackFilePath={subject.path}
          />
        </div>
      ) : null}
      {tab === "edit" ? (
        <div className="flex min-h-0 flex-1 flex-col gap-2">
          <Textarea
            aria-label={`${subject.path} text`}
            className="min-h-64 flex-1 font-mono text-sm"
            data-testid="agents-repo-textarea"
            disabled={!editing || busy}
            onChange={(event) => setText(event.target.value)}
            value={text}
          />
          {editing ? (
            <>
              <input
                aria-label={copy.noteLabel}
                className="rounded-md border border-border bg-background px-2 py-1 text-sm"
                data-testid="agents-repo-note"
                maxLength={512}
                onChange={(event) => setNote(event.target.value)}
                placeholder={copy.noteLabel}
                value={note}
              />
              <div className="flex flex-wrap gap-2">
                <Button
                  data-testid="agents-repo-save"
                  disabled={busy}
                  onClick={() => void submit()}
                  size="sm"
                  type="button"
                >
                  <Save className="h-4 w-4" />
                  {copy.save}
                </Button>
                <Button
                  data-testid="agents-repo-discard"
                  disabled={busy}
                  onClick={() => {
                    setEditing(false);
                    setText(editorInitialText(subject));
                    openedOn.current = subject.draft?.head.id ?? null;
                    setError(null);
                  }}
                  size="sm"
                  type="button"
                  variant="outline"
                >
                  <X className="h-4 w-4" />
                  {copy.discard}
                </Button>
              </div>
            </>
          ) : canWrite ? (
            <div className="flex flex-wrap gap-2">
              <Button
                data-testid="agents-repo-edit"
                onClick={() => setEditing(true)}
                size="sm"
                type="button"
                variant="outline"
              >
                <Pencil className="h-4 w-4" />
                {copy.edit}
              </Button>
              {counterpart && subject.main?.state === "on-main" ? (
                <Button
                  data-testid="agents-repo-archive"
                  disabled={busy}
                  onClick={() => {
                    setError(null);
                    void onArchive(null, openedOn.current).catch((caught) =>
                      setError(
                        caught instanceof Error
                          ? caught.message
                          : String(caught),
                      ),
                    );
                  }}
                  size="sm"
                  type="button"
                  variant="outline"
                >
                  {isArchived ? copy.unarchive : copy.archive}
                </Button>
              ) : null}
              {subject.main?.state === "on-main" &&
              classified.ok &&
              classified.class !== "root-file" ? (
                <Button
                  data-testid="agents-repo-delete"
                  disabled={busy}
                  onClick={() => {
                    setError(null);
                    void onDelete(null, openedOn.current).catch((caught) =>
                      setError(
                        caught instanceof Error
                          ? caught.message
                          : String(caught),
                      ),
                    );
                  }}
                  size="sm"
                  type="button"
                  variant="outline"
                >
                  {copy.delete}
                </Button>
              ) : null}
              {subject.draft && isSelf(subject.draft.head.author) ? (
                <Button
                  data-testid="agents-repo-withdraw"
                  disabled={busy}
                  onClick={() => {
                    const id = subject.draft?.head.id;
                    if (!id) return;
                    setError(null);
                    void onWithdraw(id).catch((caught) =>
                      setError(
                        caught instanceof Error
                          ? caught.message
                          : String(caught),
                      ),
                    );
                  }}
                  size="sm"
                  type="button"
                  variant="outline"
                >
                  {copy.withdraw}
                </Button>
              ) : null}
            </div>
          ) : access.kind === "read-only" ? (
            <p
              className="text-xs text-muted-foreground"
              data-testid="agents-repo-read-only"
            >
              {copy.readOnly(access.reason)}
            </p>
          ) : null}
          {error ? (
            <p
              className="text-sm text-destructive"
              data-testid="agents-repo-editor-error"
            >
              {error}
            </p>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
