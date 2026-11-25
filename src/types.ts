export type Speaker = "me" | "others";

export interface TranscriptLine {
  speaker: Speaker;
  speaker_index?: number;
  text: string;
  lang?: string;
}

export interface TranscriptUpdate {
  lines: TranscriptLine[];
  partial_me: string;
  partial_others: string;
}

export type SummarySpan = { kind: "text"; text: string } | { kind: "strong"; text: string };
export interface SummaryBullet {
  spans: SummarySpan[];
  depth: number;
  mine: boolean;
}
export type SummaryBlock =
  | { kind: "heading"; text: string }
  | { kind: "paragraph"; spans: SummarySpan[] }
  | { kind: "bullets"; items: SummaryBullet[] };

export interface MeetingNote {
  id: string;
  service: string;
  display_title: string;
  title: string;
  started_at: number;
  ended_at: number | null;
  transcript: string;
  transcript_lines: TranscriptLine[];
  summary: string | null;
  summary_blocks: SummaryBlock[];
  summarised_at: number | null;
  my_notes: string;
  my_notes_at: number | null;
  meeting_code: string | null;
  participants: string[];
  others_name: string | null;
  dropped: number;
}

export interface MeetingState {
  listening: boolean;
  service: string | null;
  id: string | null;
  warning: string | null;
}

export interface MeetingOffer {
  key: string;
  service: string;
}

export interface DictationState {
  active: boolean;
  partial: string;
  finishing: boolean;
  held: boolean;
  error: string | null;
  text: string | null;
}

export interface EventRow {
  id: string;
  title: string;
  starts_at: number;
  ends_at: number | null;
  location: string;
  description: string;
  attendees: string[];
  link: string | null;
  source: "calendar" | "manual";
}

export interface PrepQuestion {
  question: string;
  answer: string | null;
}

export interface Prediction {
  point: string;
  why: string;
  source: string;
  say: string | null;
}

export interface MeetingPrep {
  event_id: string;
  participants: string[];
  brief: string;
  agenda: string;
  questions: PrepQuestion[];
  predictions: Prediction[];
  thin: boolean;
  last_time: string | null;
  dropped: string[];
  sources: [string, string][];
  research: string | null;
  updated_at: number;
}

export interface UpcomingMeeting {
  event: EventRow;
  prep: MeetingPrep | null;
}

export interface PlatformSupport {
  os: string;
  typing: boolean;
  system_audio: boolean;
  meeting_detection: boolean;
  needs_accessibility: boolean;
}

export interface SettingsView {
  openai_key_hint: string | null;
  deepgram_key_hint: string | null;
  mic_device: string;
  dictation_on: boolean;
  dictation_key: string;
  dictation_mode: "toggle" | "hold";
  dictation_cleanup: boolean;
  notetaker_on: boolean;
  meeting_detection: boolean;
  pre_meeting_notes: boolean;
  web_research: boolean;
  calendar_url_set: boolean;
  your_name: string;
  about_you: string;
  platform: PlatformSupport;
}

export interface ExportReport {
  count: number;
  skipped: number;
  folder: string;
}

export type View = "meetings" | "upcoming" | "settings";

/// "You", "Others", "Speaker 2", or the one other person's name.
export function speakerLabel(l: TranscriptLine, othersName?: string | null): string {
  if (l.speaker === "me") return "You";
  if (othersName) return othersName;
  return l.speaker_index ? `Speaker ${l.speaker_index}` : "Others";
}
