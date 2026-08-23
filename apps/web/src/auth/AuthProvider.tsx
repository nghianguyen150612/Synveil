import {
  AuthContext,
  placeholderAuthState,
  type AuthProviderProps,
} from './auth-context'

/** Placeholder boundary; login and session behavior are intentionally absent. */
export function AuthProvider({ children }: AuthProviderProps) {
  return (
    <AuthContext.Provider value={placeholderAuthState}>
      {children}
    </AuthContext.Provider>
  )
}
