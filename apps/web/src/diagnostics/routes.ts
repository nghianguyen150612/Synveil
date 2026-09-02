import type { DiagnosticRouteCategory } from './types'

/** Convert a browser path to a bounded support label without retaining IDs. */
export function routeCategoryFromPath(pathname: string): DiagnosticRouteCategory {
  if (pathname === '/') {
    return 'HOME'
  }
  if (pathname === '/backups') {
    return 'BACKUP_OVERVIEW'
  }
  if (pathname.startsWith('/backups/') && pathname.includes('/restore')) {
    return 'BACKUP_RESTORE'
  }
  if (pathname.startsWith('/backups/') && pathname.includes('/prune')) {
    return 'BACKUP_PRUNE'
  }
  if (pathname.startsWith('/backups/')) {
    return 'BACKUP_DETAIL'
  }
  if (pathname === '/health/dev') {
    return 'DEVELOPMENT_HEALTH'
  }
  if (pathname === '/login' || pathname === '/setup') {
    return 'AUTH'
  }
  return 'NOT_FOUND'
}
