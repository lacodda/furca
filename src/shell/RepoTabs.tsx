import { useEffect, useRef, useState, type PointerEvent, type WheelEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Tabs, TabsList, TabsTab } from '@/components/ui/tabs'
import type { Tab } from '@/lib/generated/Tab'
import { tabLabels } from '@/lib/names'
import { cn } from 'dowel-ui'

/** How far the pointer moves before a press on a tab becomes a drag. */
const THRESHOLD = 5

interface Props {
  tabs: Tab[]
  active: number | null
  onActivate: (index: number) => void
  onClose: (index: number) => void
  onMove: (from: number, to: number) => void
}

/**
 * The open repositories, in the title bar: the strip scheda made the line's
 * shape for a window of documents, where a separate tab row under the bar
 * would cost a band of every laptop screen.
 *
 * A tab is clicked to bring it forward, closed with its cross, a middle click
 * or Delete, and dragged along the strip to change its place. Dragging is done
 * with pointer events rather than the browser's drag and drop: the window
 * takes folders dropped on it from the system, and on Windows that switches
 * the webview's own drag and drop off.
 */
export function RepoTabs({ tabs, active, onActivate, onClose, onMove }: Props) {
  const { t } = useTranslation()
  const labels = tabLabels(tabs)
  const [dragging, setDragging] = useState<number | null>(null)
  const [over, setOver] = useState<number | null>(null)
  const strip = useRef<HTMLDivElement>(null)

  function indexAt(x: number, y: number): number | null {
    const tab = document.elementFromPoint(x, y)?.closest<HTMLElement>('[data-tab-index]')
    if (!tab || !strip.current?.contains(tab)) return null
    return Number(tab.dataset.tabIndex)
  }

  function startDrag(event: PointerEvent, from: number) {
    if (event.button !== 0) return
    const start = { x: event.clientX, y: event.clientY }
    let moved = false
    let target: number | null = null

    const onPointerMove = (move: globalThis.PointerEvent) => {
      if (!moved && Math.abs(move.clientX - start.x) < THRESHOLD) return
      moved = true
      setDragging(from)
      target = indexAt(move.clientX, move.clientY)
      setOver(target)
    }
    const stop = () => {
      window.removeEventListener('pointermove', onPointerMove)
      window.removeEventListener('pointerup', onPointerUp)
      window.removeEventListener('pointercancel', stop)
      setDragging(null)
      setOver(null)
    }
    const onPointerUp = () => {
      stop()
      if (moved && target !== null && target !== from) onMove(from, target)
    }
    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', onPointerUp)
    window.addEventListener('pointercancel', stop)
  }

  // The tab in front is kept in sight: opening the line puts eighteen tabs in
  // a strip with room for twelve, and the one just opened may be off the end.
  useEffect(() => {
    if (active === null) return
    strip.current
      ?.querySelector(`[data-tab-index="${String(active)}"]`)
      ?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
  }, [active, tabs.length])

  // The strip has no scrollbar - a forty-pixel bar has no room for one - so
  // the wheel, which only ever turns one way, scrolls it sideways.
  function onWheel(event: WheelEvent) {
    if (event.deltaX !== 0 || !strip.current) return
    strip.current.scrollLeft += event.deltaY
  }

  if (tabs.length === 0) return null

  return (
    <Tabs value={active} onValueChange={(value) => onActivate(value as number)} className="min-w-0">
      {/* `!` because dowel's theme draws every scrollbar thin from outside
          any layer, which outranks the bar's own `scrollbar-width: none`:
          eighteen tabs grew a scrollbar inside a forty-pixel bar. */}
      <TabsList variant="bar" aria-label={t('tabs.label')} ref={strip} className="[scrollbar-width:none]!" onWheel={onWheel}>
        {tabs.map((tab, index) => (
          <TabsTab
            key={tab.path}
            value={index}
            data-tab-index={index}
            title={tab.path}
            onPointerDown={(event) => startDrag(event, index)}
            onClose={() => onClose(index)}
            closeLabel={t('tabs.close', { name: labels[index] })}
            className={cn(
              dragging === index && 'opacity-60',
              over === index && dragging !== null && dragging !== index && 'shadow-[inset_2px_0_0_var(--accent)]',
            )}
          >
            {labels[index]}
          </TabsTab>
        ))}
      </TabsList>
    </Tabs>
  )
}
