// Everything the demo shows: a made-up project, made-up pull requests, a
// made-up month of spending. No real person, account, email or path appears
// here, and nothing is read from the user's machine. Demo content stays in
// English (file names, commands, PR titles); the island around it is
// translated as usual.

import { DEFAULT_SETTINGS, type AskedQuestion, type IntegrationInfo, type Settings } from "../core/state";
import { FINANCE_ID, localDate, type FinanceEntry, type FinancePoll, type FinanceSummary, type NewExpense } from "../core/finance";
import type { ContributionDay, GitHubActivity, GitHubPR, GitHubRepoCI } from "../core/github";

// ── Who and where ─────────────────────────────────────────────────────────────

export const CLAUDE_ID = "integration_claude";
export const CODEX_ID = "agent_codex";
export const GITHUB_ID = "integration_github";
export { FINANCE_ID };

export const DEMO_PROJECT = "ciel-demo";
export const DEMO_CWD = `/projects/${DEMO_PROJECT}`;
export const CODEX_CWD = "/projects/ciel-demo-api";
export const CLAUDE_SESSION = "demo-session-claude";
export const CODEX_SESSION = "demo-session-codex";

/** Every request the demo makes carries this prefix: the gate answers those itself. */
export const DEMO_REQUEST_PREFIX = "demo-";
export const isDemoRequest = (id: unknown): boolean => typeof id === "string" && id.startsWith(DEMO_REQUEST_PREFIX);

/** The keys the demo pretends to hold (only whether one exists is ever asked). */
export const DEMO_KEYS: ReadonlySet<string> = new Set(["github-token", "anthropic-api-key"]);

/** The hook-driven pills the demo pretends are installed. */
export const DEMO_HOOKS: Readonly<Record<string, boolean>> = { [CLAUDE_ID]: true, [CODEX_ID]: true };

/**
 * The settings the island runs on during the demo: the person's own choices
 * about how it feels (language, sound, hover, display, wallpapers — the same
 * object, so the wallpaper controller sees no change) and the demo's pills.
 */
export function demoSettings(real: Settings, overrides: Partial<Settings> = {}): Settings {
  return {
    ...real,
    mainPill: CLAUDE_ID,
    activeIntegrations: [GITHUB_ID, FINANCE_ID],
    autoCloseInterval: 30,
    hooksInstalled: true,
    model: DEFAULT_SETTINGS.model,
    showPlanInNotch: false,
    showCodexPlanInNotch: false,
    chatProvider: "anthropic",
    chatModels: {},
    ollamaUrl: "",
    lmstudioUrl: "",
    customUrl: "",
    pillColors: {},
    ...overrides,
  };
}

// ── The Claude Code session ───────────────────────────────────────────────────

export const PROMPT = "Retry failed uploads, and add tests";

export const READ_CLIENT = { file_path: `${DEMO_CWD}/src/upload/client.ts` };

export const SEARCH_UPLOADS = { pattern: "upload(", path: `${DEMO_CWD}/src` };

export const EDIT_CLIENT = {
  file_path: `${DEMO_CWD}/src/upload/client.ts`,
  old_string: [
    "export async function upload(file: Blob, url: string) {",
    "  const res = await fetch(url, { method: \"PUT\", body: file });",
    "  if (!res.ok) throw new Error(`Upload failed: ${res.status}`);",
    "  return res;",
    "}",
  ].join("\n"),
  new_string: [
    "const MAX_RETRIES = 3;",
    "",
    "export async function upload(file: Blob, url: string) {",
    "  for (let attempt = 0; ; attempt++) {",
    "    const res = await fetch(url, { method: \"PUT\", body: file });",
    "    if (res.ok) return res;",
    "    if (attempt >= MAX_RETRIES || res.status < 500) {",
    "      throw new Error(`Upload failed: ${res.status}`);",
    "    }",
    "    await sleep(250 * 2 ** attempt);",
    "  }",
    "}",
  ].join("\n"),
};

export const WRITE_TEST = {
  file_path: `${DEMO_CWD}/tests/retry.test.ts`,
  content: [
    "import { expect, test } from \"vitest\";",
    "import { upload } from \"../src/upload/client\";",
    "import { flakyServer } from \"./helpers\";",
    "",
    "test(\"a 503 is retried, then succeeds\", async () => {",
    "  const server = flakyServer([503, 503, 200]);",
    "  const res = await upload(new Blob([\"hi\"]), server.url);",
    "  expect(res.ok).toBe(true);",
    "  expect(server.calls).toBe(3);",
    "});",
    "",
    "test(\"a 404 fails at once\", async () => {",
    "  const server = flakyServer([404]);",
    "  await expect(upload(new Blob([\"hi\"]), server.url)).rejects.toThrow(\"404\");",
    "});",
    "",
  ].join("\n"),
};

export const TEST_COMMAND = { command: "npm test", description: "Run the test suite" };

export const QUESTION: AskedQuestion = {
  question: "How should a failed upload be retried?",
  options: [
    { label: "Exponential backoff", description: "Wait 250 ms, then 500 ms, then 1 s" },
    { label: "Fixed delay", description: "Wait 1 s between tries" },
    { label: "Don't retry", description: "Fail at once and show the error" },
  ],
  multiSelect: false,
};

export const EDIT_BACKOFF = {
  file_path: `${DEMO_CWD}/src/upload/client.ts`,
  old_string: "    await sleep(250 * 2 ** attempt);",
  new_string: [
    "    // 250 ms, 500 ms, 1 s — with a little jitter so clients don't retry in step.",
    "    await sleep(250 * 2 ** attempt + Math.random() * 100);",
  ].join("\n"),
};

export const FINAL_LINE =
  "Added retries with exponential backoff to the upload client. All 24 tests pass.";

// ── The Codex session, alongside ──────────────────────────────────────────────

export const CODEX_PROMPT = "Document the retry behaviour in the API docs";

export const READ_DOCS = { file_path: `${CODEX_CWD}/docs/api.md` };

export const EDIT_DOCS = {
  file_path: `${CODEX_CWD}/docs/api.md`,
  old_string: [
    "## Uploads",
    "",
    "`PUT /files/:id` stores the file in one request.",
  ].join("\n"),
  new_string: [
    "## Uploads",
    "",
    "`PUT /files/:id` stores the file in one request.",
    "Clients retry a 5xx up to 3 times, waiting 250 ms, 500 ms, then 1 s.",
    "A 4xx is never retried.",
  ].join("\n"),
};

export const CODEX_FINAL_LINE = "Documented the retry behaviour in docs/api.md.";

// ── The chat ──────────────────────────────────────────────────────────────────

export const CHAT_QUESTION = "What changed in ciel-demo today?";

export const CHAT_ANSWER = [
  "Three things landed in **ciel-demo** today:",
  "",
  "- Uploads retry with exponential backoff (`src/upload/client.ts`).",
  "- New tests cover the retry path, and all 24 pass.",
  "- Codex documented the retry behaviour in `docs/api.md`.",
  "",
  "PR #42 is green and ready for review.",
].join("\n");

// ── GitHub ────────────────────────────────────────────────────────────────────

const ORG = "demo-org";
const repoUrl = (repo: string) => `https://github.com/${ORG}/${repo}`;

function pr(repo: string, number: number, title: string, rest: Partial<GitHubPR>): GitHubPR {
  return {
    id: `${ORG}/${repo}#${number}`, title, url: `${repoUrl(repo)}/pull/${number}`, repo: `${ORG}/${repo}`,
    number, isDraft: false, ci: "success", review: "pending", headSha: null, ...rest,
  };
}

/** The pull request whose CI the demo watches turn green. */
export const WATCHED_PR = `${ORG}/ciel-demo#42`;

/** Two open PRs, one to review, and the default branches' CI: running, or passed. */
export function githubData(ciPassed: boolean, now: Date): Record<string, unknown> {
  const ci = ciPassed ? "success" : "pending";
  const mainCI: GitHubRepoCI[] = [
    { repo: `${ORG}/ciel-demo`, url: repoUrl("ciel-demo"), branch: "main", ci, headSha: null },
    { repo: `${ORG}/ciel-demo-api`, url: repoUrl("ciel-demo-api"), branch: "main", ci: "success", headSha: null },
  ];
  return {
    totalRepos: 12,
    totalStars: 248,
    pulse: {
      login: "ciel-demo",
      myPRs: [
        pr("ciel-demo", 42, "Retry failed uploads with backoff", { ci }),
        pr("ciel-demo", 39, "Dark mode for the settings page", { isDraft: true, review: "approved" }),
      ],
      toReview: [pr("ciel-demo-api", 17, "Rate-limit the upload endpoint", { ci: "unknown" })],
      mainCI,
      fetchedAt: 0,
    },
    activity: contributions(now),
  };
}

/** Half a year of made-up contributions, the same every time for a given day. */
export function contributions(now: Date, weeks = 26): GitHubActivity {
  let seed = 7;
  const next = () => (seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648;
  const end = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const start = new Date(end);
  start.setDate(end.getDate() - end.getDay() - (weeks - 1) * 7);
  const out: ContributionDay[][] = [];
  let total = 0;
  for (const day = new Date(start); day <= end; day.setDate(day.getDate() + 1)) {
    const weekday = day.getDay();
    if (weekday === 0 || out.length === 0) out.push([]);
    const r = next();
    const weekend = weekday === 0 || weekday === 6;
    const count = r < (weekend ? 0.55 : 0.15) ? 0 : Math.round(r * (weekend ? 4 : 11));
    const level = count === 0 ? 0 : count < 3 ? 1 : count < 6 ? 2 : count < 9 ? 3 : 4;
    total += count;
    out[out.length - 1].push({ date: localDate(day), count, level, weekday });
  }
  return { total, weeks: out, fetchedAt: 0 };
}

// ── Finance: a month of made-up spending, in memory only ──────────────────────

export const BUDGET = 2000;
const BASE_SPENT = 1240;
/** This month by category; the top three are Groceries, Transport, Eating out. */
const BASE_CATEGORIES: readonly [string, number][] = [
  ["Groceries", 486.2],
  ["Transport", 214.5],
  ["Eating out", 168.3],
  ["Bills & Utilities", 158],
  ["Entertainment", 121.5],
  ["Subscriptions", 91.5],
];
const CATEGORIES = [
  "Housing", "Bills & Utilities", "Groceries", "Eating out", "Transport", "Phone & Internet",
  "Subscriptions", "Health", "Clothing", "Learning", "Entertainment", "Other",
];
/** The newest entries: days before today, amount, category, note. */
const BASE_RECENT: readonly [number, number, string, string][] = [
  [0, 18.4, "Eating out", "Lunch"],
  [1, 64.25, "Groceries", "Weekly shop"],
  [2, 32, "Transport", "Train pass top-up"],
  [3, 11.99, "Subscriptions", "Music"],
  [5, 27.8, "Groceries", "Market"],
];

const round2 = (n: number) => Math.round(n * 100) / 100;

/**
 * The Finance pill's folder, as the demo plays it: the summary finance.rs
 * would send for a made-up month. An expense added from the card during the
 * demo lands here, in memory, and is gone when the demo ends.
 */
export class DemoFinance {
  private added: FinanceEntry[] = [];
  private version = 0;
  private readonly now: () => Date;

  constructor(now: () => Date = () => new Date()) {
    this.now = now;
  }

  /** Back to the made-up month as it starts. */
  reset() {
    this.added = [];
    this.version++;
  }

  summary(): FinanceSummary {
    const now = this.now();
    const today = localDate(now);
    const month = today.slice(0, 7);
    const day = (back: number) => {
      const d = new Date(now.getFullYear(), now.getMonth(), Math.max(1, now.getDate() - back));
      return localDate(d);
    };
    const base: FinanceEntry[] = BASE_RECENT.map(([back, amount, category, note], i) => ({
      id: `demo-${i}`, date: day(back), amount, currency: "USD", category, note,
      createdAt: `${day(back)}T12:00:00`,
    }));
    const totals = new Map(BASE_CATEGORIES);
    let spent = BASE_SPENT;
    let spentToday = BASE_RECENT.filter(([back]) => day(back) === today).reduce((sum, [, a]) => sum + a, 0);
    let monthCount = 23;
    for (const e of this.added) {
      if (!e.date.startsWith(month)) continue;
      spent += e.amount;
      monthCount++;
      totals.set(e.category, (totals.get(e.category) ?? 0) + e.amount);
      if (e.date === today) spentToday += e.amount;
    }
    spent = round2(spent);
    const recent = [...this.added, ...base]
      .sort((a, b) => b.date.localeCompare(a.date) || b.createdAt.localeCompare(a.createdAt))
      .slice(0, 10);
    return {
      baseCurrency: "USD",
      month,
      today,
      spentMonth: spent,
      spentToday: round2(spentToday),
      budget: BUDGET,
      remaining: round2(BUDGET - spent),
      percentUsed: (spent / BUDGET) * 100,
      topCategories: [...totals]
        .sort((a, b) => b[1] - a[1])
        .map(([category, amount]) => ({ category, amount: round2(amount) })),
      recent,
      entryCount: 87 + this.added.length,
      monthCount,
      skippedRows: 0,
      unconverted: [],
      currencies: ["USD"],
      categories: [...CATEGORIES],
    };
  }

  /** What finance_poll answers: the summary when `stamp` is not the current one. */
  poll(stamp: unknown): FinancePoll | null {
    const current = `demo-${this.version}`;
    if (stamp === current) return null;
    return { stamp: current, folder: "", summary: this.summary(), error: null };
  }

  /** What finance_add_expense answers: the expense kept in memory, and the fresh summary. */
  add(expense: NewExpense): FinancePoll {
    const now = this.now();
    this.added.push({
      id: `demo-added-${this.added.length}`,
      date: expense.date,
      amount: expense.amount,
      currency: "USD",
      category: expense.category,
      note: expense.note,
      createdAt: now.toISOString(),
    });
    this.version++;
    return this.poll(null)!;
  }
}

// ── What the integrations start with ─────────────────────────────────────────

/** The pills' connection state and GitHub's sample data at the start of the loop. */
export function demoIntegrations(now: Date): Record<string, IntegrationInfo> {
  const none = { data: {}, error: null, loaded: false, configured: true };
  return {
    [CLAUDE_ID]: { ...none },
    [CODEX_ID]: { ...none },
    [GITHUB_ID]: { data: githubData(false, now), error: null, loaded: true, configured: true },
  };
}
