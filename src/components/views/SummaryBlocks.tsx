import type { SummaryBlock, SummarySpan } from "../../types";

/// Lays out a summary the app has already parsed: what is a heading, a
/// bullet or an emphasis is decided (and tested) in `summary_md.rs`.
function Spans({ spans }: { spans: SummarySpan[] }) {
  return (
    <>
      {spans.map((s, i) =>
        s.kind === "strong" ? <strong key={i}>{s.text}</strong> : <span key={i}>{s.text}</span>,
      )}
    </>
  );
}

export function SummaryBlocks({ blocks }: { blocks: SummaryBlock[] }) {
  return (
    <div className="summary">
      {blocks.map((block, i) => {
        if (block.kind === "heading") {
          return (
            <h4 key={i} className="summary-h">
              {block.text}
            </h4>
          );
        }
        if (block.kind === "paragraph") {
          return (
            <p key={i} className="summary-p">
              <Spans spans={block.spans} />
            </p>
          );
        }
        return (
          <ul key={i} className="summary-list">
            {block.items.map((item, j) => (
              <li key={j} className={`summary-li depth-${Math.min(item.depth, 1)}${item.mine ? " is-mine" : ""}`}>
                <Spans spans={item.spans} />
              </li>
            ))}
          </ul>
        );
      })}
    </div>
  );
}
