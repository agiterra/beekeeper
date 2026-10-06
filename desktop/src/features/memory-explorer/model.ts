export type SourceNode = {
  id: string;
  path: string;
  fragment: string;
  title: string;
  type: "document" | "section" | "row" | "finding";
  start: number;
  end: number;
  text: string;
  claim?: string;
  readText?: string;
  references: {
    target: string;
    basis: "markdown-link" | "recognized-reference";
    line: number;
  }[];
};
export type DocumentIndex = {
  path: string;
  nodes: SourceNode[];
  frontmatter: string | null;
};
export type Connection = {
  from: string;
  to: string[];
  target: string;
  basis: string;
  line: number;
};
export const nodeId = (path: string, fragment: string) => `${path}#${fragment}`;
export function localTarget(path: string, href: string): string | null {
  if (/^(?:[a-z][\w+.-]*:|\/\/|\/)/i.test(href)) return null;
  let decoded: string;
  try {
    decoded = decodeURIComponent(href);
  } catch {
    return null;
  }
  const [file, fragment = ""] = decoded.split("#");
  const parts = (
    file ? `${path.split("/").slice(0, -1).join("/")}/${file}` : path
  ).split("/");
  const normalized: string[] = [];
  for (const part of parts) {
    if (!part || part === ".") continue;
    if (part === "..") {
      if (!normalized.length) return null;
      normalized.pop();
    } else normalized.push(part);
  }
  return nodeId(normalized.join("/"), fragment);
}
export function connect(documents: DocumentIndex[]): Connection[] {
  const nodes = documents.flatMap((d) => d.nodes);
  const explicit = nodes.flatMap((node) =>
    node.references.map((ref) => ({
      from: node.id,
      target: ref.target,
      basis: ref.basis,
      line: ref.line,
      to: ref.target.startsWith("sv:")
        ? (nodes.some(
            (n) =>
              n.type === "row" &&
              n.path === node.path &&
              n.fragment === ref.target.slice(3),
          )
            ? nodes.filter(
                (n) =>
                  n.type === "row" &&
                  n.path === node.path &&
                  n.fragment === ref.target.slice(3),
              )
            : nodes.filter(
                (n) => n.type === "row" && n.fragment === ref.target.slice(3),
              )
          ).map((n) => n.id)
        : nodes
            .filter((n) => nodeId(n.path, n.fragment) === ref.target)
            .map((n) => n.id),
    })),
  );
  const contains = documents.flatMap((d) =>
    d.nodes.slice(1).map((n) => ({
      from: d.nodes[0].id,
      to: [n.id],
      target: n.id,
      basis: "contains",
      line: n.start,
    })),
  );
  return [...contains, ...explicit];
}
