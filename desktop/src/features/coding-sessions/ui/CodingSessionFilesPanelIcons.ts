import type * as React from "react";
import {
  Braces,
  CodeXml,
  File,
  FileArchive,
  FileAudio,
  FileCode2,
  FileImage,
  FileJson,
  FileLock2,
  FileSpreadsheet,
  FileSymlink,
  FileText,
  FileVideo,
  Package,
  Settings,
} from "lucide-react";

import type { CodingSessionTreeEntry } from "@/shared/api/tauriCodingSessionTree";

/**
 * The icon a Files row draws for a file, by name (SV-24).
 *
 * T3 Code's tree (`files/FileBrowserPanel.tsx`, `pierre-icons.ts`) colours
 * each file by its type and gives a folder no icon at all — a chevron and a
 * name. Beekeeper has no sprite sheet, so the same idea is drawn with lucide
 * glyphs in the hues the project's repository panel already uses
 * (`features/projects/ui/ProjectRepositoryPanel.tsx`). Folders are not passed
 * here; the row draws their chevron.
 */
export type CodingSessionFileIcon = {
  Icon: React.ComponentType<{ className?: string; "aria-hidden"?: boolean }>;
  className: string;
};

const CODE = new Set([
  "c",
  "cc",
  "cpp",
  "cs",
  "dart",
  "go",
  "h",
  "java",
  "js",
  "jsx",
  "kt",
  "kts",
  "mjs",
  "mts",
  "py",
  "rb",
  "rs",
  "sh",
  "swift",
  "ts",
  "tsx",
  "zig",
]);
const IMAGE = new Set(["gif", "heic", "jpeg", "jpg", "png", "svg", "webp"]);
const ARCHIVE = new Set(["7z", "bz2", "gz", "rar", "tar", "tgz", "zip"]);
const AUDIO = new Set(["aac", "flac", "m4a", "mp3", "ogg", "wav"]);
const VIDEO = new Set(["avi", "m4v", "mov", "mp4", "webm"]);
const SHEET = new Set(["csv", "ods", "tsv", "xls", "xlsx"]);
const TEXT = new Set(["md", "mdx", "rst", "txt"]);
const CONFIG = new Set(["conf", "config", "env", "ini", "toml", "yaml", "yml"]);

/** The extension after the last dot, lowercased; "" for none or a dotfile. */
function extensionOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot <= 0 ? "" : name.slice(dot + 1).toLowerCase();
}

export function codingSessionFileIcon(
  entry: Pick<CodingSessionTreeEntry, "kind" | "name">,
): CodingSessionFileIcon {
  if (entry.kind === "symlink") {
    return { Icon: FileSymlink, className: "text-muted-foreground" };
  }
  const name = entry.name.toLowerCase();
  const extension = extensionOf(name);
  if (
    name === "package.json" ||
    name === "cargo.toml" ||
    name === "dockerfile" ||
    name === "containerfile"
  ) {
    return { Icon: Package, className: "text-orange-500" };
  }
  if (name.includes("lock") || extension === "pem" || extension === "key") {
    return { Icon: FileLock2, className: "text-amber-500" };
  }
  if (extension === "json")
    return { Icon: FileJson, className: "text-yellow-500" };
  if (CONFIG.has(extension) || name.startsWith(".")) {
    return { Icon: Settings, className: "text-zinc-500" };
  }
  if (extension === "html" || extension === "xml") {
    return { Icon: CodeXml, className: "text-rose-500" };
  }
  if (extension === "css" || extension === "scss") {
    return { Icon: Braces, className: "text-violet-500" };
  }
  if (CODE.has(extension))
    return { Icon: FileCode2, className: "text-blue-500" };
  if (TEXT.has(extension)) return { Icon: FileText, className: "text-sky-500" };
  if (IMAGE.has(extension))
    return { Icon: FileImage, className: "text-pink-500" };
  if (ARCHIVE.has(extension)) {
    return { Icon: FileArchive, className: "text-orange-500" };
  }
  if (AUDIO.has(extension))
    return { Icon: FileAudio, className: "text-purple-500" };
  if (VIDEO.has(extension))
    return { Icon: FileVideo, className: "text-fuchsia-500" };
  if (SHEET.has(extension)) {
    return { Icon: FileSpreadsheet, className: "text-emerald-500" };
  }
  return { Icon: File, className: "text-muted-foreground" };
}
