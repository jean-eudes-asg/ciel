# Changelog

All notable changes to Ciel. Versions follow [Semantic Versioning](https://semver.org).

## [Unreleased]

### Added
- **YouTube Music**: a pill for what plays in a music.youtube.com tab, with
  the cover, the progress and the controls, through Ciel for Chrome. Its
  buttons do what the media keys do. On Windows the music pill leaves the
  browser to it meanwhile, so a song doesn't show twice.
- **Close when I click outside** (Settings → General, on by default): a click
  anywhere else folds the open island, as a popover; a card waiting for your
  answer stays. On Wayland, which shows no app the clicks in other windows,
  the island folds a moment after the pointer leaves it instead. Auto-close
  can now be Never, too.

### Changed
- **The open island makes room for what you have**: with one pill, it fills
  the island in detail — an agent's feed with its plan and the files it
  edited beside it, a card next to its detail list, a list in two columns, a
  bigger cover. With two pills, both are in detail side by side, each in its
  place (the main pill beside Ciel); a press on one gives it the keys.
  From three on, as before.

## [0.2.5] - 2026-10-20

### Added
- **Ciel keeps your notifications**: the ones you haven't opened stay, their
  apps' icons beside Ciel in the folded island, which stays on screen while
  any wait. A press there, or the bell in the header, opens the list; a press
  on one opens it in its app, its ✕ dismisses it, Clear empties the list.
- Linux: notifications show their app's icon, or the picture they came with.
- Linux: **your other apps' notifications in the island**: Thunderbird,
  Slack, system messages... Open brings the app forward. Off until you turn it
  on (Settings → Integrations → Notifications → Desktop apps). On GNOME the
  switch turns the desktop's own banners off too, and back on after.
- Linux, GNOME: **Open goes to the very chat or page**, as GNOME's own
  notifications do, through a small Ciel extension for GNOME Shell (Settings →
  Integrations → Notifications → Open in the app; Ciel shows what it writes
  first). Dismiss takes GNOME's copy away too.
- GNOME: **Hide GNOME's top bar** (Settings → General), so the island has the
  top of the screen; the bar still shows in the Activities overview. Done by
  Ciel's GNOME Shell extension: no other extension needed.

### Fixed
- Linux: Discord's notifications, and other apps' that address GNOME's
  notification server by its unique name, reach the island.
- Linux: Open no longer starts a second copy of the app (Discord's showed an
  error, Chrome opened a new window): without the extension, the app's own
  window comes forward.
- Linux on GNOME: with Desktop apps on, quitting Ciel gives GNOME its
  notification banners back (Ciel turns them off again when it starts), so
  notifications still show while Ciel is closed.

## [0.2.4] - 2026-10-20

### Added
- The time, in the middle of the island, folded or open. Handy where the
  desktop's own clock is hidden.

## [0.2.3] - 2026-10-19

### Fixed
- Windows: with dictation in the island, the pill no longer shows near the top
  left of the main display, where it also caught clicks.
- Windows: a hover at the top of the screen brings the hidden island back
  again. It came back only with an agent's message.

## [0.2.2] - 2026-10-19

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

### Fixed
- Settings → Wallpaper no longer calls the lock screen "the Windows lock screen"
  on Linux.

## [0.2.1] - 2026-10-19

### Fixed
- Linux: where live wallpapers can't run (GNOME, X11), the desktop gets Ciel's
  still sky instead of nothing at all.
- Linux on X11: no more frozen copy of Ciel left behind in the island after it
  folds.

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

[0.2.5]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.2.5
[0.2.4]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.2.4
[0.2.3]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.2.3
[0.2.2]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.2.2
[0.2.1]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.2.1
[0.2.0]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.2.0
[0.1.0]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.1.0
