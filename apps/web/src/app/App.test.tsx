import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { App } from './App'

describe('application bootstrap', () => {
  it('renders the foundation home route with accessible landmarks', () => {
    render(<App router="memory" initialEntries={['/']} />)

    expect(screen.getByRole('banner')).toBeInTheDocument()
    expect(screen.getByRole('main')).toBeInTheDocument()
    expect(
      screen.getByRole('heading', { name: /your data\. your devices\. your cloud/i }),
    ).toBeInTheDocument()
    expect(screen.getByRole('link', { name: /^Development health$/ })).toHaveAttribute(
      'href',
      '/health/dev',
    )
  })

  it('renders the explicit not-found route without fake product data', () => {
    render(<App router="memory" initialEntries={['/does-not-exist']} />)

    expect(screen.getByRole('heading', { name: /page does not exist/i })).toBeInTheDocument()
    expect(screen.getByRole('link', { name: /return home/i })).toHaveAttribute('href', '/')
  })
})
