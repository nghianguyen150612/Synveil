import { describe, expect, it } from 'vitest'

import {
  DEFAULT_AUTHENTICATED_ROUTE,
  sanitizeReturnRoute,
} from './returnRoute'

describe('sanitizeReturnRoute', () => {
  it.each([
    '/backups',
    '/backups/abc',
    '/backups/abc?kind=PRUNE',
    '/backups/abc#activity',
    '/backups/abc?kind=RESTORE#activity',
  ])('preserves the safe same-origin path %s', (candidate) => {
    expect(sanitizeReturnRoute(candidate)).toBe(candidate)
  })

  it.each([
    '',
    'https://evil.example',
    '//evil.example',
    '/\\evil.example',
    'javascript:alert(1)',
    'data:text/html,unsafe',
    '/login?next=/backups',
    '/login/nested',
    '/auth?next=/auth',
    '/auth-required',
    '/logout',
    '/setup',
    '/%2f%2fevil.example',
    '/backups/%E0%A4%A',
  ])('rejects unsafe, looping, empty, or malformed input %s', (candidate) => {
    expect(sanitizeReturnRoute(candidate)).toBe(DEFAULT_AUTHENTICATED_ROUTE)
  })

  it('rejects non-string route state', () => {
    expect(sanitizeReturnRoute({ href: '/backups' })).toBe(DEFAULT_AUTHENTICATED_ROUTE)
  })
})
