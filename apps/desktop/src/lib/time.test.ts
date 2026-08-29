import { describe, expect, test } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { compactDuration, readableTime } from "./time";

/**
 * The same table the Rust side reads, at `tests/shared/duration-cases.json`.
 *
 * Read from disk rather than imported, so that a missing or renamed file is a
 * loud failure here instead of a silent zero-case pass.
 */
const casesPath = fileURLToPath(new URL("../../../../tests/shared/duration-cases.json", import.meta.url));
const table = JSON.parse(readFileSync(casesPath, "utf8")) as {
  cases: { seconds: number; expect: string }[];
};

describe("compactDuration", () => {
  test("agrees with the terminal on every shared case", () => {
    // If this fails, the window and the terminal have started describing the
    // same running process two different ways. Whichever one moved is the one
    // that is wrong; the table is the agreement.
    expect(table.cases.length).toBeGreaterThan(8);
    for (const { seconds, expect: wanted } of table.cases) {
      expect(compactDuration(seconds), `${seconds} seconds`).toBe(wanted);
    }
  });

  test("the table it checks against is actually loaded", () => {
    // Without this, a broken path would make the test above pass by iterating
    // over nothing -- the worst kind of green.
    expect(Array.isArray(table.cases)).toBe(true);
    expect(table.cases[0]).toHaveProperty("seconds");
    expect(table.cases[0]).toHaveProperty("expect");
  });

  test("stays scannable in a column", () => {
    // The whole reason this is not readableDuration: it sits beside two
    // hundred rows and has to stay narrow and aligned.
    for (const { seconds } of table.cases) {
      const rendered = compactDuration(seconds);
      expect(rendered.length).toBeLessThanOrEqual(8);
      expect(rendered).toMatch(/^\d+[dhm]( \d+[hm])?$/);
    }
  });
});

describe("readableTime", () => {
  test("renders an instant the reader can place", () => {
    const rendered = readableTime("2026-08-28T21:04:33.1234567Z");
    // The exact string depends on the machine's locale and time zone, which is
    // the point of using the platform's formatter. What must be true anywhere
    // is that it is no longer the RFC 3339 string it arrived as.
    expect(rendered).not.toBe("2026-08-28T21:04:33.1234567Z");
    expect(rendered).toContain("2026");
    expect(rendered).not.toContain("T");
  });

  test("hands back anything it cannot read, unchanged", () => {
    // An odd-looking timestamp is still evidence. "unknown" is not, and
    // somebody working out what happened on their computer this afternoon
    // deserves the odd-looking one.
    for (const rubbish of ["not a date", "", "yesterday", "0000", "2026", "28/08/2026"]) {
      expect(readableTime(rubbish)).toBe(rubbish);
    }
  });

  test("does not let Date guess at something that is not a date", () => {
    // `new Date("0000")` succeeds. It reads the year zero and renders it as a
    // real date in the year 2, so a value the tool could not read would have
    // appeared on screen as a confident wrong answer about when something
    // happened. Found by this test, which is why the shape is checked before
    // the value is parsed.
    expect(readableTime("0000")).toBe("0000");
    expect(readableTime("0000")).not.toContain("Dec");
  });

  test("accepts the shapes the back end actually sends", () => {
    // `OffsetDateTime` serialises as RFC 3339, to seven decimal places, with
    // an offset that is usually but not always `Z`. All of these must render.
    for (const real of [
      "2026-08-28T21:04:33.1234567Z",
      "2026-08-28T21:04:33Z",
      "2026-08-28T21:04:33+01:00",
      "2026-08-28T21:04:33-05:00",
    ]) {
      const rendered = readableTime(real);
      expect(rendered, real).not.toBe(real);
      expect(rendered, real).toContain("2026");
    }
  });
});
