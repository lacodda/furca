import { invoke, type Channel } from '@tauri-apps/api/core'
import type { Details } from './generated/Details'
import type { FileChange } from './generated/FileChange'
import type { FileDiff } from './generated/FileDiff'
import type { Finder } from './generated/Finder'
import type { GitRun } from './generated/GitRun'
import type { Found } from './generated/Found'
import type { GraphAnswer } from './generated/GraphAnswer'
import type { GraphRow } from './generated/GraphRow'
import type { Hunk } from './generated/Hunk'
import type { LastCommit } from './generated/LastCommit'
import type { Layout } from './generated/Layout'
import type { Line } from './generated/Line'
import type { List } from './generated/List'
import type { MessageCheck } from './generated/MessageCheck'
import type { Opened } from './generated/Opened'
import type { Parts } from './generated/Parts'
import type { Plan } from './generated/Plan'
import type { ReleaseState } from './generated/ReleaseState'
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
  setLayout: (layout: Layout) => invoke<WindowState>('set_layout', { layout }),
  line: () => invoke<Line>('line'),
  openLine: () => invoke<Opened>('open_line'),
  sidebar: (path: string) => invoke<Sidebar>('sidebar', { path }),
  standing: (path: string) => invoke<Standing>('standing', { path }),
  maintain: (path: string) => invoke<null>('maintain', { path }),
  /** The graph of `path`, of one branch (a full ref name) or of everything. */
  graph: (path: string, branch: string | null) => invoke<GraphAnswer>('graph', { path, branch }),
  graphRows: (path: string, generation: number, from: number, count: number) =>
    invoke<GraphRow[]>('graph_rows', { path, generation, from, count }),
  graphPosition: (path: string, generation: number, id: string) =>
    invoke<number | null>('graph_position', { path, generation, id }),
  /** Starts a search; what it finds arrives on `found`, and the answer says
   * who is searching. */
  graphFind: (path: string, generation: number, query: string, found: Channel<Found>) =>
    invoke<Finder[]>('graph_find', { path, generation, query, found }),
  forgetGraph: (path: string) => invoke<null>('forget_graph', { path }),
  /** The commit `id`: who made it, its whole message, the files it changed. */
  details: (path: string, id: string) => invoke<Details>('details', { path, id }),
  /** One file's change, as `details` listed it: both texts and the hunks. */
  fileDiff: (path: string, file: FileChange) => invoke<FileDiff>('file_diff', { path, file }),
  /** Keeps the working copy of `path` up to date, in place of the one kept
   * now, and reads it whole if it was that one already; `null` keeps none.
   * What it finds arrives as `working-copy` events. */
  watchWorkingCopy: (path: string | null) => invoke<null>('watch_working_copy', { path }),
  /** What the release panel of `path` starts from. */
  releaseState: (path: string) => invoke<ReleaseState>('release_state', { path }),
  /** The next release of `path` and what stands in its way; changes nothing. */
  releasePlan: (path: string) => invoke<Plan>('release_plan', { path }),
  /** Starts a run, or carries on the one that stopped, and returns at once:
   * it is heard as `release` events. */
  releaseRun: (path: string, resume: boolean) => invoke<null>('release_run', { path, resume }),
  /** Stages exactly `paths`; the list is heard again afterwards, as every
   * change to the index is. */
  stagePaths: (path: string, paths: string[]) => invoke<null>('stage_paths', { path, paths }),
  /** Stages a whole list, however many rows of it the window was given. */
  stageList: (path: string, list: List) => invoke<null>('stage_list', { path, list }),
  unstagePaths: (path: string, paths: string[]) => invoke<null>('unstage_paths', { path, paths }),
  unstageAll: (path: string) => invoke<null>('unstage_all', { path }),
  stageHunk: (path: string, file: FileChange, hunk: Hunk) => invoke<null>('stage_hunk', { path, file, hunk }),
  unstageHunk: (path: string, file: FileChange, hunk: Hunk) => invoke<null>('unstage_hunk', { path, file, hunk }),
  /** The types of the convention, as the core keeps them. */
  commitTypes: () => invoke<string[]>('commit_types'),
  /** Reads `message`, and asks the repository's own `commit-msg` hook of it. */
  checkMessage: (path: string, message: string) => invoke<MessageCheck>('check_message', { path, message }),
  /** Commits what is staged, or puts it in the last commit; the new commit's id. */
  commit: (path: string, message: string, amend: boolean) => invoke<string>('commit', { path, message, amend }),
  lastCommit: (path: string) => invoke<LastCommit>('last_commit', { path }),
  /** Whether the Claude Code CLI is on this machine to draft a message. */
  draftAvailable: () => invoke<boolean>('draft_available'),
  draftMessage: (path: string) => invoke<Parts>('draft_message', { path }),
  /** What git the window has had run in `path`, oldest first. */
  consoleLog: (path: string) => invoke<GitRun[]>('console_log', { path }),
  /** Tells a measurement the window reached `stage`; nothing otherwise. The
   * stages between the start and the first frame say where a slow start
   * spent its time; the two diff stages time the first diff opened. */
  measured: (stage: 'script' | 'state' | 'read' | 'first-frame' | 'settled' | 'diff-asked' | 'diff-shown') =>
    invoke<null>('measured', { stage }),
}

/** A command's failure as the window shows it: Tauri hands over the Rust
 * error's message as a plain string. */
export function failure(cause: unknown): string {
  return typeof cause === 'string' ? cause : cause instanceof Error ? cause.message : String(cause)
}
