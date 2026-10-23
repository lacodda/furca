import { useEffect, useState } from 'react'
import type { Palette } from './draw'

/*
 * The theme's colours as a canvas takes them. A theme token can be written
 * as a mix of others (`color-mix(in oklab, ...)`), and a canvas reads a
 * colour, not a token; so each is resolved by the page, on an element that
 * wears it, and the colour the page computed is what the canvas is given.
 */

const SERIES = 8

/** Reads the palette from the theme as it is now. */
export function readPalette(): Palette {
  const probe = document.createElement('span')
  probe.style.display = 'none'
  document.body.append(probe)
  const resolve = (token: string) => {
    probe.style.color = `var(${token})`
    return getComputedStyle(probe).color
  }
  try {
    return {
      series: Array.from({ length: SERIES }, (_, index) => resolve(`--series-${String(index + 1)}`)),
      ground: resolve('--bg'),
      head: resolve('--text'),
    }
  } finally {
    probe.remove()
  }
}

/** The palette, read again whenever the theme changes: the system switching
 * between light and dark, or the page's own theme attribute. */
export function usePalette(): Palette {
  const [palette, setPalette] = useState(readPalette)
  useEffect(() => {
    const read = () => setPalette(readPalette())
    const scheme = window.matchMedia('(prefers-color-scheme: dark)')
    scheme.addEventListener('change', read)
    const observer = new MutationObserver(read)
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['class', 'data-theme', 'style'] })
    return () => {
      scheme.removeEventListener('change', read)
      observer.disconnect()
    }
  }, [])
  return palette
}
