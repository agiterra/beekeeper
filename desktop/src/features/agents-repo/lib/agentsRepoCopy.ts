/** Every sentence the Files tab says, in one place. */
export const agentsRepoCopy = {
  tabLabel: "Files",
  subtitle:
    "The project's agents repository — plans, roles, skills, team and actions. Drafts are shared through the relay; nothing is real until someone commits it to main.",
  noSource:
    "This project has no agents repository yet. Finish repository setup under Project settings → Packs.",
  mainUnreadable: (reason: string) =>
    `Could not read the agents repository from the relay: ${reason}. Showing drafts only; a new draft cannot start until main is readable.`,
  refresh: "Refresh",
  asFetchedAt: (when: string) => `main as fetched at ${when}`,
  notOnMain: "not on main",
  newPlan: "New plan",
  newPlanHelp:
    "Lower-case letters, digits and dashes; the file lands at plans/<name>.md once you save a draft.",
  create: "Create",
  cancel: "Cancel",
  edit: "Edit",
  save: "Save draft",
  discard: "Discard",
  preview: "Preview",
  diff: "Diff vs main",
  editTab: "Edit",
  archive: "Archive",
  unarchive: "Put back in force",
  delete: "Delete",
  withdraw: "Withdraw",
  commit: "Commit…",
  commitTitle: "Commit drafts to main",
  commitMessage: "Commit message",
  noteLabel: "Why (one line, optional)",
  draftBy: (name: string, age: string) => `Draft by ${name}, ${age}`,
  supersededCount: (n: number) =>
    `${n} earlier draft${n === 1 ? "" : "s"} superseded`,
  diverged:
    "Two people saved from the same starting point; the newer save is the head and the other's text is not in it.",
  mainMovedSince: (from: string, to: string) =>
    `main moved since this draft was based (${from} → ${to}); the file itself is unchanged.`,
  baseChanged:
    "main changed this file since this draft was based on it — a commit will refuse it until the draft is re-applied to the current text.",
  identicalToMain: "identical to main",
  readOnly: (reason: string) => reason,
  mobileHint: "",
  pushedYes: (n: number, sha: string) =>
    `${n} file${n === 1 ? "" : "s"} committed as ${sha}.`,
  pushedNo: "Nothing was pushed.",
  pushedUnknown:
    "The push's result could not be confirmed; main may or may not have moved. Reload before retrying.",
  recordFailed: (sha: string) =>
    `Committed ${sha} to main, but the drafts could not be marked committed on the relay. Retry marking.`,
  retryMarking: "Retry marking",
  ignored: (n: number) =>
    `${n} draft op${n === 1 ? "" : "s"} could not be read and ${n === 1 ? "is" : "are"} not shown.`,
  otherRepo: (n: number) =>
    `${n} draft${n === 1 ? "" : "s"} belong to a repository this project no longer pins; kept, not shown.`,
  truncated:
    "The draft log was too long to read fully; older drafts may be missing.",
  fileTooLarge: "This file is larger than a draft can carry; edit it with git.",
  fileNotText: "This file is not text.",
  drafts: "Open drafts",
  noDrafts: "No open drafts.",
  commits: "Recent commits",
} as const;
