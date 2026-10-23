import type { GraphRow } from '@/lib/generated/GraphRow'

/*
 * The graph column, drawn on a canvas: one canvas the height of the view,
 * redrawn as it scrolls, rather than a picture per row in the DOM. The
 * arithmetic of where things go is here and plain, so it can be checked
 * without a browser; `draw` only says it to a context.
 */

/** Sizes of the graph column, in CSS pixels. */
export interface Geometry {
  /** The height of a row. */
  row: number
  /** The width of a lane. */
  lane: number
  /** The room left of the first lane. */
  pad: number
  /** The radius of a commit's dot. */
  dot: number
}

export const GEOMETRY: Geometry = { row: 24, lane: 14, pad: 6, dot: 4 }

/** The most lanes the column is wide enough for; a graph wider than this is
 * cut at its right edge rather than pushing the subjects out of sight. */
export const MOST_LANES = 16
/** The fewest it is wide enough for, so its name in the header fits and the
 * column does not jump at every branch of a narrow history. */
export const FEWEST_LANES = 4

/** How wide the graph column is for a graph `width` lanes wide. */
export function columnWidth(width: number, geometry: Geometry = GEOMETRY): number {
  return geometry.pad * 2 + Math.min(Math.max(width, FEWEST_LANES), MOST_LANES) * geometry.lane
}

/** The middle of lane `column`. */
export function laneX(column: number, geometry: Geometry = GEOMETRY): number {
  return geometry.pad + column * geometry.lane + geometry.lane / 2
}

/** The colours the graph is drawn in, resolved from the theme. */
export interface Palette {
  /** One per lane colour, in order. */
  series: string[]
  /** What a merge's hollow dot is filled with: the ground under the rows. */
  ground: string
  /** The ring around the commit `HEAD` is on. */
  head: string
}

/** One stroke of the picture: a straight segment or an S-curve between two
 * points, each end leaving or reaching its point upright. */
export interface Stroke {
  color: number
  from: [number, number]
  to: [number, number]
}

/** The strokes and the dot of one row, its top at `top`. */
export function rowStrokes(row: GraphRow['row'], top: number, geometry: Geometry = GEOMETRY): Stroke[] {
  const middle = top + geometry.row / 2
  const bottom = top + geometry.row
  return row.segments.map((segment) => {
    const from = laneX(segment.from, geometry)
    const to = laneX(segment.to, geometry)
    switch (segment.kind) {
      case 'pass':
        return { color: segment.color, from: [from, top], to: [to, bottom] }
      case 'in':
        return { color: segment.color, from: [from, top], to: [to, middle] }
      case 'out':
        return { color: segment.color, from: [from, middle], to: [to, bottom] }
    }
  })
}

/** Draws `rows`, the first of them at `top`, into `context`. A row not read
 * yet is `undefined` and is left blank until it is. */
export function draw(
  context: CanvasRenderingContext2D,
  rows: readonly (GraphRow | undefined)[],
  top: number,
  palette: Palette,
  head: string | null,
  geometry: Geometry = GEOMETRY,
): void {
  context.lineWidth = 1.5
  context.lineCap = 'round'
  rows.forEach((graphRow, index) => {
    if (!graphRow) return
    const y = top + index * geometry.row
    for (const stroke of rowStrokes(graphRow.row, y, geometry)) {
      context.strokeStyle = colorOf(palette, stroke.color)
      context.beginPath()
      context.moveTo(...stroke.from)
      const [fromX, fromY] = stroke.from
      const [toX, toY] = stroke.to
      if (fromX === toX) {
        context.lineTo(toX, toY)
      } else {
        const turn = (fromY + toY) / 2
        context.bezierCurveTo(fromX, turn, toX, turn, toX, toY)
      }
      context.stroke()
    }

    const x = laneX(graphRow.row.column, geometry)
    const middle = y + geometry.row / 2
    const color = colorOf(palette, graphRow.row.color)
    const merge = graphRow.commit.parents.length > 1
    context.beginPath()
    context.arc(x, middle, geometry.dot, 0, Math.PI * 2)
    context.fillStyle = merge ? palette.ground : color
    context.fill()
    if (merge) {
      context.strokeStyle = color
      context.stroke()
    }
    if (graphRow.commit.id === head) {
      context.beginPath()
      context.arc(x, middle, geometry.dot + 2.5, 0, Math.PI * 2)
      context.strokeStyle = palette.head
      context.stroke()
    }
  })
}

function colorOf(palette: Palette, color: number): string {
  return palette.series[color % palette.series.length] ?? palette.head
}
