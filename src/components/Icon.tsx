// SF-Symbol-style glyphs as inline SVG: 1.6px rounded strokes on a 20px
// grid, drawn to sit next to San Francisco text the way the real symbols
// do. Web views cannot use SF Symbols themselves, so these are the few
// the app needs, in that language. `currentColor` throughout.
import type { ReactElement, SVGProps } from "react";

type Name =
  | "sun"
  | "external"
  | "moon"
  | "display"
  | "mic"
  | "pause"
  | "play"
  | "xmark"
  | "speaker-off"
  | "chevron-right"
  | "waveform"
  | "sparkle"
  | "flag"
  | "eye"
  | "loops"
  | "notes"
  | "feather"
  | "meetings"
  | "review"
  | "memory"
  | "layers"
  | "video"
  | "history"
  | "settings"
  | "activity"
  | "search"
  | "signout"
  | "compose"
  | "plus"
  | "paperclip"
  | "arrow-up"
  | "dots"
  | "check-circle"
  | "check"
  | "lock"
  | "type"
  | "clock"
  | "copy"
  | "thumb-up"
  | "thumb-down"
  | "speaker"
  | "retry"
  | "arrow-down"
  | "stop";

const PATHS: Record<Name, ReactElement> = {
  sun: (
    <>
      <circle cx="10" cy="10" r="3.4" />
      <path d="M10 2.5v2M10 15.5v2M2.5 10h2M15.5 10h2M4.7 4.7l1.4 1.4M13.9 13.9l1.4 1.4M4.7 15.3l1.4-1.4M13.9 6.1l1.4-1.4" />
    </>
  ),
  moon: <path d="M15.8 12.6A6.5 6.5 0 0 1 7.4 4.2a6.5 6.5 0 1 0 8.4 8.4z" />,
  display: (
    <>
      <rect x="2.5" y="3.5" width="15" height="10" rx="2" />
      <path d="M7 17h6M10 13.5V17" />
    </>
  ),
  mic: (
    <>
      <rect x="7" y="2.5" width="6" height="9.5" rx="3" />
      <path d="M4.5 9.5a5.5 5.5 0 0 0 11 0M10 15v2.5M7.5 17.5h5" />
    </>
  ),
  pause: <path d="M7 4.5v11M13 4.5v11" strokeWidth="2.2" />,
  play: <path d="M6.5 4.2v11.6L16 10z" fill="currentColor" stroke="none" />,
  xmark: <path d="M5.5 5.5l9 9M14.5 5.5l-9 9" />,
  // The chat-first shell (2026-10-06): new chat, attach, send, more.
  compose: (
    <>
      <path d="M10 16.5h7" />
      <path d="M13.8 3.2a1.8 1.8 0 0 1 2.6 2.6L6 16.2l-3.4.9.9-3.4z" />
    </>
  ),
  plus: <path d="M10 4v12M4 10h12" />,
  paperclip: <path d="M15.5 9.5 9.6 15.4a3.3 3.3 0 0 1-4.7-4.7l6.3-6.3a2.2 2.2 0 0 1 3.1 3.1l-6.2 6.2a1.1 1.1 0 0 1-1.6-1.6l5.6-5.6" />,
  "arrow-up": <path d="M10 16V4M5 9l5-5 5 5" strokeWidth="2" />,
  "arrow-down": <path d="M10 4v12M5 11l5 5 5-5" strokeWidth="2" />,
  "thumb-up": <path d="M6 8.5v8H3.5v-8Zm0 0 3.3-5.6a1.6 1.6 0 0 1 2.9 1.2l-.7 3.4h4.1a1.6 1.6 0 0 1 1.6 1.9l-1 5A1.6 1.6 0 0 1 14.6 16.5H6" />,
  "thumb-down": <path d="M14 11.5v-8h2.5v8Zm0 0-3.3 5.6a1.6 1.6 0 0 1-2.9-1.2l.7-3.4H4.4a1.6 1.6 0 0 1-1.6-1.9l1-5A1.6 1.6 0 0 1 5.4 3.5H14" />,
  speaker: <path d="M3.5 7.5v5h3l4 3.5v-12l-4 3.5Zm10 0a4 4 0 0 1 0 5" />,
  retry: <path d="M16.5 10a6.5 6.5 0 1 1-1.9-4.6l1.9 1.9M16.5 3.5v3.8h-3.8" />,
  stop: <rect x="5.5" y="5.5" width="9" height="9" rx="1.5" fill="currentColor" stroke="none" />,
  external: <path d="M8 4H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h10a1 1 0 0 0 1-1v-3M11 4h5v5M16 4l-7 7" />,
  dots: (
    <>
      <circle cx="4.5" cy="10" r="1.3" fill="currentColor" stroke="none" />
      <circle cx="10" cy="10" r="1.3" fill="currentColor" stroke="none" />
      <circle cx="15.5" cy="10" r="1.3" fill="currentColor" stroke="none" />
    </>
  ),
  check: <path d="M4.5 10.5l3.6 3.6 7.4-8" />,
  type: (
    <path d="M4.5 6V4.5h11V6M10 4.5v11M7.5 15.5h5" />
  ),
  lock: (
    <>
      <rect x="4.5" y="9" width="11" height="8" rx="1.8" />
      <path d="M7 9V6.8a3 3 0 0 1 6 0V9" />
    </>
  ),
  "check-circle": (
    <>
      <circle cx="10" cy="10" r="7" />
      <path d="M7 10.2l2.1 2.1 3.9-4.3" />
    </>
  ),
  clock: (
    <>
      <circle cx="10" cy="10" r="7" />
      <path d="M10 6.2V10l2.6 1.7" />
    </>
  ),
  // A speaker with a bar through it: "stop the voice", not "close".
  "speaker-off": (
    <>
      <path d="M3.5 8h2.8L10 5v10l-3.7-3H3.5z" />
      <path d="M12.5 7.5l4 5M16.5 7.5l-4 5" />
    </>
  ),
  "chevron-right": <path d="M7.5 4.5l5.5 5.5-5.5 5.5" />,
  waveform: <path d="M3 10h1M6 7v6M9 4v12M12 6.5v7M15 8.5v3M18 10h-1" />,
  sparkle: <path d="M10 2.5l1.7 5.3 5.3 1.7-5.3 1.7L10 16.5l-1.7-5.3L3 9.5l5.3-1.7z" />,
  flag: <path d="M5 17.5v-14M5 3.5h9.5l-2 3.5 2 3.5H5" />,
  eye: (
    <>
      <path d="M2 10s3-5.5 8-5.5 8 5.5 8 5.5-3 5.5-8 5.5S2 10 2 10z" />
      <circle cx="10" cy="10" r="2.4" />
    </>
  ),
  loops: (
    <>
      <path d="M6.8 5.2h6.1a3.6 3.6 0 0 1 0 7.2H11" />
      <path d="M13.2 14.8H7.1a3.6 3.6 0 0 1 0-7.2H9" />
      <path d="M12.8 2.8l2.4 2.4-2.4 2.4M7.2 17.2l-2.4-2.4 2.4-2.4" />
    </>
  ),
  // A quill.
  feather: (
    <>
      <path d="M16.6 3.4c-4.6.2-8.6 3.5-9.6 8.3l-.8 3.6 3.6-.8c4.8-1 8.1-5 8.3-9.6z" />
      <path d="M3.2 16.8l6.6-6.6M8.6 11.4h3.8" />
    </>
  ),
  // Two sheets, one over the other: the copy glyph everyone knows.
  copy: (
    <>
      <rect x="7" y="6.5" width="10" height="11" rx="2" />
      <path d="M13 6.5V4.5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2h2" />
    </>
  ),
  notes: (
    <>
      <rect x="4.5" y="2.5" width="11.5" height="15" rx="2" />
      <path d="M7.5 6h5.5M7.5 9.5h5.5M7.5 13h3.5M2.5 6h3M2.5 10h3M2.5 14h3" />
    </>
  ),
  // Two people talking, not a notepad: the view is the whole meeting --
  // the brief before, the words during, the write-up after -- and the
  // notepad glyph stays with the notes.
  meetings: (
    <>
      <path d="M3.5 2.6H9.9a1.7 1.7 0 0 1 1.7 1.7v1.2a1.7 1.7 0 0 1-1.7 1.7H6.3L3.5 9.4V7.2a1.7 1.7 0 0 1-1.7-1.7V4.3a1.7 1.7 0 0 1 1.7-1.7z" />
      <path d="M16.5 10.4H10.1a1.7 1.7 0 0 0-1.7 1.7v1.2a1.7 1.7 0 0 0 1.7 1.7h3.6l2.8 2.2V15a1.7 1.7 0 0 0 1.7-1.7v-1.2a1.7 1.7 0 0 0-1.7-1.7z" />
    </>
  ),
  review: (
    <>
      <path d="M3.5 7V4.5A1.5 1.5 0 0 1 5 3h2.5M12.5 3H15a1.5 1.5 0 0 1 1.5 1.5V7M16.5 13v2.5A1.5 1.5 0 0 1 15 17h-2.5M7.5 17H5a1.5 1.5 0 0 1-1.5-1.5V13" />
      <path d="M6.5 10l2.2 2.2 4.8-5" />
    </>
  ),
  memory: (
    <>
      <circle cx="10" cy="4" r="2" /><circle cx="4.5" cy="14.5" r="2" /><circle cx="15.5" cy="14.5" r="2" />
      <path d="M8.9 5.8 5.6 12.7M11.1 5.8l3.3 6.9M6.5 14.5h7" />
    </>
  ),
  // Edits in Settings (board 279): a clip with a play mark.
  video: (
    <>
      <rect x="2.5" y="4.5" width="15" height="11" rx="2.5" />
      <path d="M8.5 8v4l3.5-2z" />
    </>
  ),
  // Supermemory in Settings (board 273): two stacked layers.
  layers: (
    <>
      <path d="M10 3l7 3.8-7 3.8-7-3.8z" />
      <path d="M3 11l7 3.8 7-3.8" />
    </>
  ),
  history: <path d="M4.1 5.5A7 7 0 1 1 3 11M3 5.5v4h4M10 6.5V10l2.5 1.5" />,
  settings: (
    <>
      <path d="M3 5h8M14 5h3M3 10h3M9 10h8M3 15h6M12 15h5" />
      <circle cx="12.5" cy="5" r="1.5" /><circle cx="7.5" cy="10" r="1.5" /><circle cx="10.5" cy="15" r="1.5" />
    </>
  ),
  activity: <path d="M2 11h3l2-6 3.2 11 2.3-8 1.7 3H18" />,
  search: (
    <>
      <circle cx="8.5" cy="8.5" r="5" /><path d="m12.2 12.2 4.3 4.3" />
    </>
  ),
  signout: (
    <>
      <path d="M7.5 3.5H5a1.5 1.5 0 0 0-1.5 1.5v10A1.5 1.5 0 0 0 5 16.5h2.5" />
      <path d="M12.5 13.5 16.5 10l-4-3.5M16.5 10H8" />
    </>
  ),
};

export function Icon({ name, size = 16, ...rest }: { name: Name; size?: number } & SVGProps<SVGSVGElement>) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...rest}
    >
      {PATHS[name]}
    </svg>
  );
}
