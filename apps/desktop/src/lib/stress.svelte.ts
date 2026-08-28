/**
 * The stress test, kept somewhere a screen cannot take with it.
 *
 * Same reasoning as the watcher next door. Screens here are created when you
 * switch to them and destroyed when you switch away, so a subscription that
 * belonged to a screen would stop existing the moment somebody clicked another
 * tab -- and this is a thing people start and then go and look at something
 * else for an hour.
 *
 * A run that lost its progress and its result because somebody checked the
 * Scan tab would be worse than no run at all: they would have heated their
 * machine for an hour and have nothing to show for it.
 */

import {
  api,
  onStressEvent,
  type StressReport,
  type StressStatus,
  type UnlistenFn,
} from "./api";

export const stress = $state({
  /** What to work. Both by default, because faults hide in either. */
  cpu: true,
  memory: true,
  minutes: 10,
  memoryShare: 0.6,

  running: false,
  /** Set once the run is under way, so the screen can show what it is doing. */
  elapsed: 0,
  total: 0,
  blocks: 0,
  memoryPatterns: 0,
  hottest: null as { label: string; peak_c: number } | null,
  /** False means nothing is watching the temperature. Never that it is cool. */
  watchingHeat: true,
  /** Faults as they arrive, not held back until the end. */
  faults: [] as { kind: string; part: string; detail: string }[],

  status: null as StressStatus | null,
  report: null as StressReport | null,
  error: null as string | null,
});

let unlisten: UnlistenFn | null = null;

/** Start hearing from a running test. Safe to call repeatedly. */
export async function listenForProgress() {
  if (unlisten) return;
  unlisten = await onStressEvent((event) => {
    switch (event.event) {
      case "started":
        stress.running = true;
        stress.error = null;
        stress.faults = [];
        stress.report = null;
        stress.elapsed = 0;
        stress.total = event.seconds;
        stress.blocks = 0;
        stress.memoryPatterns = 0;
        stress.hottest = null;
        stress.watchingHeat = event.watching_heat;
        break;
      case "progress":
        stress.elapsed = event.elapsed_secs;
        stress.total = event.total_secs;
        stress.blocks = event.blocks;
        stress.memoryPatterns = event.memory_patterns;
        stress.hottest = event.hottest;
        break;
      case "fault":
        // Shown the moment it happens. Somebody watching their machine be
        // worked hard should not have to wait for the end to be told it has
        // started getting arithmetic wrong.
        stress.faults = [...stress.faults, event.fault];
        break;
      case "finished":
        stress.running = false;
        stress.report = event.report;
        void refresh();
        break;
    }
  });
}

/** Ask what a run would do to this machine, and what the last one found. */
export async function refresh() {
  try {
    const status = await api.stressStatus(stress.memoryShare);
    stress.status = status;
    stress.running = status.running;
    // The back end's copy is the authority: it kept the result while this
    // screen did not exist.
    if (status.last && !stress.report) {
      stress.report = status.last;
    }
    stress.error = null;
  } catch (problem) {
    stress.error = String(problem);
  }
}

export async function start() {
  if (stress.running) return;
  stress.error = null;
  await listenForProgress();
  try {
    await api.stressStart(stress.cpu, stress.memory, stress.minutes, stress.memoryShare);
    stress.running = true;
  } catch (problem) {
    stress.error = String(problem);
    stress.running = false;
  }
}

export async function stop() {
  try {
    await api.stressStop();
  } catch (problem) {
    stress.error = String(problem);
  }
}
