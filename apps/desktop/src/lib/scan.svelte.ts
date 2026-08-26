/**
 * The scan, kept somewhere a screen cannot take with it.
 *
 * The views in this application are created when you switch to them and
 * destroyed when you switch away. That is fine for a screen that only reads
 * something back -- it reads it again. It is not fine for a scan.
 *
 * When this lived inside the scan screen, looking at any other tab threw the
 * whole run away: the event subscription went with the component, and the
 * finished report arrived to an object nothing was watching any more. A deep
 * scan reads and hashes most of the operating system and can take an hour.
 * Losing that because somebody clicked "Checks" to look something up is not a
 * rough edge, it is the tool wasting an hour of a person's time and not saying
 * so.
 *
 * So the run lives here, at module level, outside every screen. The screen
 * shows what this holds and asks it to start; whether anyone is looking makes
 * no difference to it.
 */

import { api, onScanEvent, type ScanEvent, type ScanReport } from "./api";

export type Progress = { index: number; total: number; name: string };

export const scan = $state({
  tier: "quick",
  running: false,
  progress: null as Progress | null,
  report: null as ScanReport | null,
  error: null as string | null,
  explanation: null as any | null,
  explaining: false,
});

export async function runScan() {
  // One at a time. The back end refuses a second scan anyway; refusing here
  // as well means the screen never shows a state the back end will not honour.
  if (scan.running) return;

  scan.error = null;
  scan.explanation = null;
  scan.report = null;
  scan.running = true;
  scan.progress = null;

  // Every check reports as it finishes, so a long scan never looks stuck --
  // and because this subscription belongs to the module rather than to a
  // screen, it keeps arriving while you are looking at another tab.
  const unlisten = await onScanEvent((event: ScanEvent) => {
    if (event.event === "probe-started") {
      scan.progress = {
        index: event.index ?? 0,
        total: event.total ?? 0,
        name: event.name ?? "",
      };
    }
  });

  try {
    scan.report = await api.startScan(scan.tier);
  } catch (problem) {
    scan.error = String(problem);
  } finally {
    unlisten();
    scan.running = false;
    scan.progress = null;
  }
}

export async function explainFindings() {
  if (!scan.report || scan.explaining) return;
  scan.explaining = true;
  scan.error = null;
  try {
    scan.explanation = await api.explain(scan.report);
  } catch (problem) {
    scan.error = String(problem);
  } finally {
    scan.explaining = false;
  }
}

/** The user's manual stop. Nothing in this tool ends a scan for taking too long. */
export function cancelScan() {
  return api.cancelScan();
}
