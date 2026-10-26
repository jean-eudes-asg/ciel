# Changelog

All notable changes to Ciel. Versions follow [Semantic Versioning](https://semver.org).

## [Unreleased]

### Added
- **System**, a pill for this computer (after SysDash): CPU, memory, disk and
  temperature as rings with their last two minutes, green, orange or red past
  their limits, and the processes that weigh most, with End on your own (a
  second click ends it). **Warn me** (Settings → Integrations → System, off by
  default): a notification when one runs high, naming what uses the most, at
  most once an hour each. Ciel looks every 2 s while the card shows, every
  30 s with the warnings on, and not at all otherwise.

### Fixed
- Linux: Ciel's default shortcuts no longer take the desktop's own Ctrl+Alt
  keys (a terminal on Ctrl+Alt+T, the desktop on D, the workspaces on the
  arrows). On Linux, Open terminal is Ctrl+Alt+J, Ciel on the desktop
  Ctrl+Alt+O, and the next and previous pill Ctrl+Alt+Page Down and Page Up.
  Shortcuts you changed stay as you set them.

## [0.3.2] - 2026-10-26

### Added
- **Another name for Ciel** (Settings → OK Ciel → Also answers to): said
  first, it wakes Ciel like « OK Ciel » does, « Raphael, what time is it? »,
  for when the voice model never writes « Ciel » right for your voice.
- **A conversation**: after answering, Ciel listens 8 seconds more for your
  reply, without « OK Ciel », and again after each answer.
- **Kokoro, natural voices on your computer** (Settings → OK Ciel →
  Answers): downloaded once on a click, then made here without the internet.
  Six English voices to pick from, and French answers in its French voice.
- **Ciel starts speaking sooner**: with Kokoro, Kitten, ElevenLabs or a voice
  server, the answer is said a sentence at a time, the next one made while
  one plays.
- **A voice server for the answers** (Settings → OK Ciel → Answers): one on
  this computer or your network that speaks OpenAI's speech API, like
  VoiceStudio, with its voice picked from the ones it lists. When it doesn't
  answer, the system voice speaks.
- **Hide from screen sharing** (Windows, Settings → General): Ciel stays on
  your screen but is left out of screen shares, recordings and screenshots,
  so your sessions, approvals, mail and notes don't go out with a shared
  screen. Off by default.

### Fixed
- GNOME: the **Lock screen** option shows Ciel's picture, the sky or the flip
  clock. GNOME draws its lock screen from the desktop's background, so the
  picture Ciel set never showed; Ciel's GNOME extension now draws it there,
  unblurred, while the option is on. Install the extension again
  (Integrations → Notifications).
- « OK Ciel » wakes when « Ciel » is said in one syllable, which the voice
  model writes « see yeah », « C yeah » or, as what you say begins,
  « see you ».
- « OK Ciel » no longer answers at your first pause: what you say is heard
  whole, across pauses, and answered once you've stopped.
- The system voice says long answers to the end, and it's the same voice
  every time: the first answer no longer gets Windows' default one.
- Kitten, ElevenLabs and voice server answers are heard in their own voice:
  the pill played them in a way the app's own security policy refused, and
  the system voice said them instead.

## [0.3.1] - 2026-10-24

### Added
- **The flip clock on the lock screen** (Settings → Wallpaper → On the lock
  screen): the screensaver's clock on black instead of Ciel's sky, in its
  size and with its date, below the system's own clock. It is drawn again
  every minute while the computer is locked.

### Fixed
- Windows: the screensaver's **Preview** froze Ciel the first time. The
  clock's window is now made at launch, like Ciel's other windows.
- Windows: the flip clock could go away as soon as it came up, taking the Alt
  key Windows presses to bring it to the front for you coming back.

## [0.3.0] - 2026-10-24

### Added
- **Screensaver** (Settings → General → Screensaver, off by default): a flip
  clock on pitch black, like Fliqlo, on the main display only, after the
  minutes chosen without the mouse or a key. Three sizes, the date if wanted,
  and a Preview button. A key or the mouse sends it away.
- **Screens off at night**: during the night you set (23:00 to 06:00 by
  default), once the clock has shown long enough the screens go off; when the
  night ends they come back on, with the clock.
- **Flip clocks** in the island and on Home, and a **Clock** setting for 12 or
  24 hours (Automatic follows the language).
- **Calendar**, a new pill: your next meetings, from the secret iCal link of
  up to three calendars (Google Calendar, Outlook, iCloud, Fastmail…),
  with repeating meetings, time zones and the changes of time handled. No
  sign-in: the link is kept in the keychain, the meetings only in memory.
  Settings → Integrations → Calendar.
- **A card before each meeting**, 5 minutes ahead by default (or 1 to 15,
  or never): its time and who comes, **Join** for its Meet, Teams, Zoom or
  Webex room, and **Take notes**. It stays until the meeting has begun.
- **Notes that know the meeting**: started from the card, the Notetaker
  names the call after the meeting, its summary is told who was invited
  and the agenda, and renaming a speaker offers the guests' names.
- **« OK Ciel » knows your meetings and your calls**: what's next, who is
  invited, when you're free; what was decided in a call and what's left to
  do from it (from its summary, never from what was said).
- **What Claude may read** (Settings → Chat → Ciel's brain): a switch each
  for your calendar, your call notes and your spending. With Claude, the
  first two are on and spending is off until you turn it on. A local model
  reads all of it, as before; spending used to be for a local model only.
- **Kitten**, a voice on your computer for « OK Ciel »'s answers (Settings →
  OK Ciel → Answers): KittenML's small model through sherpa-onnx, downloaded
  once on a click (about 63 MB), then offline. A woman's or a man's voice.
  English only: an answer in another language gets the system voice.
- **Alerts**, a new pill: what Grafana, Alertmanager, Uptime Kuma or Zabbix
  send to Ciel's own address, on this computer only unless you let the
  network in (Settings → Integrations → Alerts). A new problem is a
  notification that opens the page its tool sent; the card lists what's
  still wrong, worst first, and a recovery takes its problem off. The
  address carries a token kept in the keychain; the alerts stay in memory.

### Fixed
- Linux on X11: out on the desktop, Ciel's eyes follow the pointer across the
  whole screen, not only over him.

## [0.2.7] - 2026-10-20

### Added
- **Updates from the app** (Settings → General → Updates): Check now, or
  once a day if you turn it on (off by default). A newer version installs on
  "Install and restart" only: Windows' installer, or the AppImage replacing
  itself. A .deb or .rpm is told, and gets the release page. Updates are
  signed, and Ciel refuses one whose signature doesn't match.
- **Home**, the page the Ciel window opens on: your sky right now, the real
  one with the sun and the moon where they are for you, and Ciel floating in
  it, with the greeting of the hour, what the sky does next and a line from
  sunrise to sunset. Under it, your day: words dictated today, time saved,
  the streak, the last dictations and the to-dos your calls left.

## [0.2.6] - 2026-10-20

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
- **The notifications list holds them all**: the island grows with it, up
  to three quarters of the screen, and only scrolls beyond; it gives the room
  back as they are opened or dismissed.

### Fixed
- Linux on X11: the island no longer stays down where GNOME's top bar was
  after the bar came back for a moment (the lock screen, GNOME starting the
  extension): it follows the top of the screen as it moves.

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

[0.3.2]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.3.2
[0.3.1]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.3.1
[0.3.0]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.3.0
[0.2.7]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.2.7
[0.2.6]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.2.6
[0.2.5]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.2.5
[0.2.4]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.2.4
[0.2.3]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.2.3
[0.2.2]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.2.2
[0.2.1]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.2.1
[0.2.0]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.2.0
[0.1.0]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.1.0
