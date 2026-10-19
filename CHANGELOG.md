# Changelog

All notable changes to Ciel. Versions follow [Semantic Versioning](https://semver.org).

## [Unreleased]

### Added
- **Notifications in the island**: the folded island slides out with the app's
  icon and title, and a card with Open and Dismiss shows under it, for as long
  as you choose (Settings → Integrations → Notifications), or until dismissed.
- **Ciel for Chrome**: an extension for Chrome, Edge and Brave that shows what
  websites notify — WhatsApp, Gmail, Calendar, Slack… — in the island instead
  of the browser's popups; Open brings the tab forward. Connect it in Settings,
  then load `Ciel-for-Chrome.zip` (in each release) unpacked.
- **Gmail**: a pill with the unread count and the latest mails, and new mail
  as a notification, over IMAP with an app password kept in your keychain.
  Ciel reads only who new mail is from and its subject.

### Changed
- **Dictation in the island**: while you dictate, the folded island shows
  it — the dots of your voice, hands-free's ✕ and ✓, a call's clock, short
  messages — and what « OK Ciel » heard and answered shows under it. The pill
  is still there for whoever prefers it: Ciel window → Dictation → Where
  dictation shows. A pill at the bottom, the old default, moves to the island
  once; a side you picked stays.

## [0.2.0] - 2026-10-19

### Added
- **Linux packages**: the AppImage, the `.deb` (Debian, Ubuntu 22.04+) and the
  `.rpm` (Fedora) come with every release, next to the Windows installers.
- **Plan usage at a glance**: while the island is folded, a ring per plan
  (Claude, Codex) beside Ciel shows how much of the busier window is gone, in
  green, orange or red. Hover one for its 5-hour and weekly card. Same switches
  and numbers as the plan pills: nothing new is read or fetched.
- **Now playing, any player** (Windows): the music pill shows whatever plays —
  Spotify, a browser, VLC, Media Player… — with the controls that player takes.
  Spotify keeps its volume and "Open Spotify". Linux still follows Spotify.

### Fixed
- The hub's little Clouds look at the pointer, not away from it vertically.
- Linux: on GNOME and other desktops without layer-shell, the island sits below
  the top bar instead of under it.
- Linux on X11: Ciel's eyes follow the pointer across the screen, not only over
  the island.

## [0.1.0] - 2026-10-19

The first release of Ciel: a little cloud at the top of your screen that keeps
an eye on your AI coding agents. Windows 10/11; Linux builds from source.

### Your agents
- Claude Code, Codex, Cursor, Gemini CLI, Copilot CLI, Antigravity, OpenCode,
  Amp, Hermes, Muse Code and Claude Code sessions from the Claude desktop app,
  live in the island: what each session reads, edits and runs.
- **Allow / Deny** permission requests and answer Claude Code's questions from
  the island. If Ciel isn't running, your agent simply asks in its own terminal.
- Live diffs of every file Claude edits.

### Ciel's brain
- Chat with Claude, OpenAI, Google AI, OpenRouter or a local model.
- **« OK Ciel »** voice commands, dictation clean-up and call summaries with
  Claude or **a model on your own computer** (Ollama, LM Studio): with a local
  model nothing leaves your PC. Ciel lets a local model think before it acts,
  which makes small models reliable with its tools.

### Your day
- **Flow dictation**: hold Right Ctrl anywhere, speak, let go. Offline, with
  Parakeet.
- **Notetaker**: record a call, get the transcript, a summary and the to-dos.
- **Finance**: a pill for your own spending, this month against your budget,
  with a quick form and « OK Ciel » ("I spent 12 euros on lunch"). Your data
  stays in two plain files in a folder you choose.
- Integrations: GitHub, Vercel, Stripe, n8n, Resend, Notion, Cal.com, Spotify.

### Wallpapers and lock screen
- Skies drawn by Ciel (clear, cloudy, golden, sunset, night, space, storm), lit
  by the real sun where you are, the weather following Ciel's mood.
- Live on the desktop, behind your icons; pauses when a window, a game or
  battery saver would make it wasted work. A still picture on the lock screen.
- Optional free photos from NASA and Wikimedia Commons, credited.
- Off until you turn it on, with one click to restore your own wallpaper.

### Also
- A demo mode, to see everything without an agent running.
- 10 interface languages, global shortcuts, a weekly recap, Ciel on your desktop.
- Original name, character, icon and 29 sounds, all made in code.

### Credits
Ciel is a fork of [Coucou](https://github.com/Louis-CFM/coucou) by Louis
Raillé (MIT). Thank you.

[0.2.0]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.2.0
[0.1.0]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.1.0
