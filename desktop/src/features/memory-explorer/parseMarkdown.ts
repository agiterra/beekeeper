import { fromMarkdown } from "mdast-util-from-markdown";
import { gfm } from "micromark-extension-gfm";
import { gfmFromMarkdown } from "mdast-util-gfm";
import { enrichBeekeeper } from "./adapters/beekeeper";
import {
  localTarget,
  nodeId,
  type DocumentIndex,
  type SourceNode,
} from "./model";

type Ast = {
  type: string;
  value?: string;
  url?: string;
  identifier?: string;
  depth?: number;
  children?: Ast[];
  position?: { start: { line: number }; end: { line: number } };
};
export const prose = (n: Ast): string =>
  n.type === "code" || n.type === "inlineCode"
    ? ""
    : (n.value ?? (n.children ?? []).map(prose).join(""));
export const slug = (s: string) =>
  s
    .toLowerCase()
    .replace(/[^\p{L}\p{N}_\-\s]/gu, "")
    .replace(/\s/g, "-");

/** GFM positions retain exact authored line ranges. Parsing runs in a worker. */
export function parseMarkdown(
  path: string,
  text: string,
  beekeeper = false,
): DocumentIndex {
  const lines = text.split("\n");
  let frontEnd = 0;
  if (lines[0] === "---") {
    const end = lines.slice(1).indexOf("---");
    if (end >= 0) frontEnd = end + 2;
  }
  // Blank frontmatter without shifting positions or presenting it as Markdown.
  const ast = fromMarkdown(
    lines.map((s, i) => (i < frontEnd ? "" : s)).join("\n"),
    { extensions: [gfm()], mdastExtensions: [gfmFromMarkdown()] },
  ) as Ast;
  const blocks = ast.children ?? [];
  const definitions = new Map<string, string>();
  const visit = (n: Ast, f: (n: Ast) => void) => {
    f(n);
    for (const c of n.children ?? []) visit(c, f);
  };
  visit(ast, (n) => {
    if (n.type === "definition" && n.identifier && n.url)
      definitions.set(n.identifier.toLowerCase(), n.url);
  });
  const nodes: SourceNode[] = [];
  const make = (
    fragment: string,
    title: string,
    type: SourceNode["type"],
    start: number,
    end: number,
  ): SourceNode => ({
    id: nodeId(path, fragment),
    path,
    fragment,
    title,
    type,
    start,
    end,
    text: lines.slice(start - 1, end).join("\n"),
    references: [],
  });
  const headings = blocks.filter((n) => n.type === "heading");
  const title = headings.length
    ? prose(headings[0])
    : (path.split("/").at(-1) ?? path);
  // A giant document opens its introductory passage, with full text searchable through sections.
  nodes.push(
    make(
      "",
      title,
      "document",
      frontEnd + 1,
      headings[1]?.position?.start.line
        ? headings[1].position.start.line - 1
        : lines.length,
    ),
  );
  const seen = new Map<string, number>();
  for (const [i, h] of headings.entries()) {
    const label = prose(h);
    const base = slug(label);
    const occurrence = seen.get(base) ?? 0;
    seen.set(base, occurrence + 1);
    const fragment = occurrence ? `${base}-${occurrence}` : base;
    nodes.push(
      make(
        fragment,
        label,
        "section",
        h.position?.start.line ?? 1,
        (headings[i + 1]?.position?.start.line ?? lines.length + 1) - 1,
      ),
    );
  }
  if (!headings.length && lines.length > 120) {
    for (let start = frontEnd + 1; start <= lines.length; start += 120)
      nodes.push(
        make(
          `line-${start}`,
          `Lines ${start}–${Math.min(start + 119, lines.length)}`,
          "section",
          start,
          Math.min(start + 119, lines.length),
        ),
      );
  }
  if (beekeeper) enrichBeekeeper(path, blocks, lines, nodes, make, prose);
  // Limit reader ranges, without discarding searchable text: long sections become adjacent chunks.
  for (const n of [...nodes]) {
    if (n.end - n.start < 400) continue;
    const originalEnd = n.end;
    n.end = n.start + 399;
    n.text = lines.slice(n.start - 1, n.end).join("\n");
    for (let start = n.end + 1; start <= originalEnd; start += 400)
      nodes.push(
        make(
          `${n.fragment}-line-${start}`,
          `${n.title} · continued`,
          "section",
          start,
          Math.min(start + 399, originalEnd),
        ),
      );
  }
  visit(ast, (n) => {
    const href =
      n.type === "link"
        ? n.url
        : n.type === "linkReference"
          ? definitions.get(n.identifier?.toLowerCase() ?? "")
          : undefined;
    if (!href) return;
    const target = localTarget(path, href);
    if (!target) return;
    const line = n.position?.start.line ?? 1;
    const owner = [...nodes]
      .filter((n) => n.start <= line && n.end >= line)
      .sort((a, b) => a.end - a.start - (b.end - b.start))[0];
    owner?.references.push({ target, basis: "markdown-link", line });
  });
  const identities = new Map<string, number>();
  for (const node of nodes) {
    const count = identities.get(node.id) ?? 0;
    identities.set(node.id, count + 1);
    if (count) node.id = `${node.id}@occurrence-${count + 1}`;
  }
  return {
    path,
    nodes,
    frontmatter: frontEnd ? lines.slice(0, frontEnd).join("\n") : null,
  };
}
