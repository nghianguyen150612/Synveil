import { useEffect, useRef, useState, type FormEvent } from 'react'
import { useLocation } from 'react-router-dom'

import { isApiRequestError } from '../api/errors'
import { useAuth } from '../auth/useAuth'

interface LoginForm {
  login: string
  login_key: string
  password: string
}

function loginErrorMessage(error: unknown): string {
  if (isApiRequestError(error) && error.status === 401) {
    return 'The login details were not accepted.'
  }
  if (isApiRequestError(error) && error.status >= 500) {
    return 'Synveil is temporarily unavailable. Please try again.'
  }
  return 'Sign in could not be completed. Please try again.'
}

function routeNotice(state: unknown): string | undefined {
  if (typeof state !== 'object' || state === null || !('notice' in state)) {
    return undefined
  }
  const notice = state.notice
  return typeof notice === 'string' ? notice : undefined
}

export function LoginPage() {
  const location = useLocation()
  const { login, errorMessage, notice: authNotice } = useAuth()
  const [form, setForm] = useState<LoginForm>({
    login: '',
    login_key: '',
    password: '',
  })
  const [formError, setFormError] = useState<string>()
  const [submitting, setSubmitting] = useState(false)
  const headingRef = useRef<HTMLHeadingElement>(null)
  const notice = routeNotice(location.state) ?? authNotice

  useEffect(() => headingRef.current?.focus(), [])

  function update(field: keyof LoginForm, value: string) {
    setForm((current) => ({ ...current, [field]: value }))
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setFormError(undefined)
    if (!form.login.trim() || !form.login_key.trim() || !form.password) {
      setFormError('Enter your login identifier, login key, and password.')
      return
    }

    try {
      setSubmitting(true)
      await login(form)
    } catch (error) {
      setFormError(loginErrorMessage(error))
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <main className="auth-layout">
      <section className="auth-card panel" aria-labelledby="login-title">
        <p className="eyebrow">Synveil</p>
        <h1 id="login-title" ref={headingRef} tabIndex={-1}>Sign in</h1>
        <p>{notice?.includes('session has ended') ? 'Sign in again to continue.' : 'Sign in to continue to your Synveil server.'}</p>
        {notice && (
          <p className="form-note" role="status">
            {notice}
          </p>
        )}
        <form className="form-stack" onSubmit={submit} noValidate>
          <div className="field">
            <label htmlFor="login-identifier">Login identifier</label>
            <input
              id="login-identifier"
              name="login"
              type="text"
              autoComplete="username"
              value={form.login}
              onChange={(event) => update('login', event.target.value)}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="login-key">Login key</label>
            <input
              id="login-key"
              name="login_key"
              type="text"
              autoComplete="off"
              value={form.login_key}
              onChange={(event) => update('login_key', event.target.value)}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="login-password">Password</label>
            <input
              id="login-password"
              name="password"
              type="password"
              autoComplete="current-password"
              value={form.password}
              onChange={(event) => update('password', event.target.value)}
              required
            />
          </div>
          {(formError || errorMessage) && (
            <p className="form-error" role="alert">
              {formError ?? errorMessage}
            </p>
          )}
          <button type="submit" disabled={submitting}>
            {submitting ? 'Signing in…' : 'Sign in'}
          </button>
        </form>
      </section>
    </main>
  )
}
