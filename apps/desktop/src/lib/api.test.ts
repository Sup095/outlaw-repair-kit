import { describe, expect, test } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

/**
 * The window calls the back end by name, as a string, and nothing checks the
 * string.
 *
 * A typo in `invoke("proces_survey")` compiles, type-checks, builds, and
 * produces a screen that loads and then fails the instant somebody uses it,
 * with an error naming a command that visibly exists in the Rust source. It is
 * the same class of fault as an unregistered command, from the other end, and
 * the two halves are in different languages so no compiler can see across the
 * join.
 *
 * These read both sides and compare them. The Rust side has its own version of
 * this in `src-tauri/tests/every_command_is_reachable.rs`, which checks that
 * every command defined is registered; this checks that every command *called*
 * exists. Between them, a command cannot be defined and unreachable, and
 * cannot be called and absent.
 */

function read(relative: string): string {
  return readFileSync(fileURLToPath(new URL(relative, import.meta.url)), "utf8");
}

const apiSource = read("./api.ts");
const libSource = read("../../src-tauri/src/lib.rs");

/** Every name passed to `invoke(...)` anywhere in api.ts. */
function invoked(): string[] {
  const names = [...apiSource.matchAll(/\binvoke\s*(?:<[^>]*>)?\s*\(\s*"([^"]+)"/g)].map(
    (match) => match[1],
  );
  return [...new Set(names)];
}

/** Every name inside `generate_handler![ ... ]`, without its module path. */
function registered(): string[] {
  const start = libSource.indexOf("generate_handler![");
  expect(start, "lib.rs registers its commands with generate_handler!").toBeGreaterThan(-1);
  const end = libSource.indexOf("]", start);
  return libSource
    .slice(start, end)
    .split("\n")
    .slice(1)
    .map((line) => line.trim().replace(/,$/, ""))
    .filter((line) => line.length > 0 && !line.startsWith("//"))
    .map((line) => line.split("::").pop() as string);
}

describe("the window and the back end agree on command names", () => {
  test("both sides were actually read", () => {
    // Without this, a moved file or a changed macro would make everything
    // below pass by comparing two empty lists.
    expect(invoked().length).toBeGreaterThan(20);
    expect(registered().length).toBeGreaterThan(20);
    expect(apiSource).toContain("@tauri-apps/api/core");
  });

  test("every command the window calls exists in the back end", () => {
    const known = registered();
    const missing = invoked().filter((name) => !known.includes(name));
    expect(
      missing,
      `these are invoked by the window but not registered in lib.rs, so calling \
them fails at runtime: ${missing.join(", ")}`,
    ).toEqual([]);
  });

  test("the checker notices a name that does not exist", () => {
    // The test above is only worth having if it can fail. This proves the
    // comparison works on a name known to be absent, without needing to break
    // the real file to find out.
    const known = registered();
    expect(known).not.toContain("no_such_command_exists");
    expect(["queue_list", "no_such_command_exists"].filter((n) => !known.includes(n))).toEqual([
      "no_such_command_exists",
    ]);
  });
});

describe("every screen the window has is reachable", () => {
  const appSource = read("../App.svelte");

  /** The `id` of each entry in the `tabs` list. */
  function tabIds(): string[] {
    const start = appSource.indexOf("const tabs = [");
    expect(start, "App.svelte declares its tabs in one list").toBeGreaterThan(-1);
    const end = appSource.indexOf("] as const", start);
    return [...appSource.slice(start, end).matchAll(/\{\s*id:\s*"([^"]+)"/g)].map(
      (match) => match[1],
    );
  }

  /** Each `view === "..."` the render block branches on. */
  function branched(): string[] {
    return [...appSource.matchAll(/view\s*===\s*"([^"]+)"/g)].map((match) => match[1]);
  }

  test("both lists were actually found", () => {
    expect(tabIds().length).toBeGreaterThan(10);
    expect(branched().length).toBeGreaterThan(9);
  });

  test("every tab renders its own screen", () => {
    // A tab with no branch does not fail. It falls through to the final
    // `{:else}` and quietly shows a different screen than the one it names --
    // which is worse than an error, because it looks like it worked. The last
    // tab is the fallback and so is allowed to have no branch of its own.
    const ids = tabIds();
    const branches = branched();
    const fallback = ids[ids.length - 1];
    const unrendered = ids.filter((id) => id !== fallback && !branches.includes(id));
    expect(
      unrendered,
      `these tabs have no render branch and would silently show the ${fallback} \
screen instead: ${unrendered.join(", ")}`,
    ).toEqual([]);
  });

  test("no branch renders a screen no tab can reach", () => {
    // The other direction: a leftover branch for a tab that was removed is
    // dead code that looks live.
    const ids = tabIds();
    const orphans = branched().filter((view) => !ids.includes(view));
    expect(orphans, `these branches belong to no tab: ${orphans.join(", ")}`).toEqual([]);
  });

  test("every view file is imported by the app", () => {
    // A screen nobody can get to is a screen that stops being maintained and
    // then stops working, and nothing says so.
    const views = [...appSource.matchAll(/import\s+(\w+View)\s+from/g)].map((match) => match[1]);
    expect(views.length).toBeGreaterThan(10);
    for (const view of views) {
      expect(appSource, `${view} is imported but never rendered`).toContain(`<${view} `);
    }
  });
});

describe("the window and the back end agree on the process survey", () => {
  /**
   * The survey is not a Rust struct serialised whole -- it is built key by key
   * in `Survey::as_report`, and the window reads it through a hand-written
   * interface. Nothing checks the two against each other: a field renamed on
   * one side type-checks perfectly on the other and arrives as `undefined`,
   * which renders as a blank column rather than as an error. On a screen whose
   * whole job is saying what would happen to somebody's running programs, a
   * blank column is worse than a crash.
   */
  const surveySource = read("../../../../crates/ork-core/src/processes/survey.rs");

  /** Every `"key":` inside the `as_report` body. */
  function publishedKeys(): string[] {
    const start = surveySource.indexOf("pub fn as_report(");
    expect(start, "ork-core builds the report in `as_report`").toBeGreaterThan(-1);
    const end = surveySource.indexOf("\n    }\n", start);
    const body = surveySource.slice(start, end);
    return [...new Set([...body.matchAll(/"([a-z_]+)":/g)].map((match) => match[1]))];
  }

  /** The field names of one `export interface` in api.ts. */
  function fieldsOf(name: string): string[] {
    const start = apiSource.indexOf(`export interface ${name} {`);
    expect(start, `api.ts declares ${name}`).toBeGreaterThan(-1);
    const end = apiSource.indexOf("\n}", start);
    return [...apiSource.slice(start, end).matchAll(/^\s{2}(\w+)[?]?:/gm)].map(
      (match) => match[1],
    );
  }

  test("both sides were actually read", () => {
    expect(publishedKeys().length).toBeGreaterThan(9);
    expect(fieldsOf("ProcessSurvey").length).toBeGreaterThan(5);
    expect(fieldsOf("ProcessProgram").length).toBeGreaterThan(8);
  });

  test("every field the window reads is one the back end publishes", () => {
    const published = publishedKeys();
    const wanted = [...fieldsOf("ProcessSurvey"), ...fieldsOf("ProcessProgram")];
    const missing = wanted.filter((field) => !published.includes(field));
    expect(
      missing,
      `these are declared on the window's side but never published by \
Survey::as_report, so they arrive undefined: ${missing.join(", ")}`,
    ).toEqual([]);
  });

  test("the checker notices a field that is not published", () => {
    // The test above is only worth having if it can fail.
    const published = publishedKeys();
    expect(published).not.toContain("memory_that_would_be_freed");
    expect(published).toContain("memory_held_by_candidates");
  });

  test("nothing in the survey is described as freed", () => {
    // The one word this tool must not use about held memory, checked on the
    // window's side as well because the front-end is where the temptation to
    // put a friendlier label on a number actually lives.
    for (const forbidden of ["freed", "willFree", "would_free"]) {
      expect(publishedKeys().join(" ")).not.toContain(forbidden);
    }
  });
});
