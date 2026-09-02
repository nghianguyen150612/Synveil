import { Link } from 'react-router-dom'

import { StatusPill } from '../../components/ui/StatusPill'

export function HomePage() {
  return (
    <div className="page-stack">
      <section className="hero-panel" aria-labelledby="home-title">
        <StatusPill tone="planned">Web foundation</StatusPill>
        <h1 id="home-title">Your data. Your devices. Your cloud.</h1>
        <p className="hero-copy">
          Synveil is being built as a reliable private cloud that stays
          understandable for people and controllable for self-hosters.
        </p>
        <p className="notice" role="status">
          This is an application foundation. No account, files, or production
          health data have been loaded.
        </p>
      </section>

      <section aria-labelledby="boundaries-title">
        <div className="section-heading">
          <p className="eyebrow">Starting points</p>
          <h2 id="boundaries-title">Clear boundaries for future features</h2>
        </div>
        <ul className="boundary-grid">
          <li>
            <article className="boundary-card">
              <StatusPill tone="info">Ready for integration</StatusPill>
              <h3>Public API boundary</h3>
              <p>
                Browser requests will pass through one typed client boundary,
                with safe API errors kept separate from UI copy.
              </p>
            </article>
          </li>
          <li>
            <article className="boundary-card">
              <StatusPill>Not configured</StatusPill>
              <h3>Account state</h3>
              <p>
                Authentication is reserved as a boundary. Login behavior and
                credentials are not implemented in this skeleton.
              </p>
            </article>
          </li>
          <li>
            <article className="boundary-card">
              <StatusPill tone="planned">Planned</StatusPill>
              <h3>Core product areas</h3>
              <p>
                Files, Photos, Backups, Devices, Shared, Search, Activity,
                Storage, and Health will appear only as their contracts pass.
              </p>
            </article>
          </li>
        </ul>
      </section>

      <section className="next-step-panel" aria-labelledby="next-step-title">
        <div>
          <p className="eyebrow">For development</p>
          <h2 id="next-step-title">Inspect the health route boundary</h2>
          <p>
            The development page documents where health information will live
            without pretending that a probe has run.
          </p>
        </div>
        <Link className="button button--secondary" to="/health/dev">
          Open development health
        </Link>
      </section>
    </div>
  )
}
