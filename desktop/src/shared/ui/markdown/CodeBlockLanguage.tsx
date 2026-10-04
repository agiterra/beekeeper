/**
 * The language mark in a fenced code block's header (SV-12).
 *
 * T3 Code draws a per-language file icon from its own icon set. Beekeeper has
 * no per-language marks, so a small family map onto lucide stands in: a shell
 * fence gets a terminal, JSON gets braces, SQL a database, and so on. The icon
 * is a family, not an identity — every language would collapse onto a handful
 * of glyphs — so the language's name is always printed beside it. Nothing the
 * fence declared is hidden behind a hover.
 */

import {
  Braces,
  CodeXml,
  Database,
  FileCode,
  FileCog,
  FileDiff,
  FileText,
  type LucideIcon,
  Palette,
  SquareTerminal,
} from "lucide-react";

const LANGUAGE_FAMILIES: ReadonlyArray<readonly [LucideIcon, string[]]> = [
  [
    SquareTerminal,
    [
      "bash",
      "sh",
      "zsh",
      "fish",
      "shell",
      "shellscript",
      "shellsession",
      "console",
      "terminal",
      "powershell",
      "pwsh",
      "ps1",
      "bat",
      "cmd",
    ],
  ],
  [Braces, ["json", "jsonc", "json5", "jsonl", "ndjson"]],
  [Database, ["sql", "psql", "plsql", "pgsql", "mysql", "sqlite", "graphql"]],
  [FileText, ["md", "markdown", "mdx", "txt", "text", "plaintext", "rst"]],
  [
    FileCog,
    [
      "yaml",
      "yml",
      "toml",
      "ini",
      "conf",
      "env",
      "dotenv",
      "properties",
      "dockerfile",
      "docker",
      "nix",
      "hcl",
      "terraform",
    ],
  ],
  [FileDiff, ["diff", "patch", "udiff"]],
  [CodeXml, ["html", "xml", "svg", "vue", "svelte", "astro", "jsx", "tsx"]],
  [Palette, ["css", "scss", "sass", "less", "postcss"]],
];

const ICON_BY_LANGUAGE = new Map<string, LucideIcon>(
  LANGUAGE_FAMILIES.flatMap(([icon, names]) =>
    names.map((name) => [name, icon] as const),
  ),
);

/**
 * The glyph for a fence's language: a family icon when one fits, a generic
 * code file otherwise, and `null` for a fence that declared no language.
 */
export function codeBlockLanguageIcon(language: string): LucideIcon | null {
  const normalized = language.trim().toLowerCase();
  if (!normalized) return null;
  return ICON_BY_LANGUAGE.get(normalized) ?? FileCode;
}

/** Icon plus the fence's own language name, for the code-block header. */
export function CodeBlockLanguageLabel({ language }: { language: string }) {
  const Icon = codeBlockLanguageIcon(language);
  if (!Icon) return <span aria-hidden="true" />;
  return (
    <span
      className="inline-flex min-w-0 items-center gap-1.5 font-mono text-2xs text-muted-foreground"
      data-code-block-language={language}
    >
      <Icon aria-hidden="true" className="size-3.5 shrink-0" />
      <span className="truncate">{language}</span>
    </span>
  );
}
