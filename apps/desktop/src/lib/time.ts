/**
 * Timestamps, for reading rather than for storing.
 *
 * Instants cross from the back end as RFC 3339 in UTC, to seven decimal
 * places. That is the right thing to store and the wrong thing to put in front
 * of somebody trying to work out what happened on their computer this
 * afternoon.
 *
 * The command line does this in Rust, in `ork_core::util::readable_time`,
 * because a terminal has nothing else. A window does: the browser engine knows
 * the reader's locale and time zone, and will render an instant the way their
 * operating system renders one. So this is not a second copy of that logic --
 * it is the platform's own formatting, which is what a window should use and
 * what the terminal cannot reach.
 *
 * Where the two are obliged to agree is the audit log, and they do, because
 * there the back end sends the rendered string rather than the instant.
 */

const FORMAT: Intl.DateTimeFormatOptions = {
  year: "numeric",
  month: "short",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit",
};

/**
 * What every instant crossing from the back end looks like: an RFC 3339 date,
 * whatever it carries after it.
 *
 * Checked before parsing because `Date` is lenient to the point of being
 * misleading -- it reads `"0000"` as the year zero and renders it as a real
 * date in the year 2, which looks like the tool being wrong about when
 * something happened rather than like a value it could not read. Found by a
 * test; nothing sends `"0000"` today, and nothing should have to, for the
 * failure to be the safe one.
 */
const RFC_3339_DATE = /^\d{4}-\d{2}-\d{2}[T ]/;

/**
 * An instant, in the reader's own time zone.
 *
 * Anything unreadable is returned unchanged rather than replaced with a
 * placeholder: an odd-looking timestamp is still evidence, and "unknown" is
 * not. It is also returned unchanged rather than guessed at, which is the
 * same principle one step earlier.
 */
export function readableTime(instant: string): string {
  if (!RFC_3339_DATE.test(instant)) return instant;
  const parsed = new Date(instant);
  if (Number.isNaN(parsed.getTime())) return instant;
  return parsed.toLocaleString(undefined, FORMAT);
}

/**
 * How long something has been running, in the compact form a list wants.
 *
 * Not `readableDuration` in `bytes.ts`, which says "1.5 hours" -- that is for
 * one figure in a sentence. This is for a column beside two hundred rows,
 * where "2d 2h" is scannable and "50.0 hours" is not.
 *
 * The terminal has its own copy, in Rust, in `ork_cli::processes::how_long`.
 * A round trip to the back end for every row is not worth paying to avoid six
 * duplicated lines -- but six duplicated lines drift, so both are checked
 * against `tests/shared/duration-cases.json`. Whichever one moves is the one
 * whose test fails.
 */
export function compactDuration(seconds: number): string {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${minutes}m`;
}
