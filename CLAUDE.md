# Ciel — guide for AI coding agents

Ciel is a Tauri 2 desktop app for Windows and Linux: a small cloud at the top of
the screen that shows AI coding agent sessions (Claude Code, Codex, Cursor,
Gemini CLI and more) and a few integrations, and lets the user approve, answer,
chat, dictate and drop files. It is a fork of Coucou's Windows/Linux app (MIT).

## Where things are
- `src/` — the front end, TypeScript with no framework. `src/cloud/` is the
  character: `shape.ts` (the outline, shared with the icon generator), `engine.ts`
  (states, emotes, eyes, drawing), `greeting.ts` (launch animation).
- `src-tauri/` — the Rust backend: windows, the named pipe / Unix socket the
  hooks talk to, the Claude API, pollers, dictation (`flow/`), notes, « OK Ciel » (`voice/`).
- `hook/` — `ciel-hook`, the relay every agent's hooks call.
- `src/i18n/strings.json` and `extra.json` — every interface string, keyed by its
  English text, in 10 languages. A new string goes into one of them in every language.
- `scripts/gen-icons.mjs`, `scripts/gen-sounds.mjs` — the icon and the 29 sounds,
  generated in code. Never commit art or sounds from elsewhere.
- `dev/*.html` — preview pages (`npx vite`), not shipped.

## Build and test
```
npm install
cargo build --release -p ciel-hook   # the app's build script needs it
npm run tauri dev
npx tsc --noEmit && npm test && cargo test --workspace
```

## Rules
- No telemetry. Network calls only to services the user configured.
- Secrets live in the OS keychain (Credential Manager / Secret Service), never on disk or in git.
- Never block an agent: if the app doesn't answer, `ciel-hook` exits at once and
  the agent asks in its own terminal. It never allows anything on its own.
- Never approve a permission or send an email without the user's explicit click.
- Agent config files (`~/.claude/settings.json` and the others): dated backup,
  merge only Ciel's entries, show the diff, write only after the user confirms.
- 0 % CPU when the island is hidden.
- Pill IDs are stable contract values (keychain, settings, hook routing): never rename one.
- Ciel's name, character, icon and sounds are original. Do not bring in Coucou's
  Mochi, icon, sounds or artwork (they are not MIT-licensed).
