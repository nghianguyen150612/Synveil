import { StatusPill } from '../../components/ui/StatusPill'
import { SafeDiagnosticsPanel } from '../../diagnostics/SafeDiagnosticsPanel'

const healthRoutes = [
  {
    path: '/health/live',
    meaning: 'Minimal process liveness probe.',
  },
  {
    path: '/health/ready',
    meaning: 'Bounded readiness probe for required dependencies.',
  },
  {
    path: '/api/v1/system/health',
    meaning: 'Restricted detailed operational health view.',
  },
]

export function DevHealthPage() {
  return (
    <div className="page-stack">
      <section className="page-heading" aria-labelledby="dev-health-title">
        <StatusPill tone="planned">Development placeholder</StatusPill>
        <p className="eyebrow">Health boundary</p>
        <h1 id="dev-health-title">Health is not connected yet</h1>
        <p>
          This route records the intended API boundary. It deliberately makes
          no network request and does not report live system state.
        </p>
      </section>

      <section className="panel" aria-labelledby="health-routes-title">
        <div className="section-heading">
          <h2 id="health-routes-title">Reviewed API routes</h2>
          <p>
            Future health UI will translate safe server responses into clear
            next actions while keeping restricted detail behind authorization.
          </p>
        </div>
        <ul className="route-list">
          {healthRoutes.map((route) => (
            <li key={route.path}>
              <code>{route.path}</code>
              <span>{route.meaning}</span>
            </li>
          ))}
        </ul>
      </section>

      <SafeDiagnosticsPanel />
    </div>
  )
}
