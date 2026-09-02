import { useEffect, useState } from 'react'
import { NavLink, Outlet, useLocation } from 'react-router-dom'

import { useAuth } from '../../auth/useAuth'
import { PendingMutationAccountWarning } from '../../features/backups/PendingMutationAccountWarning'

function navigationClassName({ isActive }: { isActive: boolean }): string {
  return isActive ? 'navigation-link navigation-link--active' : 'navigation-link'
}

export function AppShell() {
  const { generation, logout, session, status } = useAuth()
  const location = useLocation()
  const [loggingOut, setLoggingOut] = useState(false)

  useEffect(() => {
    if (status === 'recovering' || status === 'recovery_error') {
      return
    }
    const heading = document.querySelector<HTMLElement>('#cross-account-retry-title')
      ?? document.querySelector<HTMLElement>('#main-content h1')
    if (heading) {
      heading.setAttribute('tabindex', '-1')
      heading.focus()
    }
  }, [generation, location.hash, location.pathname, location.search, status])

  async function performLogout() {
    if (loggingOut) {
      return
    }
    setLoggingOut(true)
    try {
      await logout()
    } finally {
      setLoggingOut(false)
    }
  }

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
                <NavLink className={navigationClassName} to="/backups">
                  Backups
                </NavLink>
              </li>
              <li>
                <NavLink className={navigationClassName} to="/health/dev">
                  Development health
                </NavLink>
              </li>
            </ul>
          </nav>
          <div className="shell-actions">
            <span className="signed-in-label">
              Signed in
              {session?.is_instance_admin ? ' · Administrator' : ''}
            </span>
            <button
              type="button"
              className="button--secondary"
              onClick={() => void performLogout()}
              disabled={loggingOut}
            >
              {loggingOut ? 'Signing out…' : 'Logout'}
            </button>
          </div>
        </div>
      </header>
      <main id="main-content" className="main-content">
        <div className="content-column">
          <PendingMutationAccountWarning />
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
