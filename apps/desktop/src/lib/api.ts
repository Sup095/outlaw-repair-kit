// Every call the window can make. Each one is a command in the Rust backend,
// which is itself a call into the shared crates -- so nothing here is a
// capability the command line does not also have.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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
  status: { status: string; reason?: unknown; error?: string };
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
  probes: () => invoke<Record<string, unknown>[]>("probe_list"),
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
  queue: () => invoke<any[]>("queue_list"),
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
  audit: (limit: number) => invoke<{ at: string; kind: string; message: string }[]>("audit_list", { limit }),
};

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

export function onScanEvent(handler: (event: ScanEvent) => void): Promise<UnlistenFn> {
  return listen<ScanEvent>("scan://event", (message) => handler(message.payload));
}
