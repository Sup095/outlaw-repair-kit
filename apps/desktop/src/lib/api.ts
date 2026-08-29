// Every call the window can make. Each one is a command in the Rust backend,
// which is itself a call into the shared crates -- so nothing here is a
// capability the command line does not also have.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type { UnlistenFn };

export type CheckState = "pass" | "warn" | "fail";

export interface CheckResult {
  name: string;
  state: CheckState;
  detail: string;
}

export interface BootEvent {
  kind: "started" | "check" | "update" | "finished";
  version?: string;
  step?: number;
  total_steps?: number;
  result?: CheckResult;
  status?: UpdateStatus;
  ready?: boolean;
  line?: string;
}

export type UpdateStatus =
  | { state: "up_to_date"; version: string }
  | { state: "available"; current: string; latest: string; url: string }
  | { state: "unknown"; reason: string };

export interface BootReport {
  version: string;
  selftest: { checks: CheckResult[] };
  update: UpdateStatus;
}

export type Severity = "critical" | "high" | "medium" | "low" | "info";

/** What would happen to one running process. Mirrors `ork_core::processes::Standing`. */
export type Standing =
  | { standing: "protected"; because: string }
  | { standing: "held-back"; because: string }
  | { standing: "candidate" };

export interface ProcessRow {
  pid: number;
  name: string;
  /** What it holds now -- never what stopping it would give back. */
  memory_bytes: number;
  run_time_secs: number;
  standing: Standing;
}

/**
 * What a sweep would do to a whole program. Mirrors `ork_core::processes::Sweep`.
 *
 * `part-of-it` is the one that matters. A program with some processes offered
 * and some held back keeps running after a sweep, with fewer processes than it
 * had -- and somebody who was not told that reads the still-open window as the
 * tool having failed.
 */
export type Sweep =
  | { how: "all-of-it" }
  | { how: "part-of-it"; offered: number; remaining: number }
  | { how: "none-of-it" };

/** Several processes of one name, which is one program to whoever is looking. */
export interface ProcessProgram {
  name: string;
  pids: number[];
  processes: number;
  /** What they hold between them -- never what stopping them would give back. */
  memory_held: number;
  /** How long the longest-running of them has been up. */
  run_time_secs: number;
  offered: number;
  held_back: number;
  protected: number;
  /** Whether this program is on the leave-alone list.
   *
   * Read from what the classifier decided rather than from the settings file,
   * so the control shows what the tool will actually do. The two agree by
   * construction, which is the point: a checkbox drawn from the file while the
   * list was drawn from the classifier could show a program as left alone and
   * offer it in the same breath. */
  pinned: boolean;
  sweep: Sweep;
  /** The same answer as a sentence, written once, in the back end. */
  sweep_says: string;
  /** And short enough for a column. Written once for the same reason: the
   * terminal prints this exact string in its own list. */
  sweep_briefly: string;
}

export interface ProcessSurvey {
  platform: string;
  running: number;
  memory_held_by_candidates: number;
  why_protected: { reason: string; count: number }[];
  why_held_back: { reason: string; count: number }[];
  /**
   * Why the "what is in front of you" rule could not be applied, or null if it
   * was. Null and a string mean genuinely different things and the screen has
   * to show the difference: everywhere else an unanswered question makes the
   * tool more careful, and here it makes it less.
   */
  in_front_unchecked: string | null;
  /** The same rows grouped by program. Both are published: they answer
   * different questions, and the per-process list is the one to check when a
   * number looks wrong. */
  programs: ProcessProgram[];
  rows: ProcessRow[];
}

export interface Finding {
  id: string;
  probe: string;
  subject: string | null;
  title: string;
  detail: string;
  severity: Severity;
  category: string;
  triage: string;
  evidence: { label: string; value: string }[];
  remediation_hint?: string | null;
  observed_at?: string;
}

export interface ProbeOutcome {
  probe: string;
  name: string;
  status: { status: string; reason?: string; error?: string };
  /** Why it did not run, as a sentence. Present only when it did not run. */
  skipped_because?: string;
  findings: Finding[];
}

export interface ScanReport {
  tier: string;
  outcomes: ProbeOutcome[];
  cancelled: boolean;
  started_at?: string;
}

export interface ScanEvent {
  event: string;
  probe?: string;
  name?: string;
  index?: number;
  total?: number;
  outcome?: ProbeOutcome;
  finding_count?: number;
  cancelled?: boolean;
  tier?: string;
  probe_count?: number;
}

export interface Discovered {
  machine_id: string;
  name: string;
  platform: string;
  version: string;
  pairing_open: boolean;
  address: string;
}

export const api = {
  boot: () => invoke<BootReport>("boot"),
  hostInfo: () => invoke<Record<string, unknown>>("host_info"),
  probes: () => invoke<CheckCatalogue>("probe_list"),
  startScan: (tier: string) => invoke<ScanReport>("start_scan", { tier }),
  cancelScan: () => invoke<boolean>("cancel_scan"),
  explain: (report: ScanReport) =>
    invoke<{ routing: string; analysis: Record<string, unknown> }>("explain_report", { report }),
  settingsLoad: () => invoke<{ path: string; exists: boolean; config: any }>("settings_load"),
  settingsSave: (config: unknown) => invoke<string>("settings_save", { config }),
  secretStatus: () => invoke<{ cloud: boolean; remote: boolean }>("secret_status"),
  secretSet: (which: string, value: string) => invoke<void>("secret_set", { which, value }),
  secretClear: (which: string) => invoke<void>("secret_clear", { which }),
  routing: () => invoke<any>("routing_status"),
  queue: () => invoke<QueueItem[]>("queue_list"),
  // Returns once the run has finished. `true` means it was stopped early.
  fixRun: (apply: boolean) => invoke<boolean>("fix_run", { apply }),
  fixAnswer: (id: number, answer: FixAnswer) => invoke<boolean>("fix_answer", { id, answer }),
  fixCancel: () => invoke<boolean>("fix_cancel"),
  linkStatus: () => invoke<any>("link_status"),
  linkHostStart: (port?: number, modelUrl?: string) =>
    invoke<string>("link_host_start", { port, modelUrl }),
  linkHostStop: () => invoke<boolean>("link_host_stop"),
  linkPairReopen: () => invoke<string>("link_pair_reopen"),
  linkFind: (port?: number) => invoke<Discovered[]>("link_find", { port }),
  linkJoin: (code: string, address: string) => invoke<any>("link_join", { code, address }),
  linkRemove: (name: string) => invoke<number>("link_remove", { name }),
  linkView: (name?: string) => invoke<any>("link_view", { name }),
  linkCheck: (name: string) => invoke<any>("link_check", { name }),
  reportBuild: () => invoke<ProblemReport>("report_build"),
  reportIncidents: (limit: number) => invoke<Incident[]>("report_incidents", { limit }),
  // Sends what the window is showing, not what the backend generated, so an
  // edit made here is what gets carried into the form.
  reportOpenIssue: (title: string, body: string) =>
    invoke<string>("report_open_issue", { title, body }),
  reportOpenForm: () => invoke<string>("report_open_form"),
  reportSave: (body: string) => invoke<string>("report_save", { body }),
  reportClear: () => invoke<void>("report_clear"),
  audit: (limit: number) => invoke<{ at: string; readable: string; kind: string; message: string }[]>("audit_list", { limit }),
  // What is running and what a sweep would do to each. Read-only: there is no
  // command that stops anything, in either front-end, by design.
  processSurvey: () => invoke<ProcessSurvey>("process_survey"),
  // Pinning is by name, not by process id: a browser is forty processes and
  // pinning one of them would leave the rest offered, which is not what
  // anybody means by "leave this alone". Returns whether anything changed.
  processPin: (name: string, pinned: boolean) =>
    invoke<boolean>("process_pin", { name, pinned }),
  // The manual is compiled into the program, not fetched. A machine that has
  // gone wrong is often a machine that cannot reach the internet, and the
  // pages most likely to be needed are the ones least likely to be reachable
  // when they are needed.
  // The watcher. It keeps running while the window is on another screen, and
  // while it is on no screen at all, so what it noticed is asked for rather
  // than only listened for.
  watchStatus: () => invoke<WatchStatus>("watch_status"),
  watchStart: (tier: string, everyMinutes: number) =>
    invoke<void>("watch_start", { tier, everyMinutes }),
  watchStop: () => invoke<void>("watch_stop"),
  watchForget: () => invoke<void>("watch_forget"),

  // The stress test. `stressStatus` takes the memory share because the whole
  // point of asking is to show the real number -- how much of this machine's
  // memory would actually be tested -- before anybody presses the button,
  // rather than after.
  stressStatus: (memoryShare: number) =>
    invoke<StressStatus>("stress_status", { memoryShare }),
  stressStart: (cpu: boolean, memoryTest: boolean, minutes: number, memoryShare: number) =>
    invoke<void>("stress_start", { cpu, memoryTest, minutes, memoryShare }),
  stressStop: () => invoke<void>("stress_stop"),

  manualContents: () => invoke<ManualEntry[]>("manual_contents"),
  manualPage: (id: string) => invoke<ManualPage>("manual_page", { id }),
  manualLicence: () => invoke<string>("manual_licence"),
};

export interface ManualEntry {
  id: string;
  title: string;
  summary: string;
}

export interface ManualPage extends ManualEntry {
  /** Rendered from Markdown in the back end. See src-tauri/src/manual.rs. */
  html: string;
}

/// One check this build knows how to run.
export interface CheckInfo {
  id: string;
  name: string;
  description: string;
  category: string;
  tier: "quick" | "full" | "deep";
  platforms: string[];
  requires_elevation: boolean;
  required_tools: string[];
  /// Every problem this check is able to report.
  reports: string[];
  /// Whether it can run on this machine, decided by the same rule the scanner
  /// uses rather than by a second copy of it here.
  available: boolean;
  unavailable_reason: string | null;
}

export interface CheckCatalogue {
  platform: string;
  elevated: boolean;
  checks: CheckInfo[];
}

/// One thing that went wrong, as it was recorded at the time.
export interface Incident {
  at: string;
  kind: "error" | "panic";
  source: string;
  message: string;
  location?: string | null;
  backtrace?: string | null;
}

/// A finished bug report. `body` is already redacted and is exactly what would
/// be posted.
export interface ProblemReport {
  title: string;
  body: string;
  incident_count: number;
  includes_crash: boolean;
  /// Absent when the report is too long to carry in a link.
  issue_url: string | null;
  issue_form_url: string;
}

export function onBootEvent(handler: (event: BootEvent) => void): Promise<UnlistenFn> {
  return listen<BootEvent>("boot://event", (message) => handler(message.payload));
}

export type LinkEvent =
  | { event: "linked"; name: string }
  | { event: "wrong-code"; attempts_left: number }
  | { event: "model-requested"; name: string };

export function onLinkEvent(handler: (event: LinkEvent) => void): Promise<UnlistenFn> {
  return listen<LinkEvent>("link://event", (message) => handler(message.payload));
}

export type ItemOutcome =
  | { outcome: "resolved"; action: string }
  | { outcome: "exhausted"; tried: number }
  | { outcome: "needs-a-person"; instructions: string[] }
  | { outcome: "stopped" }
  | { outcome: "no-candidates" };

export type FixEvent =
  | {
      event: "started";
      total: number;
      testable: number;
      apply: boolean;
      snapshot_warning: string | null;
    }
  | {
      event: "item";
      index: number;
      total: number;
      occurrence_key: string;
      title: string;
      severity: Severity;
    }
  | { event: "outcome"; occurrence_key: string; outcome: ItemOutcome }
  | { event: "finished"; resolved: number; stopped: boolean };

/// A change waiting on permission. Nothing happens until this is answered.
/// One problem on the triage queue.
export interface QueueItem {
  id: number;
  occurrence_key: string;
  finding_id: string;
  subject: string | null;
  severity: Severity;
  title: string;
  finding: Finding;
  state: "pending" | "resolved" | "exhausted" | "dismissed";
  attempts: number;
  /// When this problem was first put on the queue, RFC 3339 in UTC.
  first_seen: string;
  /// When a scan last actually observed it. The same as `first_seen` until a
  /// second scan finds it again -- which is the difference between a problem
  /// the machine still has and one it had a fortnight ago.
  last_seen: string;
  /// The two above as one sentence, built by the backend so that the window
  /// and the command line cannot word it differently.
  seen: string;
}

export interface FixAsk {
  id: number;
  action: string;
  title: string;
  occurrence_key: string;
}

// Anything the backend does not recognise is a refusal, so there is no answer
// string that accidentally means yes.
export type FixAnswer = "approve" | "decline" | "stop";

export function onFixEvent(handler: (event: FixEvent) => void): Promise<UnlistenFn> {
  return listen<FixEvent>("fix://event", (message) => handler(message.payload));
}

export function onFixAsk(handler: (ask: FixAsk) => void): Promise<UnlistenFn> {
  return listen<FixAsk>("fix://ask", (message) => handler(message.payload));
}

export function onWatchEvent(handler: (event: WatchEvent) => void): Promise<UnlistenFn> {
  return listen<WatchEvent>("watch://event", (message) => handler(message.payload));
}

export function onStressEvent(handler: (event: StressEvent) => void): Promise<UnlistenFn> {
  return listen<StressEvent>("stress://event", (message) => handler(message.payload));
}

export function onScanEvent(handler: (event: ScanEvent) => void): Promise<UnlistenFn> {
  return listen<ScanEvent>("scan://event", (message) => handler(message.payload));
}

/** One thing the watcher noticed changing. */
export type Change =
  | { change: "appeared"; finding: Finding }
  | { change: "worsened"; finding: Finding; was: Severity }
  | { change: "eased"; finding: Finding; was: Severity }
  | { change: "cleared"; id: string; subject: string | null; title: string; was: Severity }
  | { change: "flapping"; finding: Finding; appearances: number };

/** What one look produced. Most looks produce nothing, which is the point. */
export interface Look {
  at: string;
  changes: Change[];
  established_baseline: boolean;
  recorded: number;
  /** Checks that could have run and did not, so a quiet round is never mistaken for a clean one. */
  did_not_run: string[];
}

export type WatchEvent =
  | { event: "started"; interval_secs: number; known: number }
  | { event: "looking" }
  | { event: "looked"; look: Look }
  | { event: "trouble"; error: string }
  | { event: "stopped" };

/** One problem the watcher has seen, present or not. */
export interface Seen {
  id: string;
  probe: string;
  subject: string | null;
  title: string;
  severity: Severity;
  present: boolean;
  first_seen: string;
  last_change: string;
  appearances: number;
}

/** Something being held quiet because it comes and goes, and why. */
export interface Muted {
  key: string;
  title: string;
  reason: string;
  appearances: number;
}

export interface Baseline {
  established: boolean;
  seen: Record<string, Seen>;
  muted: Muted[];
}

export interface WatchStatus {
  running: boolean;
  baseline: Baseline;
  baseline_path: string;
  history: Look[];
}

/** The hottest one part of the machine got during a stress test. */
export interface Heat {
  label: string;
  peak_c: number;
  critical_c: number | null;
}

/** Something the machine got wrong under load. Always a hardware fault. */
export interface StressFault {
  kind: string;
  part: string;
  detail: string;
}

export type StressEnding =
  | { ending: "completed" }
  | { ending: "cancelled" }
  | { ending: "too-hot"; sensor: string; reached_c: number; ceiling_c: number };

export type StressMemory =
  | { memory: "ran"; bytes: number; patterns: number; mismatches: unknown[] }
  | { memory: "not-run"; reason: string };

export interface StressReport {
  started_at: string;
  asked_for_secs: number;
  ran_for_secs: number;
  ending: StressEnding;
  cpu: { threads: number; blocks: number; wrong: number } | null;
  memory: StressMemory | null;
  heat: Heat[];
  /** False means nothing was watching the temperature -- never that it stayed cool. */
  watched_heat: boolean;
  faults: StressFault[];
}

export type StressEvent =
  | {
      event: "started";
      seconds: number;
      cpu_threads: number;
      memory_bytes: number;
      watching_heat: boolean;
    }
  | {
      event: "progress";
      elapsed_secs: number;
      total_secs: number;
      blocks: number;
      memory_patterns: number;
      hottest: Heat | null;
    }
  | { event: "fault"; fault: StressFault }
  | { event: "finished"; report: StressReport };

export interface StressStatus {
  running: boolean;
  last: StressReport | null;
  cores: number;
  /** What would actually be tested at the current share. Zero means it would not be. */
  memory_bytes: number;
  memory_available_bytes: number;
  memory_reserved_bytes: number;
}
