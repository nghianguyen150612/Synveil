# Server first-admin bootstrap

Status: **Implemented foundation (Prompt035); P036 composition pending**

## Existing authority

Prompt035 hardens rather than replaces the existing authentication stack:
`AuthenticationService` calls `AuthRepository`, whose PostgreSQL transaction
locks the singleton `bootstrap_state` row with `FOR UPDATE`. The transaction
creates one active instance administrator, its Argon2id password verifier, the
initial logical owner Library/root, and the permanent `CLOSED` marker together.
There is no configuration-file, filesystem, service, or edge bootstrap flag.

The UI-neutral `inspect_admin_bootstrap` mapping is deterministic:

| PostgreSQL observation | Product state |
| --- | --- |
| `Open` | `AdminBootstrapRequired` |
| `Closed` | `AdminBootstrapComplete` |
| `Inconsistent` | `AdminBootstrapNeedsRepair` |
| PostgreSQL unavailable | `InfrastructureUnavailable` |

Missing rows, OPEN with existing users, malformed closure metadata, and other
inconsistency fails closed. It requires advanced/operator recovery; setup never
deletes users, resets the row, drops the database, or creates another owner.

## Managed local trust boundary

Personal/Home managed setup is machine-local and invokes the existing bounded strict-JSON endpoint
at `http://127.0.0.1:3000/api/v1/bootstrap/admin`. This is privileged local
setup traffic, not a production client base URL. The backend remains loopback
only, no management listener or public port is added, and PostgreSQL is never
exposed to the desktop wizard.

Every Synveil-managed LocalOnly or PrivateLan HTTPS edge answers the bootstrap
mutation with 404 before `reverse_proxy`. It does not authorize from
`Forwarded`, `X-Forwarded-For`, `X-Real-IP`, `Host`, or
`X-Forwarded-Proto`. Query strings do not alter the Caddy path match; the
canonical and trailing-slash forms are denied, while malformed/double-slash or
encoded paths do not match the Axum mutation route. `GET
/api/v1/system/bootstrap-status` may remain available because it returns only
`setup_required`, request metadata, and `Cache-Control: no-store`.

AdvancedExternalHttps is operator-owned. It retains the implemented HTTP
contract, and its operator is responsible for securing first-run exposure;
Synveil does not claim to configure or enforce that external proxy. Managed
security is not weakened for this compatibility.

## Credential and completion sequence

The request keeps the canonical login, login key, and password concepts and
the existing 16 KiB body bound. `PlaintextPassword`, `PasswordHasherConfig`,
and `StoredPasswordHash` retain password validation and Argon2id PHC hashing.
Only the salted verifier enters PostgreSQL. Passwords and raw session tokens
must not enter configuration, journals, tracing fields, setup state, or replay
evidence; their secret wrappers redact `Debug` output.

A successful creation response contains no session and sets no cookie. P036
must explicitly perform normal authentication with the same submitted
credential and require an active instance-admin principal. This is exposed by
`verify_first_admin_login`; it returns the ordinary secure session boundary,
which uses an opaque high-entropy token, server-side digest, Secure/HttpOnly
cookie presentation, SameSite policy, and CSRF protection.

Completion is therefore: coherent `Closed` state, durable active admin,
credential and initial Library/root, plus successful explicit admin login. A
201/200 response alone is not the final readiness proof.

## Races, unknown outcomes, and lifecycle

Concurrent claims serialize on PostgreSQL. Exactly one transaction commits;
losers receive `BootstrapClosed`, and no check-then-insert occurs outside the
transaction. Transaction errors roll back user, credential, Library/root and
closure together.

If a POST response is lost, inspect authoritative status before doing
anything else. If Closed, never submit a different owner: explicitly log in
with the credential already entered. If Open, the same intended operation may
be deliberately retried. If inconsistent, stop with setup-needs-attention.
There is no fallback administrator.

Bootstrap state is durable SERVER_DATABASE data. API restart, service repair,
software reinstall, storage reconnect, networking changes, restored databases,
new binaries, hostnames, or missing browser/desktop state never reopen it.
P033 data-preserving removal preserves the database, administrator, verifier,
Library/root, and Closed state.

## Readiness and P036 handoff

Before owner creation, P036 must establish PostgreSQL reachability, current
canonical migrations, API health, required storage availability, and coherent
service topology. LAN reachability is not required for this local operation.
P036 consumes only `inspect_admin_bootstrap`, `create_first_admin`, and
`verify_first_admin_login`; it does not query SQL or own `AuthRepository`.

Known limitations: native clean-machine composition remains P036; the focused
PostgreSQL gate requires a disposable PostgreSQL 17 service; production Caddy
artifact qualification and P028 Windows native acceptance remain separate.
No distributed bootstrap rate limiter is added because managed edges do not
expose the mutation; advanced exposure remains operator responsibility.
