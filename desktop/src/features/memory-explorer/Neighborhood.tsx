import * as React from "react";
import type { Connection, SourceNode } from "./model";

const typeLabel = {
  document: "Document",
  section: "Section",
  row: "Plan row",
  finding: "Finding",
};

/** A bounded, keyboard-accessible neighborhood with edges measured after layout. */
export function Neighborhood({
  active,
  neighbors,
  edges,
  select,
}: {
  active: SourceNode;
  neighbors: SourceNode[];
  edges: Connection[];
  select: (id: string) => void;
}) {
  const tree = React.useRef<HTMLDivElement>(null);
  const [drawing, setDrawing] = React.useState({
    width: 1,
    height: 1,
    paths: [] as string[],
  });
  React.useLayoutEffect(() => {
    const element = tree.current;
    if (!element) return;
    const draw = () => {
      const box = element.getBoundingClientRect();
      const focus = element
        .querySelector(".explorer-focus")
        ?.getBoundingClientRect();
      if (!focus || !box.width) return;
      const x = focus.left + focus.width / 2 - box.left;
      const y = focus.bottom - box.top;
      const paths = Array.from(
        element.querySelectorAll(".explorer-related"),
      ).map((child) => {
        const rect = child.getBoundingClientRect();
        const endX = rect.left + rect.width / 2 - box.left;
        const endY = rect.top - box.top;
        return `M ${x} ${y} V ${endY - 14} H ${endX} V ${endY + 26}`;
      });
      setDrawing({
        width: box.width,
        height: box.height,
        paths: paths.slice(0, neighbors.length),
      });
    };
    const observer = new ResizeObserver(draw);
    observer.observe(element);
    for (const child of element.querySelectorAll(".explorer-node"))
      observer.observe(child);
    draw();
    return () => observer.disconnect();
  }, [neighbors]);
  const card = (node: SourceNode) => (
    <>
      <span className="explorer-node-kind">
        <i aria-hidden="true" />
        {typeLabel[node.type]}
      </span>
      <strong className="explorer-node-title">{node.title}</strong>
      {node.claim && (
        <span className="explorer-node-claim">
          {node.claim.replace(/[`*]/g, "")}
        </span>
      )}
    </>
  );
  return (
    <div className="explorer-tree" ref={tree} data-testid="explorer-graph">
      <svg
        className="explorer-edges"
        aria-hidden="true"
        viewBox={`0 0 ${drawing.width} ${drawing.height}`}
        preserveAspectRatio="none"
      >
        {drawing.paths.map((path) => (
          <path key={path} d={path} />
        ))}
      </svg>
      <div
        className="explorer-node explorer-selected explorer-focus"
        data-kind={active.type}
      >
        {card(active)}
      </div>
      <div className="explorer-children">
        {neighbors.map((node) => {
          const outgoing = edges.find(
            (edge) => edge.from === active.id && edge.to.includes(node.id),
          );
          const incoming = edges.find(
            (edge) => edge.from === node.id && edge.to.includes(active.id),
          );
          const edge = outgoing ?? incoming;
          const relation =
            edge?.basis === "contains"
              ? outgoing
                ? "contains"
                : "belongs to"
              : outgoing
                ? "references"
                : "referenced by";
          return (
            <div className="explorer-related" key={node.id}>
              <span
                className="explorer-relation"
                title={
                  edge ? `${edge.basis} · source line ${edge.line}` : undefined
                }
              >
                {relation}
              </span>
              <button
                type="button"
                className="explorer-node"
                data-kind={node.type}
                onClick={() => select(node.id)}
                title={`${node.path}:${node.start}–${node.end}`}
              >
                {card(node)}
              </button>
            </div>
          );
        })}
      </div>
      {!neighbors.length && (
        <p className="mt-6 text-center text-xs text-muted-foreground">
          No indexed connections for this passage.
        </p>
      )}
    </div>
  );
}
