/**
 * Sizes, for reading.
 *
 * The back end has `ork_core::util::format_bytes` and the terminal uses it.
 * This is the same rule written once for the window, rather than the same
 * expression written inline at each of the places that needs it -- which is
 * how the graphics card ended up reporting "GB" for a number of gibibytes.
 */

const UNITS = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];

/**
 * A byte count, in the largest unit that leaves a number a person can hold in
 * their head.
 *
 * Binary units, and labelled as such, because that is what an operating system
 * means when it says how much memory is free -- and a figure that disagrees
 * with the one in the system's own window is a figure nobody believes.
 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "unknown";
  let size = bytes;
  let unit = 0;
  while (size >= 1024 && unit < UNITS.length - 1) {
    size /= 1024;
    unit += 1;
  }
  // Whole bytes are never fractional, and a fraction of a byte is nonsense.
  const digits = unit === 0 ? 0 : size < 10 ? 1 : 0;
  return `${size.toFixed(digits)} ${UNITS[unit]}`;
}

/**
 * A number of seconds, the way somebody would say it out loud.
 */
export function readableDuration(seconds: number): string {
  if (seconds < 90) return `${Math.round(seconds)} seconds`;
  if (seconds < 60 * 90) return `${Math.round(seconds / 60)} minutes`;
  return `${(seconds / 3600).toFixed(1)} hours`;
}
