# Jelly Voice & Notes

A small desktop app with three things in it:

1. **Dictation.** Press a key in any app, talk, and the words are typed where your cursor is.
2. **Meeting notes.** A live transcript of your calls, your side and the other side kept apart, and notes written when the call ends.
3. **Pre-meeting notes.** Before a meeting on your calendar: who you are meeting, what will probably come up, and what to have ready.

It runs on your own API keys and keeps everything in a small database on your computer. There is no account, no server of its own, and no analytics.

## What it uses

| What | Provider | Needed for |
| --- | --- | --- |
| Live speech to text | Deepgram (`nova-3`, streaming) | Words as you speak, while dictating and during calls |
| Writing | OpenAI | Meeting notes, pre-meeting notes, tidying a dictation |
| Speech to text, in pieces | OpenAI | Dictation and calls when there is no Deepgram key |
| Web lookup of the people on a meeting | OpenAI (web search tool) | Pre-meeting notes, when "Look people up on the web" is on |

One key is enough to start: with only an OpenAI key everything works, and calls are written down about a minute behind instead of live. With only a Deepgram key, dictation and live transcripts work, but nothing is written from them.

The models are set in `src-tauri/src/openai.rs`. To use others without changing code, set `JELLY_OPENAI_MODEL`, `JELLY_OPENAI_FAST_MODEL` or `JELLY_OPENAI_TRANSCRIBE_MODEL` before starting the app.

## Adding your keys

1. Open the app, then **Settings, General, Your keys**.
2. Press **Add key** next to Deepgram or OpenAI, paste the key, and press **Save and check**. The app checks it with the provider and says whether it was accepted.

Keys are stored in the app's own database on your computer (`notes.sqlite` in the app's data folder, readable only by your user on macOS and Linux) and sent only to their provider. The page never shows a key again, only its last four characters.

## Your calendar

Pre-meeting notes read your calendar from its private address in iCal format. In Google Calendar it is in the calendar's settings under **Integrate calendar** ("Secret address in iCal format"); Outlook and iCloud give one when you publish or share a calendar. Paste it in **Settings, Pre-meeting notes, Calendar**. The calendar is read every 15 minutes.

No calendar? Add meetings by hand on the **Upcoming** page.

What the reader understands: single events, daily and weekly repeats (with skipped dates), attendees, and call links in the invite. Times with a time zone name are read as your computer's local time. Monthly and yearly repeats show their first date only.

## What works on each system

| | macOS (14.4+) | Windows 10/11 | Linux |
| --- | --- | --- | --- |
| Dictation: listening | Yes | Yes | Yes |
| Dictation: typing into other apps | Yes (needs Accessibility) | Yes | With `xdotool` (X11) or `wtype` (Wayland); otherwise the words stay in the pill to copy |
| Dictation key | Yes | Yes | Yes on X11; Wayland desktops may block global keys, use the tray item |
| Meeting notes: your microphone | Yes | Yes | Yes |
| Meeting notes: the other side of the call | Yes (System Audio Recording) | Yes (loopback of your speakers) | Not yet: only your microphone is heard |
| Noticing a call and offering notes | Yes | Yes | Not yet: start notes from the Meetings page or the tray |
| Pre-meeting notes | Yes | Yes | Yes |

There is no echo cancellation: on speakers rather than headphones, your microphone also hears the other people, so some of their words can appear twice.

## Building

You need [Rust](https://rustup.rs) (stable), Node.js 20 or newer, and the Tauri prerequisites for your system (see the Tauri v2 "Prerequisites" guide).

```sh
npm install
npm run tauri dev      # run it
npm run tauri build    # build an installer for this system
```

### macOS

Xcode Command Line Tools (`xcode-select --install`). On first use macOS asks for the microphone; allow **Accessibility** (for typing and for seeing which meeting window is in front) and **System Audio Recording** (for the other side of calls) in System Settings, Privacy and Security. **Settings, General, Permissions** in the app opens those pages. Permissions belong to the built app, so run the bundled app (`npm run tauri build`, then open the `.app`) rather than a binary started from a terminal.

### Windows

The Microsoft C++ Build Tools and WebView2 (preinstalled on Windows 11). Allow the microphone in Settings, Privacy and security, Microphone, under "Let desktop apps access your microphone". Apps run as administrator take no typing from other apps.

To check the Windows build from a Mac: `rustup target add x86_64-pc-windows-gnu`, `brew install mingw-w64`, then `cargo check --target x86_64-pc-windows-gnu` in `src-tauri`.

### Linux

The Tauri prerequisites (`libwebkit2gtk-4.1-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `build-essential`, `libssl-dev`) plus ALSA (`libasound2-dev`). For typing into other apps install `xdotool` (X11) or `wtype` (Wayland).

## Checks

```sh
npx tsc --noEmit -p .
cd src-tauri && cargo test
```

## Where things are

```
src/                     the pages (React)
  components/views/      Meetings, Upcoming (with the prep page), Settings
  components/Pill.tsx    the pill at the top of the screen
  styles/                the app's flat look
src-tauri/src/
  dictation.rs           dictation: listen, tidy, type
  meeting.rs             the notetaker: both sides of a call, live
  deepgram.rs            live speech to text
  chunked.rs             speech to text in pieces, with OpenAI
  notes.rs               meeting notes: naming, the summary, Markdown export
  prep.rs, ics.rs        pre-meeting notes and the calendar feed
  detect.rs              noticing a call from the window in front
  audio.rs               the microphone (and Windows loopback)
  system_audio*.rs       what the computer plays
  platform.rs            typing, the window in front, permissions, per system
  store.rs               the local database
```

Data lives in your system's app data folder under `Jelly Voice and Notes` (for example `~/Library/Application Support/Jelly Voice and Notes` on macOS). Set `JELLY_DATA_DIR` to use another folder. Exported notes go to `Downloads/Jelly notes`.

## License

Not chosen yet. See [LICENSE](LICENSE).
