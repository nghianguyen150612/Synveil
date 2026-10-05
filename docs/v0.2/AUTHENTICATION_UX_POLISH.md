# Authentication UX polish

Prompt039 presents the existing desktop device-enrollment boundary in product
language. The user signs in to a device with a **one-time device code**; the
code is not an account password and cannot be reused.

## Ownership and protocol

The protocol remains `POST /api/v1/device-enrollment/exchange`. Its ownership
path is unchanged:

`QML transient input → DesktopUiBridge → DesktopController → local IPC →
synveil-client → DesktopSyncHostHandle → HttpEnrollmentClient → enrollment
exchange → LocalStateStore + SecretStore → CredentialChanged → runtime reload`.

QML has no HTTP client or credential-store ownership. It collects a masked,
sensitive value of at most 69 encoded bytes, copies it for dispatch, and clears
the visible field immediately. It does not place the code in properties,
snapshots, events, settings, logs, diagnostics, tray text, or profile metadata.
The domain `EnrollmentSecret` parser remains final validation authority.

## Presentation states and results

The Rust presentation boundary distinguishes sign-in required, signing in,
authenticated, checking/reconciling, invalid code, network unavailable, server
unavailable, rate limited, secure-storage unavailable, protocol problem,
signing out, and sign-out attention. Canonical results remain `Authenticated`,
`SignedOut`, `InvalidCredentials`, `NetworkUnavailable`, `ServerUnavailable`,
`RateLimited`, `SecureStoreUnavailable`, `Busy`, `ProtocolError`, and
`OutcomeUnknown`. They map only to bounded product copy; rejection never
reveals whether a code was unknown, expired, malformed, or already used.

Success is durable-first: receipt validation, profile metadata and secret
promotion, readback, `CredentialChanged`, and runtime reload precede an
authenticated status and success UI. The refreshed process state—not a QML
boolean—is authoritative after restart.

## Unknown outcomes and restart

`OutcomeUnknown` clears the input, disables immediate submission, and requests
canonical state reconciliation. Synveil never automatically replays a one-time
exchange. A durable credential advances the UI; otherwise the user must obtain
and deliberately enter a new code. GUI or client restart reconstructs only safe
authentication status from profile metadata plus `SecretStore`; no credential
returns to QML and no QML transaction journal exists.

## Sign out

Sign out is explicit and confirmed. It is never coupled to window close, tray
close, Quit, restart, or shutdown. The confirmation explains that the saved
device credential is removed and a new device code will be required. Durable
forgotten state, verified secret cleanup, `CredentialChanged`, and runtime
reload precede `SignedOut`. Failure or an unknown response cannot claim success
or trigger an automatic replay.

Signing out preserves the configured server profile, address, libraries, local
root bindings, cache, and pending safe state. Origin-generation fencing from
ADR-043 remains authoritative: a credential for origin A cannot authenticate
origin B, and stale results cannot overwrite a newer connection generation.

## First-run routing and handoffs

A configured but signed-out profile bypasses Welcome and sees the focused
sign-in surface. Authentication is never automatic after connection setup.
After authentication, existing libraries open normally; zero libraries expose
the existing `library_setup_required` boundary. P040 owns first-library setup,
P041 owns overall installation-to-sync progress, and P042 owns broad repair and
reconnect UX.

## Known limitations

Prompt039 adds no password login, OAuth/OIDC, MFA, browser callback, QR pairing,
clipboard management, code reveal, or auth-specific TLS bypass. Live enrollment
evidence still depends on disposable server infrastructure and native platform
qualification remains platform-specific.
