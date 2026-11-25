import { speakerLabel, type TranscriptUpdate } from "../../types";

/// A two-sided transcript. Who said it is shown on every line: a note that
/// cannot tell "you agreed" from "they agreed" is worse than no note.
/// Words still being revised are dimmed.
export function TranscriptLines({ update, emptyMessage }: { update: TranscriptUpdate | null; emptyMessage: string }) {
  const lines = update?.lines ?? [];
  const me = update?.partial_me?.trim() ?? "";
  const others = update?.partial_others?.trim() ?? "";

  if (lines.length === 0 && !me && !others) {
    return <p className="note-waiting">{emptyMessage}</p>;
  }

  return (
    <div className="transcript">
      {lines.map((l, i) => (
        <p key={i} className={`transcript-line is-${l.speaker}`}>
          <span className="transcript-who">{speakerLabel(l)}</span>
          <span className="transcript-text">{l.text}</span>
        </p>
      ))}
      {others && (
        <p className="transcript-line is-others is-partial">
          <span className="transcript-who">Others</span>
          <span className="transcript-text">{others}</span>
        </p>
      )}
      {me && (
        <p className="transcript-line is-me is-partial">
          <span className="transcript-who">You</span>
          <span className="transcript-text">{me}</span>
        </p>
      )}
    </div>
  );
}
