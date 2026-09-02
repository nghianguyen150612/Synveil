import { NavLink, Outlet } from 'react-router-dom'

function navigationClassName({ isActive }: { isActive: boolean }): string {
  return isActive ? 'navigation-link navigation-link--active' : 'navigation-link'
}

export function AppShell() {
  return (
    <div className="app-frame">
      <a className="skip-link" href="#main-content">
        Skip to main content
      </a>
      <header className="site-header">
        <div className="header-inner content-column">
          <NavLink className="brand" to="/" aria-label="Synveil home">
            <span className="brand-mark" aria-hidden="true">
              S
            </span>
            <span>
              <strong>Synveil</strong>
              <small>Private cloud foundation</small>
            </span>
          </NavLink>
          <nav aria-label="Primary navigation">
            <ul className="navigation-list">
              <li>
                <NavLink className={navigationClassName} to="/" end>
                  Home
                </NavLink>
              </li>
              <li>
                <NavLink className={navigationClassName} to="/health/dev">
                  Development health
                </NavLink>
              </li>
            </ul>
          </nav>
        </div>
      </header>
      <main id="main-content" className="main-content">
        <div className="content-column">
          <Outlet />
        </div>
      </main>
      <footer className="site-footer">
        <div className="content-column">
          <p>Foundation status: implementation in progress.</p>
        </div>
      </footer>
    </div>
  )
}
