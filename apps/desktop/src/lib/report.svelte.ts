/**
 * The bug report being written, kept somewhere a screen cannot take with it.
 *
 * The same reasoning as `scan.svelte.ts`, and for a worse loss. Screens are
 * destroyed when you switch away from them, and this one contains the only
 * thing in the whole application a person types by hand: their description of
 * what they were doing when it broke. Written into the component, that
 * paragraph disappeared the moment they clicked another tab to go and check a
 * detail they wanted to include -- which is exactly why somebody would click
 * another tab while writing one.
 *
 * So the draft lives here, and the screen shows it.
 */

import { api, type Incident, type ProblemReport } from "./api";

export const state = $state({
  /** What the tool generated, kept so edits can be measured against it. */
  generated: null as ProblemReport | null,
  incidents: [] as Incident[],
  error: null as string | null,
  notice: null as string | null,
  loaded: false,
  /** What will actually be posted. Seeded from `generated`, then theirs. */
  title: "",
  body: "",
});

export function edited(): boolean {
  return (
    state.generated !== null &&
    (state.title !== state.generated.title || state.body !== state.generated.body)
  );
}

/**
 * Fetch the report the tool would generate.
 *
 * Edits are never overwritten by this, not even when somebody presses Refresh.
 * Refresh asks for a fresh reading of what has been recorded; it is not a
 * request to throw away what they have written, and there is a separate,
 * clearly labelled button for that.
 */
export async function load() {
  const keep = edited();
  try {
    const [built, recorded] = await Promise.all([
      api.reportBuild(),
      api.reportIncidents(40),
    ]);
    state.generated = built;
    state.incidents = recorded;
    if (!keep) {
      state.title = built.title;
      state.body = built.body;
    } else {
      state.notice =
        "Re-read what has been recorded. Your edits were kept — use “Undo my edits” to take the freshly generated text instead.";
    }
    state.error = null;
  } catch (problem) {
    state.error = String(problem);
  } finally {
    state.loaded = true;
  }
}

/** Load once, the first time somebody opens the screen. */
export async function loadIfNeeded() {
  if (state.loaded) return;
  await load();
}

export function restore() {
  if (!state.generated) return;
  state.title = state.generated.title;
  state.body = state.generated.body;
  state.notice = null;
}

export async function openIssue() {
  state.notice = null;
  try {
    await api.reportOpenIssue(state.title, state.body);
    state.notice =
      "The issue form is open in your browser. Nothing is sent until you press the button there.";
  } catch (problem) {
    // Usually "too long for a link". Saving is the way through, so say so
    // rather than leaving a dead end.
    state.error = String(problem);
  }
}

export async function save() {
  state.notice = null;
  try {
    const path = await api.reportSave(state.body);
    state.notice = `Saved to ${path}. Attach that file to the issue.`;
  } catch (problem) {
    state.error = String(problem);
  }
}

export async function openForm() {
  state.notice = null;
  try {
    await api.reportOpenForm();
    state.notice = "The issue form is open in your browser.";
  } catch (problem) {
    state.error = String(problem);
  }
}

export async function clear() {
  state.notice = null;
  try {
    await api.reportClear();
    // Deliberately not `load()`: what was recorded is gone, so the generated
    // text must be regenerated from nothing rather than kept alongside edits
    // that describe incidents which no longer exist.
    state.loaded = false;
    state.title = "";
    state.body = "";
    state.generated = null;
    await load();
    state.notice = "Cleared. Nothing recorded is kept.";
  } catch (problem) {
    state.error = String(problem);
  }
}
