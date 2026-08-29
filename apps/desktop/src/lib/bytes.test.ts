import { describe, expect, test } from "vitest";
import { formatBytes, readableDuration } from "./bytes";

describe("formatBytes", () => {
  test("uses the largest unit that leaves a readable number", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1024)).toBe("1.0 KiB");
    expect(formatBytes(1536)).toBe("1.5 KiB");
    expect(formatBytes(1024 * 1024)).toBe("1.0 MiB");
    expect(formatBytes(3.4 * 1024 * 1024 * 1024)).toBe("3.4 GiB");
  });

  test("says binary units, because that is what it measured", () => {
    // The comment in bytes.ts records this going wrong once: the graphics card
    // reported "GB" for a number of gibibytes. A figure that disagrees with
    // the one in the system's own window is a figure nobody believes, so the
    // label has to match the arithmetic.
    expect(formatBytes(1024 ** 3)).toContain("GiB");
    expect(formatBytes(1024 ** 3)).not.toContain("GB");
    expect(formatBytes(1024 ** 2)).toContain("MiB");
  });

  test("never shows a fraction of a byte", () => {
    // A byte is not divisible and a reader knows it. "1.5 B" reads as a bug in
    // the tool rather than as a measurement.
    for (const bytes of [1, 7, 999, 1023]) {
      expect(formatBytes(bytes)).toMatch(/^\d+ B$/);
    }
  });

  test("drops the decimal once the number is big enough not to need it", () => {
    // One decimal below ten, none above: 9.4 GiB is worth the digit and
    // 512.3 MiB is noise.
    expect(formatBytes(9.4 * 1024 ** 3)).toBe("9.4 GiB");
    expect(formatBytes(512.3 * 1024 ** 2)).toBe("512 MiB");
  });

  test("refuses to invent a figure it was not given", () => {
    // A memory reading that failed arrives as NaN or as a negative, and
    // printing "0 B" for either would be the tool claiming a measurement it
    // does not have.
    expect(formatBytes(Number.NaN)).toBe("unknown");
    expect(formatBytes(-1)).toBe("unknown");
    expect(formatBytes(Number.POSITIVE_INFINITY)).toBe("unknown");
  });

  test("does not run off the end of its units", () => {
    // Absurd, and it must still produce a string rather than "undefined".
    const huge = formatBytes(1024 ** 8);
    expect(huge).toContain("PiB");
    expect(huge).not.toContain("undefined");
  });
});

describe("readableDuration", () => {
  test("changes unit where the number stops being easy to hold", () => {
    expect(readableDuration(5)).toBe("5 seconds");
    expect(readableDuration(89)).toBe("89 seconds");
    expect(readableDuration(90)).toBe("2 minutes");
    expect(readableDuration(60 * 89)).toBe("89 minutes");
    expect(readableDuration(60 * 90)).toBe("1.5 hours");
  });

  test("never leaves a bare number with no unit", () => {
    for (const seconds of [0, 1, 89, 90, 3599, 3600, 86_400]) {
      expect(readableDuration(seconds)).toMatch(/(seconds|minutes|hours)$/);
    }
  });
});
