import { useState, useSyncExternalStore } from 'react'
import { useLocation } from 'react-router-dom'

import { buildDiagnosticsBundle, serializeDiagnosticsBundle } from './format'
import { routeCategoryFromPath } from './routes'
import { diagnosticStore } from './store'
import type { DiagnosticEvent, DiagnosticRouteCategory } from './types'

const EMPTY_EVENTS: readonly DiagnosticEvent[] = []
const subscribe = (listener: () => void) => diagnosticStore.subscribe(listener)
const getSnapshot = () => diagnosticStore.all()
const getServerSnapshot = () => EMPTY_EVENTS

function formatTimestamp(value: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime())
    ? value
    : new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(date)
}

function eventStatus(event: DiagnosticEvent): string {
  return event.http_status === undefined ? 'Not applicable' : String(event.http_status)
}

function retryableLabel(event: DiagnosticEvent): string {
  return event.retryable ? 'Yes' : 'No'
}

export interface SafeDiagnosticsPanelProps {
  readonly routeCategory?: DiagnosticRouteCategory
}

export function SafeDiagnosticsPanel({ routeCategory: providedRouteCategory }: SafeDiagnosticsPanelProps) {
  const location = useLocation()
  const events = useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot)
  const routeCategory = providedRouteCategory ?? routeCategoryFromPath(location.pathname)
  const [copying, setCopying] = useState(false)
  const [announcement, setAnnouncement] = useState('')
  const currentDiagnostics = buildDiagnosticsBundle(events, routeCategory)

  async function copyDiagnostics() {
    if (copying) {
      return
    }
    setCopying(true)
    try {
      const clipboard = navigator.clipboard
      if (!clipboard || typeof clipboard.writeText !== 'function') {
        throw new Error('clipboard unavailable')
      }
      await clipboard.writeText(serializeDiagnosticsBundle(events, routeCategory))
      setAnnouncement('Diagnostics copied.')
    } catch {
      setAnnouncement('Diagnostics could not be copied. Check browser clipboard permissions and try again.')
    } finally {
      setCopying(false)
    }
  }

  function clearDiagnostics() {
    diagnosticStore.clear()
    setAnnouncement('Diagnostics cleared.')
  }

  return (
    <section className="panel diagnostics-panel" aria-labelledby="diagnostics-title">
      <div className="section-heading">
        <p className="eyebrow">Browser-only support details</p>
        <h2 id="diagnostics-title">Diagnostics</h2>
        <p>
          Recent sanitized failures can help an operator troubleshoot this browser session.
          Diagnostics stay in this browser unless you choose to copy them.
        </p>
        <p>
          No file contents, passwords, session cookies, or backup payloads are included.
        </p>
      </div>

      <div className="diagnostics-actions">
        <button type="button" onClick={() => void copyDiagnostics()} disabled={copying}>
          {copying ? 'Preparing diagnostics…' : 'Copy diagnostics'}
        </button>
        <button type="button" className="button--secondary" onClick={clearDiagnostics}>
          Clear diagnostics
        </button>
      </div>
      <p className="visually-hidden" role="status" aria-live="polite">{announcement}</p>

      {events.length === 0 ? (
        <p className="diagnostics-empty">No recent diagnostic events.</p>
      ) : (
        <div className="diagnostics-table-wrapper">
          <table className="diagnostics-table">
            <caption className="visually-hidden">Recent sanitized diagnostic events</caption>
            <thead>
              <tr>
                <th scope="col">When</th>
                <th scope="col">Severity</th>
                <th scope="col">Category</th>
                <th scope="col">Operation</th>
                <th scope="col">Status</th>
                <th scope="col">Safe code</th>
                <th scope="col">Request ID</th>
                <th scope="col">Retryable</th>
              </tr>
            </thead>
            <tbody>
              {events.map((event) => (
                <tr key={event.event_id}>
                  <td>
                    <time dateTime={event.occurred_at} title={event.occurred_at}>
                      {formatTimestamp(event.occurred_at)}
                    </time>
                    {event.occurrences && event.occurrences > 1 && (
                      <span className="diagnostics-occurrences">{event.occurrences} occurrences</span>
                    )}
                  </td>
                  <td>{event.severity}</td>
                  <td>{event.category}</td>
                  <td><code>{event.operation}</code></td>
                  <td>{eventStatus(event)}</td>
                  <td>
                    <code>{event.code}</code>
                    {event.api_error_code && (
                      <span className="diagnostics-api-code">API: <code>{event.api_error_code}</code></span>
                    )}
                  </td>
                  <td>
                    {event.server_request_id
                      ? <span className="diagnostics-request-id">Request ID: <code>{event.server_request_id}</code></span>
                      : 'Not available'}
                  </td>
                  <td>{retryableLabel(event)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <p className="diagnostics-network-note">
        Browser network status is only a hint; it does not confirm service availability.
        {currentDiagnostics.browser_online === true
          ? ' This browser currently reports online.'
          : currentDiagnostics.browser_online === false
            ? ' This browser currently reports offline.'
            : ''}
      </p>
      <p className="diagnostics-support-copy">
        If you contact support or the server administrator, include the Request ID or copied diagnostics.
      </p>
    </section>
  )
}
