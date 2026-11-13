# Changelog

All notable changes to Ciel. Versions follow [Semantic Versioning](https://semver.org).

## [Unreleased]

### Changed
- Islets draw back into the sky island as they go, instead of vanishing:
  when the island opens, when it hides, and when what they show is over.
- A second reply while the bubble shows brings its words up anew.
- The icons at the top of the open island go by importance: home, the
  agents, the chat, the calendar, the timers, today, the weather, the
  clipboard, the shelf, the controls. What needs you comes forward right
  after home: an agent asking, the notifications waiting, a timer running
  (its icon in blue) or rung (in amber). Resting the pointer on an icon
  goes there, no click needed.
- Each tool has the whole island: no small Ciel beside it any more.
- A new look for Controls: the sound and the brightness side by side, each a
  pill that fills in Ciel's blue with its icon and its value inside (the
  speaker still mutes), and a tile that is on takes its colour across the
  whole tile: blue, red for the microphones muted, amber for keeping awake,
  violet for do not disturb. The tiles share their row, their names in full.

### Fixed
- In the sky island, a plan's card hangs from the rings' islet, its tail at
  the ring, instead of from the island's right edge.

## [0.4.3] - 2026-11-13

### Added
- **A white island** (Preferences → General → Window → Island): black, white,
  or as the system, on its own from the Ciel window's colours. On white, Ciel
  and the little clouds get a soft shadow for an edge, and coloured text takes
  the deeper shade of its colour.
- **The sky island** (Preferences → General → Window → Shape): the island let
  go of the screen's edge, a pill floating just under it, in black or white.
  With no edge to slide into, it hides by drawing in to a dot that fades,
  and comes back from that dot. The notch stays the default.
- **Islets**, around the sky island: what the notch carries inside it, the
  sky island sets down beside it, each thing on a small island of its own.
  Folded, the other agents' clouds and the notifications float to its left,
  what is going on now (a timer, the microphones off, rain coming, the line
  of a song) and each plan's ring to its right, and the time keeps its
  place. Open on the overview, the pills are a row of islands under it and
  the card has the whole width. An islet buds out as it comes. The line of
  the song being sung has a bubble of its own under the island, tied to it
  by a little round one, as a thought is drawn; each new line comes up into
  place.
- **Replies in a bubble**, in the sky island: when an agent finishes, what
  it said comes up in a bubble under the folded island, beside that agent's
  own little cloud, for a few seconds; a click opens the island on its card.
  A permission or a question still opens the island. Preferences → General →
  Window → Replies puts it back to opening the island each time.
- **Don't ask again**, on a permission card, beside Allow and Deny: Ciel
  remembers your answer and allows the same kind of request from that agent
  in that project at once, with no card. A line above the buttons says what
  will be remembered before you click: a command word for word, or its first
  words when Claude Code suggests them (`npm test …`, never a command that
  chains, pipes or redirects); a website, for fetches; one MCP tool or web
  searches; file edits inside the project folder (Claude Code, hidden folders
  such as `.git` left out). Each time a rule answers, the session says
  "Allowed by your rule". The rules are in **Settings → Agents → Remembered
  answers**, each with **Remove**; they stay on your computer. For Claude
  Code, Codex, Copilot CLI and Muse Code. A paused Ciel answers nothing, and
  a question or a plan to approve is never remembered.
- **Every agent session in one list** (the prompt icon in the island's
  header): how many need you, work or are done on top, then a row a session
  with that agent's own little cloud in its mood, what it was asked, what it
  is doing now, since when and in which project. Two Claude Code sessions
  have a row each, where the pill shows only the one that spoke last. The
  row that waits for you says what it asks, with Deny and Allow on the row;
  a press on a row opens it, and its arrow brings that session's own
  terminal window to the front. The icon wears a badge while a card waits.
- **Today, in tiles** (the sun in the header, and what the island opens on
  with nothing running): the week with today lit and a dot on the days that
  hold something, the weather with the day's high and low, the timer on a
  ring that fills as it runs, your agents as their little clouds, what comes
  next in your calendar, and what waits on the shelf or in the clipboard.
  Each tile opens the tool behind it.

### Changed
- **One row of icons in the island's header**, each one move from any other:
  home, the agents, today, the chat, then every tool by itself: timers, the
  clipboard, the shelf, the controls, the calendar and the weather. The Tools
  tab and its second row of tabs are gone, a dot sits under the icon you are
  on, and each tool has the room that row took. The time sits in what the
  icons and the buttons leave free between them, never over an icon.
- **The notch grows out of the screen's edge**: where its sides meet the
  edge they curve outward into it, a small shoulder each side, rounder when
  the island is open. The sky island floats free and has none.
- **New sounds**, all 29, low and few-noted. What you touch answers with a
  tap on wood: the island opening and closing, a key, a file attached. What
  asks for you or tells you something rings a deep bell once or twice and
  lets it fade: a permission, a question, a task done. They share one voice
  and one key, so the island sounds like one instrument. Still made in code (`npm run sounds`); a sound of your own in
  the sounds folder still replaces Ciel's.
- **The island moves better**: it opens wide a touch ahead of its fall and
  lands with a little give, folds up before it draws in, and what it shows
  comes into place with a spring. Changing view, one sinks
  back as the next rises. Buttons and tabs spring back when let go. With
  reduced motion asked of the system, only the fades remain.
- **Ciel is more alive.** His body follows on springs, so he overshoots and
  settles like jelly, and he leans and stretches when he is carried across
  the island. With the island open and in use he breathes, his eyes dart, and with
  nothing to do he fidgets: a look aside, a slow blink, a stretch, a little
  hop. Each state has its own way in: a crouch and a hop off to work, a head
  cocked and a drifting gaze while he thinks, a landing after the roll when a
  task is done, a sigh as he falls asleep.
- **He speaks with his mouth**: while « OK Ciel » answers out loud, the
  island's Ciel moves his mouth to the rhythm of speech and nods along.

## [0.4.2] - 2026-11-10

### Added
- **Answers during a call** (Ctrl+Alt+K, Settings → Shortcuts): while the
  Notetaker records, Ciel finds the last question the others put to you in
  the live transcript and suggests an answer on a card in the island, with
  your Claude key or a local model. Pressed before the call is noted, it
  asks first: Take notes on its card starts the Notetaker. Each question and
  answer stays in the call's note (Ciel window → Notetaker). While an answer
  is on screen, Ciel shows in screen shares even with Hide from screen
  sharing on, so the others see it too; the rest of the call it stays out.
  **Show answers in screen shares** (Settings → General, under Hide from
  screen sharing) turns that off: then only you see the answers.
  - **At once**: the card shows during the call, under its strip. It says
    Ciel is thinking the moment the key is pressed, and the answer is written
    into it as it comes: first a sentence you can say as it is, then a figure
    or what to check. Whole, it has **Copy** for the clipboard. The last
    words said are transcribed at the press, after what the live transcript
    is still writing, so the question just asked is never left out. A button
    on the Notetaker's strip, in the island and on the pill, does the same as
    the key.
  - **Prepare answers ahead** (Settings → Island → Chat, off by default): when
    the others ask you a question, Ciel writes the answer before you press
    the key, and the key's button lights up; the press then shows it at once.
    With Claude, up to the last half hour of the call goes to Anthropic with
    each question, pressed or not.
  - **Your company folder** (Settings → Island → Chat): read when the call
    starts, then again only where a file changed, so the answers are always
    in step with it. The passages that fit the question go to the brain with
    it, a rare word counting for more than a common one. Text files for now
    (Markdown, text, CSV, JSON, YAML, HTML).
  - **Earlier calls** on the same subject help too: their summary and what
    is still to do (with Claude, when Ciel's brain may read the notes).
  - With Claude, the answer key reads up to the last half hour of the call
    (the part already said is cached from one question to the next); with a
    local model, the last three minutes.
  - The live transcript keeps up better with the others: it looks for a pause
    four times a second, and writes their words at least every six seconds.

### Fixed
- Linux: Log out in the power menu (Tools → Controls) uses the desktop's own
  on KDE Plasma, Cinnamon, Xfce, MATE and LXQt, so apps can save first.
  Elsewhere it ends only Ciel's session, no longer every session you have
  (an SSH login, a tmux).

## [0.4.1] - 2026-11-09

### Added
- **Quick settings in the island** (Tools → Controls), everything GNOME's
  menu in the top bar holds, and more, on Linux and Windows:
  - **Screenshot**, **Settings**, **Lock**, and a **power menu** (suspend,
    restart, power off, log out; a second click confirms the last three).
  - The sound with **the speakers to choose from**, and the screens'
    brightness.
  - **Network** (wired, or Wi-Fi by its name) with its connections to bring
    up or down on Linux, **VPN**, **power mode** (power saver, balanced,
    performance), **night light**, **dark style**, **do not disturb** (GNOME's
    banners and Ciel's own cards wait; they stay in the list), the
    **battery** on a laptop, Bluetooth, the microphones and keep awake.
  - **Your apps** in one click: GNOME's dock favourites on Linux, the apps
    pinned to the taskbar on Windows, with their icons.
  With the GNOME top bar hidden (Settings → General), the island takes its
  place. Install Ciel's GNOME extension again (Settings) for the screenshot
  button on Wayland.
- **Calendar** (Tools → Calendar, or the date on the Today card), as the
  desktop's clock menu: the month with a dot on each day that has events
  from your calendars (the Calendar pill's links), a day's events, and
  **world clocks** for the cities you add, with the hours between you.

### Fixed
- Linux: the volume and the microphones work on systems with PipeWire but
  without PulseAudio's tools (Ubuntu 24.04 and Fedora by default).
- The lyrics come back in the folded island when the song changes, without
  opening the island, and after a song's intro or a solo.
- A long line of lyrics no longer pushes the music card past the island's edge.

## [0.4.0] - 2026-11-07

### Added
- **Games** (Ciel window → Games), five of them for a break between two
  tasks. Chess, draughts and the maze are played against five clouds of
  Ciel's family, each with his face, his voice, his sky and his way of
  playing: Cirrus the dreamer, cheerful Cumulus, Ciel himself, calm Stratus
  at night and Nimbus, the storm.
  - **Chess** and **international draughts** (10x10, flying kings, the
    most pieces must be taken): pick your opponent, click or drag your
    pieces, take a move back, play either side; a hint in chess.
    The board takes the colours of his sky, and he takes a person's time over
    his moves. With a brain (your Claude key or a local model) he says a word
    now and then, in his own character; his moves are always his own
    player's, on this computer. An unfinished game waits for you.
  - **A maze** in his living sky, a new one at each level, a little bigger
    each time, with a new rival every two levels from Cirrus's morning to
    Nimbus's storm. Race him home or play alone, find the three stars in the
    dead ends, or play at night with only your light.
  - **Clouds**: slide the clouds along their length until Ciel can float out
    to the sun. A new puzzle at each level, harder each time, with the fewest
    moves it takes and a hint.
  - **Constellations**: join each pair of stars with lines that never
    cross, in a night sky; fill every point for a perfect one, and Ciel's
    brain names the constellation you drew.
  - **Their own window**: take the games out of the Ciel window to play
    beside your work, as big as you like, kept over the other windows if
    you want, and back into the Ciel window in one click.
  - **Sounds**: pieces on wood, captures, check, steps, stars, the
    countdown, a tune at the end, the wind and the storm, all made in code
    and following Ciel's sound switch and volume.
- **Tools**, a new tab in the open island (the four squares), for the
  everyday things, whether you code or not:
  - **Timers and reminders**: type "5", "25m tea", "1h30" or "18:30 call
    mum", or pick a length. A little cloud of Ciel's shape rains into a
    glass as the time runs out. Reminders can come back every day, on
    weekdays or every week, with Snooze. **Focus**: 25 minutes, 5 of break,
    a longer break every fourth round. They ring with the island shut, and
    one missed while Ciel was closed rings when it starts.
  - **Clipboard history**, off until you turn it on: text, links, pictures
    and files, kept in memory on this computer, the pinned ones on disk. A
    click pastes it where you were. Links lose their trackers, colours show
    their HEX, RGB and HSL, sums their result, JSON can be laid out and text
    can change case. What password managers copy is never kept, and
    one-time codes go after two minutes. **Ctrl+Alt+V** opens it, once
    turned on in Preferences → Shortcuts (Word's Paste Special is on the same
    keys).
  - **Shelf**: what you drop on Ciel goes there and waits a week, several
    files at once too. A drop and the shelf are one place: after a drop,
    ask a question about the file or open the shelf; from the shelf, ask
    about any file, drag it out into any app, show it in its folder or copy
    it (a double click opens it).
  - **Controls**: the volume, the screens' brightness (a laptop's own, and
    monitors through DDC/CI), Wi-Fi, Bluetooth, night light on GNOME, and
    **Keep awake** for half an hour up to until you say. **Every microphone
    at once**: **Ctrl+Alt+U** mutes them all, and the folded island says
    "Mic off".
  - **Weather** from Open-Meteo, free and without an account, once you pick
    your town: now, the next hours and the week. **Rain alerts** (off by
    default): a card when rain is about to start. **The sky wears the
    weather** (off by default): Ciel's sky wallpaper turns grey or stormy
    with the weather outside, and follows his mood on clear days.
- **Live activities in the folded island**, in place of the clock: a timer's
  ring counting down, "Mic off", rain coming, a cup while the computer is
  kept awake, and the line of the song being sung.
- **Today**: with nothing running, the island opens on a greeting, the date,
  the weather, the next timer and what waits on the shelf or in the
  clipboard, each a door into its tool.
- **Ciel without a coding tool** (Island → Active pills → Main tool →
  None): no pill is always on, and the island is yours for the rest.
- **Lyrics** (off by default, the speech-bubble button on the music card):
  the words of the song playing, timed, from lrclib.net, under its title.
- **Linux: music from any player**, VLC, a browser, Rhythmbox…: the one
  playing, Spotify first when none plays.
- **« OK Ciel »** sets timers and reminders, says the weather where you are,
  mutes the microphones, sets the volume and keeps the computer awake.
- **Island → Tools** in the Ciel window: the clipboard, lyrics and weather
  options together.
- **A setup**, the first time the Ciel window opens: seven short steps over
  the whole window (the language and the window's colours, what you code
  with and its hooks, how the island opens, the everyday tools, dictation,
  the pills, the sky on your desktop) and the keys worth knowing. Each step
  you finish lights a star on Ciel's outline in the night beside it; at the
  end he is drawn whole and opens his eyes. Put off, it waits at the foot of
  the menu until it is done (Preferences → General → Setup to go through it
  again).

### Changed
- **The Ciel window, redesigned.** Five places instead of eighteen pages
  under "Flow" and "Notch": **Today** (your day, the games), **Island** (the
  pills, the agents, the tools, the integrations, the chat), **Voice**
  (dictation, the Notetaker, « OK Ciel », the statistics, the dictionary,
  the snippets, the styles and their settings), **Sky** (wallpaper,
  screensaver) and **Preferences** (general, shortcuts, updates and
  privacy). Every page has the same plain title and the same rows: the
  setting on the left, its control on the right, the explanation beneath.
  Pitch black, or white (Preferences → General → Window), in Ciel's rounded
  type; no coloured bands or glows any more, a small Ciel beside the page
  you are on.
- **No more + tab in the island**: drop files on Ciel and they go on the
  shelf (Tools → Shelf), where you can ask about them or drag them out.

### Fixed
- **Weather in °F outside the US**: on an English (US) system, Automatic
  units showed Fahrenheit wherever you were. Picking a place now sets the
  units its country uses, the setup asks for °C or °F, and the temperature
  says which it is. A place picked before keeps its units: click °F on the
  weather card to switch.
- Turning off the desktop or the lock screen (Sky → Wallpaper) puts
  your own picture back. Ciel's last one stayed there, like a flip clock
  stopped at the minute it was drawn, until **Restore my previous
  wallpaper**. A picture left behind that way goes the next time Ciel
  starts.

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

[0.4.3]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.4.3
[0.4.2]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.4.2
[0.4.1]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.4.1
[0.4.0]: https://github.com/jean-eudes-asg/ciel/releases/tag/v0.4.0
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
