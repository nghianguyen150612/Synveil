import { useState, type FormEvent } from 'react'

import { isApiRequestError } from '../api/errors'
import { useAuth } from '../auth/useAuth'

interface SetupForm {
  login: string
  login_key: string
  password: string
  confirmPassword: string
}

function setupErrorMessage(error: unknown): string {
  if (isApiRequestError(error) && error.status === 409) {
    return 'Initial setup is already complete. You can sign in instead.'
  }
  if (isApiRequestError(error) && error.status >= 500) {
    return 'Synveil is temporarily unavailable. Please try again.'
  }
  return 'Setup could not be completed. Please check the fields and try again.'
}

export function SetupPage() {
  const { bootstrap, errorMessage } = useAuth()
  const [form, setForm] = useState<SetupForm>({
    login: '',
    login_key: '',
    password: '',
    confirmPassword: '',
  })
  const [formError, setFormError] = useState<string>()
  const [submitting, setSubmitting] = useState(false)

  function update(field: keyof SetupForm, value: string) {
    setForm((current) => ({ ...current, [field]: value }))
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setFormError(undefined)

    if (!form.login.trim() || !form.login_key.trim() || !form.password) {
      setFormError('Enter a login identifier, login key, and password.')
      return
    }
    if (form.password !== form.confirmPassword) {
      setFormError('The passwords do not match.')
      return
    }

    try {
      setSubmitting(true)
      await bootstrap({
        login: form.login,
        login_key: form.login_key,
        password: form.password,
      })
    } catch (error) {
      if (isApiRequestError(error) && error.status === 409) {
        return
      }
      setFormError(setupErrorMessage(error))
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <main className="auth-layout">
      <section className="auth-card panel" aria-labelledby="setup-title">
        <p className="eyebrow">First run</p>
        <h1 id="setup-title">Set up Synveil</h1>
        <p>
          Create the first administrator account for this Synveil server. This
          one-time setup closes after the account is created.
        </p>
        <form className="form-stack" onSubmit={submit} noValidate>
          <div className="field">
            <label htmlFor="setup-login">Login identifier</label>
            <input
              id="setup-login"
              name="login"
              type="text"
              autoComplete="username"
              value={form.login}
              onChange={(event) => update('login', event.target.value)}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="setup-login-key">Login key</label>
            <input
              id="setup-login-key"
              name="login_key"
              type="text"
              autoComplete="off"
              value={form.login_key}
              onChange={(event) => update('login_key', event.target.value)}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="setup-password">Password</label>
            <input
              id="setup-password"
              name="password"
              type="password"
              autoComplete="new-password"
              value={form.password}
              onChange={(event) => update('password', event.target.value)}
              required
            />
          </div>
          <div className="field">
            <label htmlFor="setup-confirm-password">Confirm password</label>
            <input
              id="setup-confirm-password"
              name="confirm_password"
              type="password"
              autoComplete="new-password"
              value={form.confirmPassword}
              onChange={(event) => update('confirmPassword', event.target.value)}
              required
            />
          </div>
          {(formError || errorMessage) && (
            <p className="form-error" role="alert">
              {formError ?? errorMessage}
            </p>
          )}
          <button type="submit" disabled={submitting}>
            {submitting ? 'Creating account…' : 'Create administrator'}
          </button>
        </form>
      </section>
    </main>
  )
}
