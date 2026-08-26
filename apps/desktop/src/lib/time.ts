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
 * An instant, in the reader's own time zone.
 *
 * Anything unparseable is returned unchanged rather than replaced with a
 * placeholder: an odd-looking timestamp is still evidence, and "unknown" is
 * not.
 */
export function readableTime(instant: string): string {
  const parsed = new Date(instant);
  if (Number.isNaN(parsed.getTime())) return instant;
  return parsed.toLocaleString(undefined, FORMAT);
}
