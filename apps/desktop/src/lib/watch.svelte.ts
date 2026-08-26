/**
 * The watcher, kept somewhere a screen cannot take with it.
 *
 * Same reasoning as the scan store next door, and more so. Screens in this
 * application are created when you switch to them and destroyed when you
 * switch away, so an event subscription that belongs to a screen stops
 * existing the moment somebody clicks another tab.
 *
 * For a watcher that is fatal in a particular way: the entire point of it is
 * to be running while you are doing something else. A watcher that only
 * notices things while you are looking at the watcher screen is not a watcher,
 * it is a scan button with extra steps.
 *
 * So the subscription and what it has heard live here, at module level. The
 * screen draws what this holds. The back end keeps its own copy besides, which
 * is what survives the window being closed and reopened -- this one only has
 * to survive a tab.
 */

import {
  api,
  onWatchEvent,
  type Look,
  type UnlistenFn,
  type WatchStatus,
} from "./api";

export const watch = $state({
  tier: "quick",
  everyMinutes: 15,
  running: false,
  /** Looks that carried a change, newest first. */
  history: [] as Look[],
  status: null as WatchStatus | null,
  error: null as string | null,
  /** The last time a look finished, whether or not it found anything. */
  lastLooked: null as string | null,
  /** Set once, when the starting point is recorded, so the screen can say so. */
  recorded: null as number | null,
});

let unlisten: UnlistenFn | null = null;

/**
 * Start hearing from the watcher.
 *
 * Safe to call repeatedly -- the screen calls it every time it is opened, and
 * a second subscription would mean every change arriving twice.
 */
export async function listenForChanges() {
  if (unlisten) return;
  unlisten = await onWatchEvent((event) => {
    switch (event.event) {
      case "started":
        watch.running = true;
        watch.error = null;
        break;
      case "looked":
        watch.lastLooked = event.look.at;
        if (event.look.established_baseline) {
          watch.recorded = event.look.recorded;
        } else if (event.look.changes.length > 0) {
          // Newest first, matching what the back end keeps.
          watch.history = [event.look, ...watch.history];
        }
        // What the watcher knows has just changed, and the panel showing it
        // was filled in before this look existed. Without this the screen
        // says "nothing watched yet" underneath its own report of the first
        // look, which reads as the tool disagreeing with itself.
        void refresh();
        break;
      case "trouble":
        // Not fatal, and deliberately not shown as the watcher having
        // stopped: one failed round is not a reason to stop looking at
        // somebody's computer, and the back end keeps going.
        watch.error = event.error;
        break;
      case "stopped":
        watch.running = false;
        break;
    }
  });
}

/** Ask the back end what it knows, which is what survives the window closing. */
export async function refresh() {
  try {
    const status = await api.watchStatus();
    watch.status = status;
    watch.running = status.running;
    // The back end's history is the authority: it kept running while this
    // window was closed, and the copy here did not.
    if (status.history.length >= watch.history.length) {
      watch.history = status.history;
    }
    watch.error = null;
  } catch (problem) {
    watch.error = String(problem);
  }
}

export async function start() {
  if (watch.running) return;
  watch.error = null;
  watch.recorded = null;
  await listenForChanges();
  try {
    await api.watchStart(watch.tier, watch.everyMinutes);
    watch.running = true;
  } catch (problem) {
    watch.error = String(problem);
    watch.running = false;
  }
  await refresh();
}

export async function stop() {
  try {
    await api.watchStop();
  } catch (problem) {
    watch.error = String(problem);
  }
  watch.running = false;
  await refresh();
}

/**
 * Forget everything and start over.
 *
 * The screen confirms before calling this. After it, the next look records a
 * fresh starting point and reports nothing -- so somebody who did this without
 * meaning to would see a watcher that has apparently stopped noticing things.
 */
export async function forget() {
  try {
    await api.watchForget();
    watch.history = [];
    watch.recorded = null;
    watch.lastLooked = null;
  } catch (problem) {
    watch.error = String(problem);
  }
  await refresh();
}
