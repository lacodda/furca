import { createContext, useContext, type KeyboardEvent, type MouseEvent, type ReactNode } from 'react'
import { Tabs as Base } from '@base-ui/react/tabs'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from 'dowel-ui'

/*
 * Tabs - one place on a screen showing one of several views, or a window
 * showing one of several open documents.
 *
 * Two shapes, because the line has two needs and they look nothing alike.
 * `line` is the familiar row of words over a panel, the active one underlined
 * in the accent: settings split into sections, a record's details and its
 * history. `bar` is the strip of open documents that lives *inside* a
 * frameless window's title bar - scheda's trade, where a separate tab row under
 * a system title bar cost about sixty pixels of every laptop screen. A bar tab
 * is as tall as the bar, the active one takes the page's ground so it reads as
 * joined to the document below, and a two-pixel accent rule on its top edge
 * marks it where an underline would sit against the window's edge.
 *
 * Behaviour is Base UI's: arrow keys move between tabs, Home and End go to
 * the ends, the list loops, and the panel is wired to its tab with the ids a
 * reader needs. Nothing here re-implements that.
 *
 * A document tab can close, and that is where the obvious markup is wrong.
 * A close button *inside* a `tab` is an interactive element nested in another,
 * which a screen reader flattens into one control and a keyboard cannot reach
 * separately. So the cross is a sibling drawn over the tab, for the pointer
 * only - taken out of the tab order and the reading - and the keyboard closes
 * the focused tab with Delete, which the tab announces through
 * `aria-keyshortcuts`. A middle click closes too, the way every tabbed thing
 * does. The cross is always drawn, faintly, rather than revealed on hover: a
 * button that appears under the pointer is one the pointer was not aiming for.
 *
 * A document with unsaved changes carries a dot in the accent. The dot is
 * drawing; the words `modifiedLabel` gives are what a reader hears, and the
 * type will not accept one without the other.
 */

export function Tabs({ className, ...props }: Base.Root.Props) {
  return <Base.Root className={cn('flex min-w-0 flex-col data-[orientation=vertical]:flex-row', className)} {...props} />
}

export const tabsListVariants = cva('relative flex min-w-0', {
  variants: {
    variant: {
      line: 'gap-1 border-b border-line data-[orientation=vertical]:flex-col data-[orientation=vertical]:border-r data-[orientation=vertical]:border-b-0',
      // The strip scrolls sideways rather than squeezing: a window with twenty
      // documents open keeps every name legible and lets the bar scroll. The
      // scrollbar itself stays out of a forty-pixel bar.
      bar: 'h-full items-stretch overflow-x-auto [scrollbar-width:none]',
    },
  },
  defaultVariants: { variant: 'line' },
})

export interface TabsListProps extends Base.List.Props, VariantProps<typeof tabsListVariants> {
  /** A name for the list, when nothing on the screen already says what the
   * tabs choose between. Documents in a window bar need one. */
  'aria-label'?: string
}

type Variant = NonNullable<TabsListProps['variant']>

/* The list tells its tabs which shape they are, through context rather than a
 * CSS ancestor selector: a panel of `line` tabs inside a `bar` document would
 * match the outer list's selector too. */
const VariantContext = createContext<Variant>('line')

export function TabsList({ variant, className, children, ...props }: TabsListProps) {
  const shape: Variant = variant ?? 'line'
  return (
    <Base.List className={cn(tabsListVariants({ variant: shape }), className)} {...props}>
      <VariantContext.Provider value={shape}>{children}</VariantContext.Provider>
      {shape === 'line' && (
        <Base.Indicator
          className={cn(
            // Base UI measures the active tab and hands its box over as CSS
            // variables, so the rule slides between tabs rather than jumping.
            'absolute bottom-0 left-(--active-tab-left) h-0.5 w-(--active-tab-width) translate-y-px bg-accent',
            'transition-[left,width] duration-quick ease-out',
            'data-[orientation=vertical]:top-(--active-tab-top) data-[orientation=vertical]:right-0 data-[orientation=vertical]:bottom-auto data-[orientation=vertical]:left-auto',
            'data-[orientation=vertical]:h-(--active-tab-height) data-[orientation=vertical]:w-0.5 data-[orientation=vertical]:translate-x-px data-[orientation=vertical]:translate-y-0',
            'data-[orientation=vertical]:transition-[top,height]',
          )}
        />
      )}
    </Base.List>
  )
}

const tabBase = cn(
  'flex cursor-default items-center gap-1.5 whitespace-nowrap text-dim outline-none select-none',
  // An icon beside a tab's name is sized like one in a small button: at its
  // own 24px it stood taller than the text it labels.
  '[&_svg:not([class*=size-])]:size-3.5 [&_svg]:shrink-0',
  'transition-colors duration-quick',
  'hover:text-text data-[active]:text-text',
  'focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-accent',
  'data-[disabled]:cursor-not-allowed data-[disabled]:opacity-50',
)

/** The shape of a tab, told by the list it sits in. */
const tabShape: Record<Variant, string> = {
  line: 'h-control-sm rounded-md px-3 text-sm',
  bar: cn(
    'h-full border-r border-line pl-3 text-xs',
    'hover:bg-soft data-[active]:bg-bg data-[active]:shadow-[inset_0_2px_0_var(--accent)]',
  ),
}

type Modified =
  | { modified?: false; modifiedLabel?: never }
  | {
      /** The document has changes that are not saved. */
      modified: true
      /** What a reader hears for the dot, e.g. "unsaved changes". */
      modifiedLabel: string
    }

type Closable =
  | { onClose?: never; closeLabel?: never }
  | {
      /** Closes the tab: from the cross, a middle click, or Delete. */
      onClose: () => void
      /** The cross's tooltip, e.g. "Close notes.md". */
      closeLabel: string
    }

export type TabsTabProps = Omit<Base.Tab.Props, 'children'> & {
  children: ReactNode
} & Modified &
  Closable

export function TabsTab({
  className,
  children,
  modified,
  modifiedLabel,
  onClose,
  closeLabel,
  onKeyDown,
  onAuxClick,
  ...props
}: TabsTabProps) {
  const variant = useContext(VariantContext)
  const tab = (
    <Base.Tab
      className={cn(tabBase, tabShape[variant], onClose && (variant === 'bar' ? 'pr-8' : 'pr-9'), className)}
      aria-keyshortcuts={onClose ? 'Delete' : undefined}
      onKeyDown={(event: KeyboardEvent<HTMLElement>) => {
        onKeyDown?.(event as Parameters<NonNullable<typeof onKeyDown>>[0])
        if (onClose && event.key === 'Delete' && !event.defaultPrevented) {
          event.preventDefault()
          onClose()
        }
      }}
      onAuxClick={(event: MouseEvent<HTMLElement>) => {
        onAuxClick?.(event as Parameters<NonNullable<typeof onAuxClick>>[0])
        if (onClose && event.button === 1) {
          event.preventDefault()
          onClose()
        }
      }}
      {...props}
    >
      <span className="min-w-0 max-w-56 truncate">{children}</span>
      {modified && (
        <>
          <span aria-hidden className="size-1.5 shrink-0 rounded-full bg-accent" />
          {/* A space of its own, outside the spans: name computation trims
              each element's text, so a space inside one is lost and the
              reader hears "plan.mdunsaved changes". */}
          {' '}
          <span className="sr-only">{modifiedLabel}</span>
        </>
      )}
    </Base.Tab>
  )

  if (!onClose) return tab

  return (
    <span className="relative flex shrink-0">
      {tab}
      <button
        type="button"
        tabIndex={-1}
        aria-hidden
        title={closeLabel}
        onClick={onClose}
        className={cn(
          'target-min absolute top-1/2 right-2 flex size-4 -translate-y-1/2 cursor-default items-center justify-center rounded-sm',
          'text-faint transition-colors duration-quick hover:bg-soft hover:text-text',
        )}
      >
        <svg width="8" height="8" viewBox="0 0 8 8" fill="none" stroke="currentColor" strokeWidth="1.2" aria-hidden>
          <path d="M0.5 0.5l7 7M7.5 0.5l-7 7" />
        </svg>
      </button>
    </span>
  )
}

export function TabsPanel({ className, ...props }: Base.Panel.Props) {
  return (
    <Base.Panel
      className={cn(
        'min-w-0 flex-1 outline-none focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-accent',
        className,
      )}
      {...props}
    />
  )
}
