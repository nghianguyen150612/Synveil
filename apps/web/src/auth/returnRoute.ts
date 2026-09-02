import type { Location } from 'react-router-dom'

export const DEFAULT_AUTHENTICATED_ROUTE = '/backups'

const AUTH_ROUTE_PREFIXES = [
  '/auth',
  '/auth-required',
  '/login',
  '/logout',
  '/setup',
] as const
const MAX_RETURN_ROUTE_LENGTH = 4096
const VALIDATION_ORIGIN = 'https://synveil.invalid'

function isAuthRoute(pathname: string): boolean {
  return AUTH_ROUTE_PREFIXES.some(
    (route) => pathname === route || pathname.startsWith(`${route}/`),
  )
}

/**
 * Accept only a short-lived, same-origin application path. The returned value
 * never contains a scheme, authority, credentials, or an authentication route
 * that could recursively grow a `next` chain.
 */
export function sanitizeReturnRoute(
  candidate: unknown,
  fallback = DEFAULT_AUTHENTICATED_ROUTE,
): string {
  if (
    typeof candidate !== 'string' ||
    candidate.length === 0 ||
    candidate.length > MAX_RETURN_ROUTE_LENGTH ||
    !candidate.startsWith('/') ||
    candidate.startsWith('//') ||
    candidate.includes('\\')
  ) {
    return fallback
  }

  let decoded: string
  try {
    decoded = decodeURIComponent(candidate)
  } catch {
    return fallback
  }
  if (decoded.startsWith('//') || decoded.includes('\\')) {
    return fallback
  }

  try {
    const resolved = new URL(candidate, VALIDATION_ORIGIN)
    if (resolved.origin !== VALIDATION_ORIGIN || isAuthRoute(resolved.pathname)) {
      return fallback
    }
    return `${resolved.pathname}${resolved.search}${resolved.hash}`
  } catch {
    return fallback
  }
}

export function returnRouteFromLocation(location: Location): string {
  return sanitizeReturnRoute(
    `${location.pathname}${location.search}${location.hash}`,
  )
}

export function returnRouteFromState(state: unknown): string {
  if (typeof state !== 'object' || state === null || !('returnTo' in state)) {
    return DEFAULT_AUTHENTICATED_ROUTE
  }
  return sanitizeReturnRoute(state.returnTo)
}
