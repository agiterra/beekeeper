import { nodeId, type SourceNode } from "../model";
type Ast = {
  type: string;
  children?: Ast[];
  position?: {
    start: { line: number; column?: number };
    end: { line: number; column?: number };
  };
};
type Make = (
  fragment: string,
  title: string,
  type: SourceNode["type"],
  start: number,
  end: number,
) => SourceNode;

/** Optional structural enrichment. No project IDs, curated edges or rewritten claims. */
export function enrichBeekeeper(
  path: string,
  blocks: Ast[],
  lines: string[],
  nodes: SourceNode[],
  make: Make,
  prose: (n: Ast) => string,
) {
  const ledger = path === "plans/SESSION_STATE.md";
  const visit = (n: Ast, f: (n: Ast) => void) => {
    f(n);
    for (const c of n.children ?? []) visit(c, f);
  };
  if (ledger) {
    let findings = false;
    let appendedFindings = false;
    for (const block of blocks) {
      const start = block.position?.start.line ?? 1;
      if (block.type === "heading" && /^## /.test(lines[start - 1])) {
        findings = /^## 2\. /.test(lines[start - 1]);
        // This ledger historically appended findings after its closing rule.
        appendedFindings = /^## 5\. /.test(lines[start - 1]);
      }
      if ((!findings && !appendedFindings) || block.type !== "list") continue;
      for (const item of block.children ?? []) {
        const first = item.position?.start.line ?? 1;
        const match = /^(\d+)\.\s+(?:\*\*|~~)/.exec(lines[first - 1]);
        if (!match) continue;
        const end = item.position?.end.line ?? first;
        const n = make(
          `ledger-${match[1]}`,
          `Ledger ${match[1]} · ${prose(item).slice(0, 110)}`,
          "finding",
          first,
          end,
        );
        // The shared renderer restarts ordered lists at 1. The finding's true
        // number is already in its title; read its prose without that marker.
        // Exact source retains the original numbered bytes.
        n.readText = n.text.replace(/^\d+\.\s+/, "");
        nodes.push(n);
        // Paragraph positions exclude fenced examples from subitem recognition.
        const subitems: { label: string; line: number }[] = [];
        visit(item, (child) => {
          if (child.type !== "paragraph" || !child.position) return;
          for (
            let line = child.position.start.line;
            line <= child.position.end.line;
            line++
          ) {
            for (const sub of lines[line - 1].matchAll(
              /(?:^|\s)\(([a-z])\)\s+\*\*/g,
            ))
              subitems.push({ label: sub[1], line });
          }
        });
        for (const [i, sub] of subitems.entries()) {
          nodes.push(
            make(
              `ledger-${match[1]}(${sub.label})`,
              `Ledger ${match[1]}(${sub.label})`,
              "finding",
              sub.line,
              Math.max(sub.line, (subitems[i + 1]?.line ?? end + 1) - 1),
            ),
          );
        }
      }
    }
  }
  for (const block of blocks) {
    if (block.type !== "table") continue;
    const header = (block.children?.[0]?.children ?? []).map(prose);
    const statusIndex = header.findIndex((s) => /status|state/i.test(s));
    for (const row of block.children?.slice(1) ?? []) {
      const cells = (row.children ?? []).map(prose);
      const id = /^SV-\d+$/.exec(cells[0]?.trim() ?? "")?.[0];
      if (!id) continue;
      const start = row.position?.start.line ?? 1;
      const node = make(
        id,
        `${id} · ${cells[1]?.slice(0, 100) ?? ""}`,
        "row",
        start,
        row.position?.end.line ?? start,
      );
      const rawCell = (cell: Ast) => {
        const range = cell.position;
        return range && range.start.line === range.end.line
          ? lines[range.start.line - 1]
              .slice((range.start.column ?? 1) - 1, (range.end.column ?? 1) - 1)
              .trim()
              .replace(/^\|\s*|\s*\|$/g, "")
          : prose(cell);
      };
      const raw = (row.children ?? []).map(rawCell);
      node.claim = statusIndex >= 0 ? raw[statusIndex] : undefined;
      node.readText = raw
        .map((cell, i) => `### ${header[i] ?? "Source cell"}\n\n${cell}`)
        .join("\n\n");
      nodes.push(node);
    }
  }
  // Read prose from the AST, excluding fenced/inline code and link destinations.
  for (const node of nodes) {
    const texts: { text: string; line: number }[] = [];
    for (const block of blocks)
      visit(block, (ast) => {
        const line = ast.position?.start.line ?? 0;
        if (
          line < node.start ||
          line > node.end ||
          !["paragraph", "tableRow", "heading"].includes(ast.type)
        )
          return;
        texts.push({ text: prose(ast), line });
      });
    for (const { text, line } of texts) {
      const refs = new Set<string>();
      for (const match of text.matchAll(
        /\bSV-(\d+)(?:\s*(?:\.\.|[–−-])\s*(?:SV-)?(\d+))?/g,
      )) {
        const a = Number(match[1]),
          b = Number(match[2] ?? match[1]);
        if (b >= a && b - a <= 30)
          for (let id = a; id <= b; id++) refs.add(`sv:SV-${id}`);
      }
      for (const match of text.matchAll(
        /(?:ledger(?:\s+item)?|SESSION_STATE\.md\s+item)\s+(\d+)(?:\(([a-z])\))?/gi,
      )) {
        refs.add(
          nodeId(
            "plans/SESSION_STATE.md",
            `ledger-${match[1]}${match[2] ? `(${match[2]})` : ""}`,
          ),
        );
      }
      if (ledger) {
        for (const match of text.matchAll(
          /\b(?:fixes|compare|finding|item)\s+(\d+)\b/gi,
        ))
          refs.add(nodeId(path, `ledger-${match[1]}`));
      }
      if (node.type === "row" && /ledger/i.test(text)) {
        for (const match of text.matchAll(/\((\d{2,4})\)/g))
          refs.add(nodeId("plans/SESSION_STATE.md", `ledger-${match[1]}`));
      }
      for (const target of refs)
        if (target !== node.id && target !== `sv:${node.fragment}`)
          node.references.push({ target, basis: "recognized-reference", line });
    }
  }
}
