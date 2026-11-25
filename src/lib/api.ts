import { invoke } from "@tauri-apps/api/core";
import type {
  EventRow,
  ExportReport,
  MeetingNote,
  MeetingOffer,
  MeetingPrep,
  MeetingState,
  SettingsView,
  TranscriptUpdate,
  UpcomingMeeting,
} from "../types";

/// Every call the pages make to the app, in one place.
export const api = {
  // Settings
  getSettings: () => invoke<SettingsView>("get_settings"),
  setSetting: (key: string, value: boolean | string) => invoke<SettingsView>("set_setting", { key, value }),
  setApiKey: (provider: "openai" | "deepgram", key: string) => invoke<SettingsView>("set_api_key", { provider, key }),
  checkApiKey: (provider: "openai" | "deepgram") => invoke<void>("check_api_key", { provider }),
  listMicrophones: () => invoke<string[]>("list_microphones"),
  setDictationKey: (key: string) => invoke<SettingsView>("set_dictation_key", { key }),
  setCalendarUrl: (url: string) => invoke<number>("set_calendar_url", { url }),
  requestAccessibility: () => invoke<boolean>("request_accessibility"),
  openPrivacySettings: (pane: string) => invoke<boolean>("open_privacy_settings", { pane }),

  // Meetings
  listMeetingNotes: () => invoke<MeetingNote[]>("list_meeting_notes"),
  getMeetingState: () => invoke<MeetingState>("get_meeting_state"),
  getLiveTranscript: () => invoke<TranscriptUpdate>("get_live_transcript"),
  getMeetingOffer: () => invoke<MeetingOffer | null>("get_meeting_offer"),
  startMeetingNotes: () => invoke<string>("start_meeting_notes"),
  stopMeetingNotes: () => invoke<void>("stop_meeting_notes"),
  acceptMeetingOffer: () => invoke<string>("accept_meeting_offer"),
  declineMeetingOffer: () => invoke<void>("decline_meeting_offer"),
  saveMeetingMyNotes: (id: string, text: string) => invoke<boolean>("save_meeting_my_notes", { id, text }),
  summariseMeeting: (id: string) => invoke<string>("summarise_meeting", { id }),
  renameMeeting: (id: string, title: string) => invoke<boolean>("rename_meeting", { id, title }),
  setMeetingPeople: (id: string, people: string[]) => invoke<boolean>("set_meeting_people", { id, people }),
  deleteMeeting: (id: string) => invoke<boolean>("delete_meeting", { id }),
  meetingMarkdown: (id: string) => invoke<string>("meeting_markdown", { id }),
  exportMeeting: (id: string, what: "notes" | "transcript") => invoke<string>("export_meeting", { id, what }),
  exportAllMeetings: () => invoke<ExportReport>("export_all_meetings"),

  // Upcoming meetings and their prep
  upcomingMeetings: () => invoke<UpcomingMeeting[]>("upcoming_meetings"),
  addMeeting: (m: { title: string; startsAt: number; minutes: number; attendees: string[]; description: string; link: string | null }) =>
    invoke<EventRow>("add_meeting", m),
  deleteUpcomingMeeting: (id: string) => invoke<boolean>("delete_upcoming_meeting", { id }),
  refreshCalendar: () => invoke<number>("refresh_calendar"),
  buildMeetingPrep: (eventId: string, fresh: boolean) => invoke<MeetingPrep>("build_meeting_prep", { eventId, fresh }),
  answerPrepQuestion: (eventId: string, index: number, answer: string | null) => invoke<MeetingPrep>("answer_prep_question", { eventId, index, answer }),
  dropPrepPoint: (eventId: string, index: number) => invoke<MeetingPrep>("drop_prep_point", { eventId, index }),
  addPrepPoint: (eventId: string, point: string) => invoke<MeetingPrep>("add_prep_point", { eventId, point }),
  setPrepPeople: (eventId: string, people: string[]) => invoke<MeetingPrep>("set_prep_people", { eventId, people }),

  // Dictation and windows
  toggleDictation: () => invoke<void>("toggle_dictation"),
  showMain: (view?: string) => invoke<void>("show_main", { view }),
  pillVisible: (visible: boolean) => invoke<void>("pill_visible", { visible }),
};

/// A command's error as a sentence. The app already words its errors for
/// people; anything else becomes a plain "try again".
export function said(e: unknown): string {
  return typeof e === "string" && e.trim() ? e : "That did not work. Try again.";
}
