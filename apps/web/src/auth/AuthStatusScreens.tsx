interface AuthErrorScreenProps {
  readonly message?: string
  readonly onRetry: () => void
}

export function AuthLoadingScreen() {
  return (
    <main className="auth-layout" aria-busy="true">
      <section className="auth-card panel" aria-labelledby="auth-loading-title">
        <p className="eyebrow">Synveil</p>
        <h1 id="auth-loading-title">Checking your server</h1>
        <p role="status">Loading account and setup state…</p>
      </section>
    </main>
  )
}

export function AuthErrorScreen({ message, onRetry }: AuthErrorScreenProps) {
  return (
    <main className="auth-layout">
      <section className="auth-card panel" aria-labelledby="auth-error-title">
        <p className="eyebrow">Synveil</p>
        <h1 id="auth-error-title">Synveil is not ready</h1>
        <p>{message ?? 'Something went wrong. Please try again.'}</p>
        <button type="button" onClick={onRetry}>
          Try again
        </button>
      </section>
    </main>
  )
}
