import { invoke } from '@tauri-apps/api/core'
import type { Line } from './generated/Line'
import type { Opened } from './generated/Opened'
import type { Sidebar } from './generated/Sidebar'
import type { Standing } from './generated/Standing'
import type { WindowState } from './generated/WindowState'

/*
 * The commands of furca-desktop, typed. The shapes are generated from the
 * Rust types (ADR 0009); this file only names the calls, so a renamed
 * argument is one place to change and the compiler finds the rest.
 *
 * The store is the one truth about the tabs: every call that changes them
 * returns the state the window draws next.
 */

export const api = {
  windowState: () => invoke<WindowState>('window_state'),
  openTab: (path: string) => invoke<WindowState>('open_tab', { path }),
  closeTab: (index: number) => invoke<WindowState>('close_tab', { index }),
  activateTab: (index: number) => invoke<WindowState>('activate_tab', { index }),
  moveTab: (from: number, to: number) => invoke<WindowState>('move_tab', { from, to }),
  saveSession: (name: string) => invoke<WindowState>('save_session', { name }),
  openSession: (name: string) => invoke<Opened>('open_session', { name }),
  deleteSession: (name: string) => invoke<WindowState>('delete_session', { name }),
  line: () => invoke<Line>('line'),
  openLine: () => invoke<Opened>('open_line'),
  sidebar: (path: string) => invoke<Sidebar>('sidebar', { path }),
  standing: (path: string) => invoke<Standing>('standing', { path }),
  maintain: (path: string) => invoke<null>('maintain', { path }),
  /** Tells a measurement the window reached `stage`; nothing otherwise. The
   * stages between the start and the first frame say where a slow start
   * spent its time. */
  measured: (stage: 'script' | 'state' | 'read' | 'first-frame' | 'settled') =>
    invoke<null>('measured', { stage }),
}

/** A command's failure as the window shows it: Tauri hands over the Rust
 * error's message as a plain string. */
export function failure(cause: unknown): string {
  return typeof cause === 'string' ? cause : cause instanceof Error ? cause.message : String(cause)
}
