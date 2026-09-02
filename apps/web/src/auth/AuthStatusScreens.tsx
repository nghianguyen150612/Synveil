import { useEffect, useRef } from 'react'

interface AuthErrorScreenProps {
  readonly message?: string
  readonly onRetry: () => void
}

export function AuthLoadingScreen() {
  const headingRef = useRef<HTMLHeadingElement>(null)
  useEffect(() => headingRef.current?.focus(), [])

  return (
    <main className="auth-layout" aria-busy="true">
      <section className="auth-card panel" aria-labelledby="auth-loading-title">
        <p className="eyebrow">Synveil</p>
        <h1 id="auth-loading-title" ref={headingRef} tabIndex={-1}>Checking your session</h1>
        <p role="status" aria-live="polite">Loading account and setup state…</p>
      </section>
    </main>
  )
}

export function AuthErrorScreen({ message, onRetry }: AuthErrorScreenProps) {
  const headingRef = useRef<HTMLHeadingElement>(null)
  useEffect(() => headingRef.current?.focus(), [])

  return (
    <main className="auth-layout">
      <section className="auth-card panel" aria-labelledby="auth-error-title">
        <p className="eyebrow">Synveil</p>
        <h1 id="auth-error-title" ref={headingRef} tabIndex={-1}>Synveil is not ready</h1>
        <p>{message ?? 'Something went wrong. Please try again.'}</p>
        <button type="button" onClick={onRetry}>
          Try again
        </button>
      </section>
    </main>
  )
}

export function AuthRecoveringScreen() {
  const headingRef = useRef<HTMLHeadingElement>(null)
  useEffect(() => headingRef.current?.focus(), [])

  return (
    <main className="auth-layout" aria-busy="true">
      <section className="auth-card panel" aria-labelledby="auth-recovering-title">
        <p className="eyebrow">Protected session</p>
        <h1 id="auth-recovering-title" ref={headingRef} tabIndex={-1}>
          Restoring your session
        </h1>
        <p role="status" aria-live="polite">Checking your current session and refreshing its request proof…</p>
      </section>
    </main>
  )
}

export function AuthRecoveryErrorScreen({ message, onRetry }: AuthErrorScreenProps) {
  const headingRef = useRef<HTMLHeadingElement>(null)
  useEffect(() => headingRef.current?.focus(), [])

  return (
    <main className="auth-layout">
      <section className="auth-card panel" aria-labelledby="auth-recovery-error-title">
        <p className="eyebrow">Protected session</p>
        <h1 id="auth-recovery-error-title" ref={headingRef} tabIndex={-1}>
          Your session could not be verified
        </h1>
        <p role="alert">{message ?? 'Unable to verify your session right now.'}</p>
        <button type="button" onClick={onRetry}>Try session recovery again</button>
      </section>
    </main>
  )
}
