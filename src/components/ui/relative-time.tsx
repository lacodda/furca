import { useState, type TimeHTMLAttributes } from 'react'
import { cn, useLocale } from 'dowel-ui'

/*
 * "3 minutes ago", in the application's language, over a date they can still read.
 *
 * The rule the component is built on: **relative time is a convenience, never
 * the only copy of the fact.** "Last week" is quicker to read than a date and
 * useless the moment the reader needs to say when something actually happened
 * - and they cannot get it back, because the page has thrown it away. So the
 * element is a `<time dateTime=…>` with the exact moment in its `title`: the
 * machine-readable value stays, hovering shows the real date, and copying the
 * text still yields something a person can act on.
 *
 * `Intl.RelativeTimeFormat` writes the phrase, which is the whole reason there
 * is no date library here. "yesterday", "3 недели назад", "in 2 months" - the
 * plural rules and the special words for the nearest units are the part a
 * hand-written version gets wrong first, and it gets it wrong only in the
 * languages its author does not read.
 *
 * What `Intl` does not decide is which unit to use: it is told "3" and
 * "weeks". Choosing between them is `relativeParts` below, and it is the one
 * piece of arithmetic here.
 *
 * Deliberately not self-updating. A component that re-renders every second to
 * keep "2 minutes ago" honest costs a timer per instance - a table of fifty
 * rows is fifty timers - to correct a number nobody is watching change. A
 * product that needs it re-renders the list on its own schedule; the `now`
 * prop is there so it can, and so tests are not written against the clock.
 */

/** The thresholds, largest unit first: how many seconds it takes to earn one,
 * and the unit `Intl` should be handed.
 *
 * A month is 30 days and a year is 365. Both are approximations, and they are
 * the right ones: this is a phrase saying roughly how long ago, and a reader
 * who needs the exact interval is reading the date in the `title` instead. */
const UNITS: [seconds: number, unit: Intl.RelativeTimeFormatUnit][] = [
  [60 * 60 * 24 * 365, 'year'],
  [60 * 60 * 24 * 30, 'month'],
  [60 * 60 * 24 * 7, 'week'],
  [60 * 60 * 24, 'day'],
  [60 * 60, 'hour'],
  [60, 'minute'],
  [1, 'second'],
]

/** How long ago, as a count and a unit: what `Intl.RelativeTimeFormat` needs.
 *
 * The sign is the one `Intl` wants - negative for the past - and the count is
 * rounded towards zero. Rounding to nearest is the tempting alternative and
 * says "in 1 hour" 31 minutes before the meeting; truncating never claims more
 * time has passed than has. */
export function relativeParts(
  from: Date | number | string,
  now: Date | number = Date.now(),
): [value: number, unit: Intl.RelativeTimeFormatUnit] {
  const then = new Date(from).getTime()
  const seconds = (then - new Date(now).getTime()) / 1000
  const magnitude = Math.abs(seconds)

  for (const [size, unit] of UNITS) {
    if (magnitude >= size) return [Math.trunc(seconds / size), unit]
  }
  // Under a second either way. Zero seconds rather than the smallest unit,
  // because `Intl` turns that into "now" in every language it knows.
  return [0, 'second']
}

/** The phrase alone, for a `title`, an `aria-label` or a string. */
export function formatRelative(
  from: Date | number | string,
  locale: string,
  now: Date | number = Date.now(),
  options?: Intl.RelativeTimeFormatOptions,
): string {
  const [value, unit] = relativeParts(from, now)
  return new Intl.RelativeTimeFormat(locale, { numeric: 'auto', ...options }).format(value, unit)
}

export interface RelativeTimeProps extends Omit<TimeHTMLAttributes<HTMLTimeElement>, 'title'> {
  /** The moment being described. A `Date`, epoch milliseconds, or an ISO
   * string - whichever the data already holds. */
  value: Date | number | string
  /** What counts as now. Given rather than read from the clock so a list can
   * re-render on its own schedule, and so a test is not written against the
   * time it runs at. */
  now?: Date | number
  /** The application's language by default - see `useLocale`. */
  locale?: string
  /** `'auto'` by default, which is what produces "yesterday" rather than "1
   * day ago" where the language has a word for it. Pass `'always'` for a
   * column where every row should read the same way. */
  numeric?: Intl.RelativeTimeFormatOptions['numeric']
  /** How the exact moment is written in the `title`. The application's own format
   * by default. */
  titleOptions?: Intl.DateTimeFormatOptions
}

export function RelativeTime({
  value,
  now,
  locale,
  numeric = 'auto',
  titleOptions = { dateStyle: 'medium', timeStyle: 'short' },
  className,
  ...props
}: RelativeTimeProps) {
  /* Read once, at mount, rather than in a default argument.
   *
   * `now = Date.now()` in the parameter list is the obvious spelling and is
   * impure: it is evaluated on every render, so "now" moves whenever the
   * parent happens to re-render and the phrase changes for reasons that have
   * nothing to do with this component. Caught by `react-hooks/purity`, and
   * worth keeping caught - a component that deliberately does not update
   * itself must not update itself by accident either.
   *
   * With `now` given, the state is initialised and never read, which is what a
   * product paging a list wants: every row is measured from the same moment. */
  const [mountedAt] = useState(() => Date.now())
  const language = useLocale(locale)
  const moment = new Date(value)
  const invalid = Number.isNaN(moment.getTime())

  if (invalid) {
    // A date that is not a date renders as nothing rather than as "Invalid
    // Date", which is a string no reader can do anything with and which looks
    // like a value. The `<time>` element with no `dateTime` says the same to a
    // machine: there is no moment here.
    return <time className={className} {...props} />
  }

  return (
    <time
      dateTime={moment.toISOString()}
      // The fact itself, kept. The phrase above it is the convenience.
      title={new Intl.DateTimeFormat(language, titleOptions).format(moment)}
      className={cn('whitespace-nowrap', className)}
      {...props}
    >
      {formatRelative(moment, language, now ?? mountedAt, { numeric })}
    </time>
  )
}
