import { useAuth } from '../../auth/useAuth'

export function HomePage() {
  const { session } = useAuth()

  return (
    <div className="page-stack">
      <section className="hero-panel" aria-labelledby="home-title">
        <p className="eyebrow">Authenticated shell</p>
        <h1 id="home-title">Welcome to Synveil</h1>
        <p className="hero-copy">
          You are signed in. This small shell confirms the browser session and
          leaves product areas behind their own contracts.
        </p>
        <p className="notice" role="status">
          Files, synchronization, backups, sharing, and device management are
          not available in this authentication phase.
        </p>
      </section>

      <section aria-labelledby="boundaries-title">
        <div className="section-heading">
          <p className="eyebrow">Safe identity</p>
          <h2 id="boundaries-title">Current session</h2>
        </div>
        <div className="panel session-summary">
          <p>
            Session status: <strong>authenticated</strong>
          </p>
          {session && (
            <p>
              Administrator access: {session.is_instance_admin ? 'enabled' : 'not enabled'}
            </p>
          )}
        </div>
      </section>
    </div>
  )
}
