/**
 * The classes for a handle between a list and the change beside it, whose
 * grab zone lies wholly on the list's side of the line. `target-min` grows
 * the zone evenly about the line, and half of it then covered the left edge
 * of the change's line numbers: a click on a number close to the border took
 * the handle instead of the line. The list's right edge is a row's padding,
 * and gives nothing away.
 */
export const GRAB_ON_THE_LIST = 'after:left-auto after:right-0 after:translate-x-0 after:-translate-y-1/2'
