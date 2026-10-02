/** Every sentence the Artifacts tab says, in one place. */
export const agentsRepoCopy = {
  tabLabel: "Artifacts",
  subtitle:
    "The project's agents repository — plans, documents, roles, skills, team and actions. Drafts are shared through the relay; nothing is real until someone commits it to main.",
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
  newDocument: "New document",
  newDocumentHelp:
    "Letters, digits, dots, dashes and underscores. A document is Markdown or HTML and is checked against no schema — that is what makes it the place for notes, a mockup or a diagram rather than a work plan.",
  newDocumentFolderLabel: "Folder (optional)",
  newDocumentFolderHelp:
    "Nest it up to seven folders deep, e.g. mockups/login. Leave blank for the top of the tree.",
  newDocumentFormatLabel: "Format",
  newDocumentNameLabel: "Name",
  newFolder: "New folder",
  newFolderHelp:
    "Git has no empty directories, so the folder lands as its .gitkeep once you commit. Until then it is a draft like any other file.",
  create: "Create",
  cancel: "Cancel",
  edit: "Edit",
  save: "Save draft",
  discard: "Discard",
  preview: "Preview",
  planSourceDisclosure:
    "Plan source is shown and edited as plain text, byte for byte. Paste it from the file, not from a rendered view: a plan whose frontmatter does not read as beekeeper-plan/v1 is refused at commit.",
  diff: "Diff vs main",
  openPreview: "Open preview",
  htmlSourceDisclosure:
    "An HTML document is shown here as its source. Open preview runs it in its own window, where its scripts work but it has no network and no access to the app — so a mockup behaves as itself and can reach nothing.",
  previewMissing: (paths: readonly string[]) =>
    `The preview could not find ${paths.length === 1 ? "this file" : "these files"} the document references: ${paths.join(", ")}. ${paths.length === 1 ? "It" : "They"} will be missing from the preview until committed or drafted.`,
  previewFailed: (reason: string) => `Could not open the preview: ${reason}`,
  insertImage: "Insert image…",
  imageUploading: "Uploading…",
  imageInsertedCommitted: (path: string) =>
    `Drafted ${path}. The image lands in the repository beside this document when you commit, so it is reviewed and versioned with it.`,
  imageInsertedLinked: (url: string) =>
    `Linked ${url}. The image is not versioned with the document and breaks if the blob is purged — commit it into the repository instead if it is meant to last.`,
  imageKeepInRepository: "Keep in the repository",
  imageLinkOnly: "Link only",
  imageChoiceHelp:
    "Keeping it commits the bytes beside the document. Linking references the relay's media store, which is lighter but is not versioned with the document.",
  pin: "Pin to sidebar",
  unpin: "Unpin from sidebar",
  pinned: "Pinned",
  pinMissing:
    "This pinned artifact is on neither main nor a draft. Nothing was lost — unpin it, or commit the file it names.",
  pinsTruncated:
    "The pin log was too long to read fully; a pin may be missing from the sidebar.",
  pinsRanksWithoutPin: (n: number) =>
    `${n} reorder${n === 1 ? "" : "s"} name${n === 1 ? "s" : ""} an artifact nothing pinned; kept, not shown.`,
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
  documents: "Documents",
  drafts: "Open drafts",
  noDrafts: "No open drafts.",
  commits: "Recent commits",
} as const;
