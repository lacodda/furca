import { Component, type ErrorInfo, type ReactNode } from 'react'
import { Button } from './button'
import { EmptyState } from './empty-state'

/*
 * The screen that appears instead of a crash.
 *
 * A class component, and the only one in the set - not a style choice: React
 * gives no hook for catching a render error, and `componentDidCatch` exists
 * nowhere else. Anything that claims otherwise catches events, not renders.
 *
 * What it is for is narrow and worth stating, because products reach for it as
 * a general error handler and it is not one: it catches errors **thrown while
 * rendering**, below itself. A failed fetch is not that - it is a value the
 * component receives and shows, which is `QueryState`. An error in an event
 * handler is not that either; nothing catches those but the handler.
 *
 * So this is the last line: something that should never have thrown did, and
 * the alternative is a white page with the product's own name at the top.
 *
 * The fallback is deliberately plain and deliberately says the message. The
 * reader cannot fix it, but the reader is who files it - and a screen that
 * hides the one string worth quoting turns a bug report into a guess. It is
 * folded away behind a summary, because it is evidence rather than an
 * instruction.
 */

export interface ErrorBoundaryProps {
  children: ReactNode
  /** Draw something else instead of the default screen.
   *
   * Given the error and a way to clear it, because a fallback that cannot
   * retry is a dead end with extra steps. */
  fallback?: (error: Error, reset: () => void) => ReactNode
  /** Change this to clear the error - a route, usually.
   *
   * Without it a boundary that has caught once stays caught: the reader
   * navigates away from the broken screen and the crash follows them, because
   * nothing told the boundary the reason had gone. */
  resetKey?: unknown
  /** Somewhere to send it. The product owns whether that is a log, a file or a
   * service - the boundary only knows it happened. */
  onError?: (error: Error, info: ErrorInfo) => void
  /** The words on the default screen. Required, and not defaulted: they are
   * the only text here, and English inside a primitive is text no product can
   * translate. */
  labels: ErrorBoundaryLabels
}

export interface ErrorBoundaryLabels {
  /** "Something went wrong." */
  title: string
  /** What the reader can do about it. */
  body: string
  /** The summary that unfolds the message, e.g. "Details". */
  details: string
  /** The retry button, e.g. "Try again". */
  retry: string
}

interface State {
  error: Error | null
}

export class ErrorBoundary extends Component<ErrorBoundaryProps, State> {
  state: State = { error: null }

  static getDerivedStateFromError(error: Error): State {
    return { error }
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    this.props.onError?.(error, info)
  }

  componentDidUpdate(previous: ErrorBoundaryProps): void {
    // Clearing on a changed key, rather than on any re-render: a boundary that
    // resets whenever its parent renders re-runs the throwing child
    // immediately, and the screen flickers between the crash and the fallback
    // for as long as the cause is there.
    if (this.state.error !== null && previous.resetKey !== this.props.resetKey) {
      this.setState({ error: null })
    }
  }

  private reset = (): void => {
    this.setState({ error: null })
  }

  render(): ReactNode {
    const { error } = this.state
    if (error === null) return this.props.children

    const { fallback, labels } = this.props
    if (fallback) return fallback(error, this.reset)

    return (
      <EmptyState
        variant="error"
        title={labels.title}
        body={labels.body}
        action={
          <div className="flex w-full flex-col items-center gap-3">
            <details className="w-full text-left">
              <summary className="cursor-pointer text-xs text-faint">{labels.details}</summary>
              {/* Selectable and monospaced, because its job is to be copied
                * into a bug report verbatim. */}
              <pre className="mt-2 max-h-40 overflow-auto rounded-md bg-softer p-3 font-mono text-xs text-dim">
                {error.message}
              </pre>
            </details>
            <Button variant="primary" onClick={this.reset}>
              {labels.retry}
            </Button>
          </div>
        }
      />
    )
  }
}
