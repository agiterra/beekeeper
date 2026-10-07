import {
  Atom,
  BookOpen,
  Braces,
  Container,
  Database,
  File,
  FileArchive,
  FileAudio,
  FileCode,
  FileCog,
  FileImage,
  FileJson,
  FileLock,
  FileSpreadsheet,
  FileTerminal,
  FileText,
  FileType,
  FileVideo,
  Folder,
  GitBranch,
  Hash,
  type LucideIcon,
  Package,
  Palette,
  Scale,
} from "lucide-react";

/**
 * A file-type icon and its colour for a path (SV-32 file chips).
 *
 * Lucide only (already a dependency), with a Beekeeper colour per family:
 * the chip must read at a glance as "a React file", "a Markdown file", "a
 * folder", without a second icon package. About thirty types, then a plain
 * file. Colours are Tailwind palette classes that hold contrast in both
 * themes; the fallback is the muted foreground.
 */
export type FileTypeIcon = {
  Icon: LucideIcon;
  /** A text-colour class for the icon. */
  colorClass: string;
  /** A short family name for tests and `data-file-type`. */
  family: string;
};

const FOLDER: FileTypeIcon = {
  Icon: Folder,
  colorClass: "text-amber-500",
  family: "folder",
};
const FALLBACK: FileTypeIcon = {
  Icon: File,
  colorClass: "text-muted-foreground",
  family: "file",
};

function icon(Icon: LucideIcon, colorClass: string, family: string) {
  return { Icon, colorClass, family } satisfies FileTypeIcon;
}

const REACT = icon(Atom, "text-sky-500", "react");
const TYPESCRIPT = icon(FileCode, "text-blue-500", "typescript");
const JAVASCRIPT = icon(FileCode, "text-yellow-500", "javascript");
const RUST = icon(FileCode, "text-orange-600", "rust");
const PYTHON = icon(FileCode, "text-emerald-500", "python");
const GO = icon(FileCode, "text-cyan-500", "go");
const SWIFT = icon(FileCode, "text-orange-500", "swift");
const KOTLIN = icon(FileCode, "text-violet-500", "kotlin");
const DART = icon(FileCode, "text-teal-500", "dart");
const RUBY = icon(FileCode, "text-red-500", "ruby");
const JAVA = icon(FileCode, "text-red-600", "java");
const C_FAMILY = icon(FileCode, "text-indigo-500", "c");
const MARKDOWN = icon(FileText, "text-blue-400", "markdown");
const TEXT = icon(FileText, "text-muted-foreground", "text");
const JSON_ICON = icon(FileJson, "text-yellow-600", "json");
const CONFIG = icon(FileCog, "text-slate-500", "config");
const STYLE = icon(Palette, "text-pink-500", "style");
const HTML = icon(Braces, "text-orange-500", "html");
const SHELL = icon(FileTerminal, "text-green-600", "shell");
const SQL = icon(Database, "text-sky-600", "sql");
const IMAGE = icon(FileImage, "text-purple-500", "image");
const VIDEO = icon(FileVideo, "text-rose-500", "video");
const AUDIO = icon(FileAudio, "text-fuchsia-500", "audio");
const ARCHIVE = icon(FileArchive, "text-stone-500", "archive");
const SPREADSHEET = icon(FileSpreadsheet, "text-green-500", "spreadsheet");
const FONT = icon(FileType, "text-slate-500", "font");
const LOCK = icon(FileLock, "text-slate-500", "lock");
const DOCKER = icon(Container, "text-sky-500", "docker");
const GIT = icon(GitBranch, "text-orange-600", "git");
const LICENSE = icon(Scale, "text-amber-600", "license");
const PACKAGE = icon(Package, "text-red-500", "package");
const README = icon(BookOpen, "text-blue-400", "readme");
const CSS_HASH = icon(Hash, "text-pink-500", "scss");

const BY_EXTENSION = new Map<string, FileTypeIcon>([
  ["tsx", REACT],
  ["jsx", REACT],
  ["ts", TYPESCRIPT],
  ["mts", TYPESCRIPT],
  ["cts", TYPESCRIPT],
  ["js", JAVASCRIPT],
  ["mjs", JAVASCRIPT],
  ["cjs", JAVASCRIPT],
  ["rs", RUST],
  ["py", PYTHON],
  ["go", GO],
  ["swift", SWIFT],
  ["kt", KOTLIN],
  ["kts", KOTLIN],
  ["dart", DART],
  ["rb", RUBY],
  ["java", JAVA],
  ["c", C_FAMILY],
  ["h", C_FAMILY],
  ["cc", C_FAMILY],
  ["cpp", C_FAMILY],
  ["hpp", C_FAMILY],
  ["m", C_FAMILY],
  ["md", MARKDOWN],
  ["mdx", MARKDOWN],
  ["txt", TEXT],
  ["log", TEXT],
  ["json", JSON_ICON],
  ["jsonc", JSON_ICON],
  ["toml", CONFIG],
  ["yml", CONFIG],
  ["yaml", CONFIG],
  ["ini", CONFIG],
  ["env", CONFIG],
  ["plist", CONFIG],
  ["xml", HTML],
  ["css", STYLE],
  ["scss", CSS_HASH],
  ["sass", CSS_HASH],
  ["html", HTML],
  ["htm", HTML],
  ["vue", HTML],
  ["svelte", HTML],
  ["sh", SHELL],
  ["bash", SHELL],
  ["zsh", SHELL],
  ["fish", SHELL],
  ["ps1", SHELL],
  ["sql", SQL],
  ["png", IMAGE],
  ["jpg", IMAGE],
  ["jpeg", IMAGE],
  ["gif", IMAGE],
  ["webp", IMAGE],
  ["svg", IMAGE],
  ["ico", IMAGE],
  ["avif", IMAGE],
  ["mp4", VIDEO],
  ["mov", VIDEO],
  ["webm", VIDEO],
  ["mp3", AUDIO],
  ["wav", AUDIO],
  ["ogg", AUDIO],
  ["zip", ARCHIVE],
  ["gz", ARCHIVE],
  ["tgz", ARCHIVE],
  ["tar", ARCHIVE],
  ["csv", SPREADSHEET],
  ["tsv", SPREADSHEET],
  ["xlsx", SPREADSHEET],
  ["ttf", FONT],
  ["otf", FONT],
  ["woff", FONT],
  ["woff2", FONT],
  ["lock", LOCK],
]);

const BY_NAME = new Map<string, FileTypeIcon>([
  ["dockerfile", DOCKER],
  ["containerfile", DOCKER],
  ["docker-compose.yml", DOCKER],
  ["compose.yml", DOCKER],
  [".gitignore", GIT],
  [".gitattributes", GIT],
  [".gitmodules", GIT],
  ["license", LICENSE],
  ["licence", LICENSE],
  ["copying", LICENSE],
  ["package.json", PACKAGE],
  ["cargo.toml", PACKAGE],
  ["pubspec.yaml", PACKAGE],
  ["readme", README],
  ["readme.md", README],
  ["makefile", SHELL],
  ["justfile", SHELL],
  ["pnpm-lock.yaml", LOCK],
  ["cargo.lock", LOCK],
]);

/**
 * The icon for `path`. `isDir` wins (a folder is a folder whatever its name);
 * then a well-known file name; then the extension; then a plain file.
 */
export function fileTypeIcon(path: string, isDir = false): FileTypeIcon {
  if (isDir || path.endsWith("/")) return FOLDER;
  const name = (path.split("/").at(-1) ?? path).toLowerCase();
  const named = BY_NAME.get(name);
  if (named) return named;
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot === name.length - 1) return FALLBACK;
  return BY_EXTENSION.get(name.slice(dot + 1)) ?? FALLBACK;
}
