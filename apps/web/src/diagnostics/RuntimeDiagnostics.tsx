import { useEffect } from 'react'

import { recordClientRuntimeFailure } from './record'

/** Install passive, message-free global failure listeners for the app lifetime. */
export function RuntimeDiagnostics() {
  useEffect(() => {
    const handleError = () => {
      recordClientRuntimeFailure()
    }
    const handleUnhandledRejection = () => {
      recordClientRuntimeFailure()
    }

    window.addEventListener('error', handleError)
    window.addEventListener('unhandledrejection', handleUnhandledRejection)
    return () => {
      window.removeEventListener('error', handleError)
      window.removeEventListener('unhandledrejection', handleUnhandledRejection)
    }
  }, [])

  return null
}
