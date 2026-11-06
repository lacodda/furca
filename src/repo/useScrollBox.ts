import { useCallback, useLayoutEffect, useRef, useState } from 'react'

/**
 * A box that draws only what shows in it: the element to hand a `ScrollArea` as
 * its `viewportRef`, how far it is scrolled and how tall it is, watched rather
 * than read once, since the panel around it opens and the window resizes.
 */
export function useScrollBox() {
  const viewport = useRef<HTMLDivElement>(null)
  const [scrollTop, setScrollTop] = useState(0)
  const [viewportHeight, setViewportHeight] = useState(0)

  useLayoutEffect(() => {
    const element = viewport.current
    if (element === null) return
    const measure = () => {
      setViewportHeight(element.clientHeight)
      setScrollTop(element.scrollTop)
    }
    const scrolled = () => setScrollTop(element.scrollTop)
    measure()
    element.addEventListener('scroll', scrolled, { passive: true })
    const observer = new ResizeObserver(measure)
    observer.observe(element)
    return () => {
      element.removeEventListener('scroll', scrolled)
      observer.disconnect()
    }
  }, [])

  /** Scrolls to `next`. Told at once rather than by the scroll event, which
   * comes after the paint: what shows there is in the DOM for the frame that
   * shows it. */
  const scrollTo = useCallback((next: number) => {
    const element = viewport.current
    if (element === null || next === element.scrollTop) return
    element.scrollTop = next
    setScrollTop(element.scrollTop)
  }, [])

  return { viewport, scrollTop, viewportHeight, scrollTo }
}
