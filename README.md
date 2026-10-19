<div align="center">

<img src="src-tauri/icons/icon.png" width="112" alt="Ciel, a small white cloud with round eyes and a smile">

# Ciel

**A little cloud at the top of your screen that keeps an eye on your AI coding agents.**

Approve Claude Code's permission requests, answer its questions, watch every
session work and read its diffs, without leaving what you're doing. Plus
dictation anywhere, a call notetaker and « OK Ciel », all running on your own
computer.

Windows 10/11 · Linux (X11 and Wayland) · Tauri 2 · Rust + TypeScript · MIT

<img src="docs/banner.png" width="760" alt="Ciel in five moods: waiting for your OK, thinking, at rest, done, and asking a question">

</div>

---

## What it does

- **Your agents, live.** Claude Code, Codex, Cursor, Gemini CLI, Copilot CLI,
  Antigravity, OpenCode, Amp, Hermes, Muse Code and Claude Code sessions from the
  Claude desktop app. Ciel shows what each session reads, edits and runs, step by
  step, and does a happy little jump when it finishes.
- **Allow / Deny from the island.** A permission request opens the island with
  **Deny / Allow**. A question from Claude Code shows its options to
  pick from. Nothing is ever approved without your click, and if Ciel isn't
  running, your agent simply asks in its own terminal as usual.
- **Live diffs.** Every file Claude edits shows up with its **+N −M** lines;
  click it to read the diff.
- **Chat** with Claude, OpenAI, Google AI, OpenRouter or a local model (Ollama,
  LM Studio) from the island, with your own keys.
- **Integrations**, each with its own coloured mini cloud: GitHub, Vercel,
  Stripe, n8n, Resend, Notion, Cal.com and Spotify.
- **Finance.** A pill for your own spending: this month against your budget,
  the top categories, the latest entries, and a quick form to add an expense.
  Everything stays in two plain files in a folder you choose
  (`expenses.csv` opens in Excel). With a local brain, tell « OK Ciel » "I
  spent 12 euros on lunch" or ask how much you spent this month.
- **Flow dictation.** Hold **right Ctrl** anywhere, speak, let go: your words
  are pasted where you type. It uses [Parakeet](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3),
  running offline on your CPU.
- **Notetaker.** Record a call (your microphone and what the computer plays),
  then get the transcript person by person, a summary and the to-dos.
- **« OK Ciel ».** Say it (the French way, "see-ell"), then ask: your pills,
  music, your services' numbers, web search, open an app. Claude answers with
  your own key, or the local model you picked for the chat (**Settings → Chat →
  Ciel's brain**, which also cleans up dictations and writes the call
  summaries). With a model running on your computer, nothing leaves it; web
  search needs Claude.
- **Wallpapers and lock screen.** Skies drawn by Ciel (clear, cloudy, golden,
  sunset, night, space, storm) lit by the real sun where you are, the weather
  following Ciel's mood, and Ciel himself on top. Live on the desktop (it pauses
  when a window or game covers it), a still picture on the lock screen, and
  optional free photos from NASA and Wikimedia Commons, credited. Off until you
  turn it on in **Settings → Wallpaper**, with one click to restore your own.
- **Ciel on your desktop.** Drag him out of the island (or press `Ctrl+Alt+D`)
  and he floats on your desktop, follows your cursor, and flies back when an
  agent needs you.
- **Weekly recap**, global shortcuts, and 10 interface languages.
- **Private by design.** No telemetry, no account. Keys live in Windows
  Credential Manager or the Linux Secret Service. Your voice never leaves the
  computer; only text goes to the AI providers you configure.

## Run a local model

Ciel's chat, « OK Ciel », dictation clean-up and call summaries can all run on a
model on your own computer, with no key and nothing sent anywhere. The easiest
way is [Ollama](https://ollama.com), natively or in Docker:

```powershell
docker run -d --name ollama --gpus all -p 11434:11434 -v ollama-models:/root/.ollama --restart unless-stopped ollama/ollama
docker exec ollama ollama pull qwen3.5:9b
```

Then in Ciel: **Settings → Chat**, connect Ollama and pick the model, and set
**Ciel's brain** to *Local model* (or *Automatic*). `qwen3.5:9b` (6.6 GB) runs
well on an 8 GB graphics card; on less, try `qwen3.5:4b`. Ciel asks the model to
think before it acts on « OK Ciel », which makes a small model reliable at
using its tools.

## Install

There are no prebuilt releases yet: [build it yourself](#build-it-yourself).
`npm run pack` makes a Windows installer you can keep and share.

## Using it

There is no notch on a PC, so the island slides out of the top edge of the screen.

| What you do | What happens |
|---|---|
| Move the mouse to the very top-centre of the screen | Ciel peeks out |
| Click the small island | It opens. With **Settings → General → Open on hover**, resting the pointer on it is enough |
| Click Ciel | He gets annoyed. Three times in a row and he goes dizzy |
| Drag Ciel out of the island | He floats on your desktop. Drop him back on the island to bring him home |
| Drag a file onto the island | Ciel turns into a box, swallows it, and offers to answer questions about it |
| Click a file in the session ticker | Its diff opens in the island |
| Hold **right Ctrl**, speak, let go | Dictation, pasted where you type. Tap twice for hands-free, `Esc` cancels |
| `Esc` | Closes the island |
| Tray icon | Open, Weekly recap, Settings…, Pause, Quit |

Global shortcuts (all can be changed or turned off in **Settings → Shortcuts**):

| Shortcut | Does |
|---|---|
| `Ctrl+Alt+Space` | Opens the chat, from any app |
| `Ctrl+Alt+A` | Jumps to the waiting permission or question |
| `Ctrl+Alt+T` | Brings the session's window forward |
| `Ctrl+Alt+→` / `Ctrl+Alt+←` | Next / previous pill |
| `Ctrl+Alt+S` | Mutes or unmutes Ciel |
| `Ctrl+Alt+D` | Sends Ciel to the desktop, or brings him home |
| `Ctrl+Alt+N` | Opens and closes the island (off until you turn it on) |

The defaults avoid the `AltGr` layer of common European keyboard layouts, and a
combination another app already holds is flagged *In use*.

## Connecting your agents

Open **Settings → Claude Code → Install hooks…** (and the matching page for each
other agent). You see the exact diff of what will change, the path of the dated
backup that will be taken first, and nothing is written until you click. Your
own hooks are never touched, and uninstalling removes only Ciel's entries.

| Agent | Installs | Permissions |
|---|---|---|
| Claude Code | `.claude\settings.json` | Allow / Deny and questions in the island |
| Codex | `.codex\hooks.json` (then trust the hooks once with `/hooks` in Codex) | Allow / Deny in the island |
| GitHub Copilot CLI | `.copilot\hooks\ciel.json` | Allow / Deny in the island |
| Muse Code | `.config\muse\settings.json` | Allow / Deny in the island |
| Gemini CLI | `.gemini\settings.json` | asked in Gemini CLI |
| Antigravity | `.gemini\config\hooks.json` | asked in Antigravity |
| Cursor Agent | `.cursor\hooks.json` | asked in Cursor |
| Claude desktop app | nothing: its Claude Code sessions are recognised by the relay | asked in the app |
| OpenCode | plugin `.config\opencode\plugins\ciel.js` | asked in OpenCode |
| Amp | plugin `.config\amp\plugins\ciel.ts` | asked in Amp |
| Hermes Agent | plugin `.hermes\plugins\ciel\` (then `hermes plugins enable ciel`) | asked in Hermes |
| Anything else | run `ciel-hook --agent <name> [<Event>]` from your tool's hooks | asked in the tool |

Agents talk to Ciel through `ciel-hook`, a tiny relay, over a named pipe on
Windows or a Unix socket on Linux. Only processes running as your user can use
either. If Ciel doesn't answer, the relay prints nothing and your agent asks in
its own terminal: it never allows anything on its own.

## Build it yourself

You need [Node 22.18+](https://nodejs.org), [Rust](https://rustup.rs) and, on
Windows, the **Visual Studio Build Tools** with "Desktop development with C++".
WebView2 ships with Windows 10/11.

```powershell
npm install
npm run tauri dev      # live-reloading development build
npm run pack           # builds the installer into release/
```

If Coucou is installed too, uninstall its hooks first (in Coucou's settings), so
only one app handles your agents' requests.

Other useful commands:

```powershell
npm test               # front-end tests
npx tsc --noEmit       # type check
cargo test --workspace # Rust tests (build ciel-hook first: cargo build --release -p ciel-hook)
npm run icons          # redraws src-tauri/icons from scripts/gen-icons.mjs
npm run sounds         # re-synthesizes the 29 sounds in sounds/ from scripts/gen-sounds.mjs
```

`npx vite` alone serves the front end in an ordinary browser, which is enough to
work on the looks. `dev/cloud-preview.html` shows Ciel in every state and emote,
the mini clouds, the drop box and the launch greeting.
`dev/upload-preview.html` replays the file-drop animation, and
`dev/recap-preview.html` the weekly recap card. None of these pages ships in the
app.

Ciel, his icon and his sounds are all made in code: the outline in
`src/cloud/shape.ts`, the character in `src/cloud/engine.ts`, the icon in
`scripts/gen-icons.mjs` and the sounds in `scripts/gen-sounds.mjs`.

### Layout

```
src/                 island front end (TypeScript, no framework)
  cloud/             Ciel: shape, engine, launch greeting, desktop logic
  desktop/           Ciel's own little window, when he lives on the desktop
  island/            state machine, hooks, integrations
  views/             every island view
  hub/               the Ciel window: Flow, the Notetaker, statistics
  settings/          the settings window
src-tauri/           Rust backend: window, named pipe, Claude API, pollers, dictation
hook/                ciel-hook, the agents' relay
scripts/             icon and sound generators, packaging
sounds/              the 29 sounds (generated)
dev/                 preview pages for development
tests/               front-end tests (node --test)
```

### Log

`%LOCALAPPDATA%\Ciel\ciel.log` on Windows, `~/.local/share/ciel/ciel.log` on
Linux: hook events, permission decisions and integration problems. It stays on
your machine.

## Credits

Ciel is a fork of [Coucou](https://github.com/Louis-CFM/coucou) by Louis Raillé,
whose Windows and Linux app it builds on, under the MIT License. Coucou's name,
its character Mochi, its icon and its sounds belong to their author and are not
part of Ciel: Ciel's name, cloud character, icon and sounds are its own. Ciel is
not made or endorsed by Coucou's author.

Speech recognition uses NVIDIA's Parakeet through
[sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx).

## License

[MIT](LICENSE).
