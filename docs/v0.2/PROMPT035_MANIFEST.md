# Prompt035 manifest

- Starting main: `2e4809f77c788f7cebc276cd2479119347366210`.
- Branch: `codex/p035-server-first-admin-bootstrap`.
- Inherited locked markers: `SYNVEIL_SERVER_SETUP_PRODUCT_CONTRACT_LOCKED`,
  `SYNVEIL_SERVER_DEPENDENCY_STRATEGY_LOCKED`,
  `SYNVEIL_MANAGED_SERVER_CONFIGURATION_READY`,
  `SYNVEIL_SERVER_STORAGE_LOCATION_READY`,
  `SYNVEIL_SERVER_SERVICE_INSTALLATION_READY`, and
  `SYNVEIL_SERVER_NETWORK_REACHABILITY_READY`.
- P028 Windows native acceptance remains pending and separate.
- P034 focused hosted workflow: no remote is configured in this checkout, so
  its post-fix hosted conclusion could not be inspected; it is not recorded as
  PASS. Local P034 regression validation remains explicit.
- Existing implementation audited: `AuthenticationService`, `AuthRepository`,
  API/OpenAPI, web setup, metadata transaction and acceptance mapping.
- Authority: singleton PostgreSQL bootstrap state only; no duplicate flag.
- Managed boundary: loopback backend creation; managed LocalOnly/PrivateLan
  edges deny the mutation before proxying. External proxy policy is operator-owned.
- Transaction: `FOR UPDATE`; user, instance-admin authority, Argon2id verifier,
  initial logical Library/root and Closed marker commit or roll back together.
- Unknown response: inspect PostgreSQL; Closed means explicit login with the
  submitted credential, Open permits deliberate same-operation retry, and
  inconsistent state fails closed.
- Persistence: Closed state and associated records survive restart and P033
  data-preserving repair/reinstall/removal boundaries.
- Verification: normal login must succeed and yield an instance-admin
  principal; creation itself never creates a browser session.
- Decision: ADR-065. Validation includes static, auth/API, edge and disposable
  PostgreSQL 17 gates; native evidence remains separately classified.
- P036 remains responsible for the full Host wizard and final readiness.

No password, session credential, database URL, or private key is recorded.
