# Prompt004 manifest — clean-machine acceptance harness

Status: **ACCEPTANCE_CONTRACT_DEFINED; native execution pending.**

## Starting state and scope

- Starting branch: `main`
- Starting HEAD, `origin/main`, and live `origin/main`: `c03860aa9878dc8feaf719231fd286a0e52cb18b`
- Product version: `0.1.0`; server migrations: `36`; client migrations: `7`; `LOCAL_SCHEMA_VERSION`: `7`.
- Schema version: `1`, JSON; no product schema or dependency changes.
- Prompt-owned paths: this manifest, `CLEAN_MACHINE_ACCEPTANCE.md`, roadmap and docs index updates, scenario and result schemas with JSON inventory, Python standard-library validator/tests, shell validation entrypoint, and validate-only CI integration.

## Scenario inventory and ownership

The ten P001 IDs are defined exactly once: `INSTALL-JOURNEY-1` through
`INSTALL-JOURNEY-8`, `FIRST-RUN-1`, and `FIRST-RUN-2`. `UPGRADE-TEMPLATE-1`
adds machine-readable prior-installation source metadata. Run
`python3 scripts/install_acceptance.py inventory` for titles, platform,
minimum evidence, implementation/acceptance owners and current availability.
All scenarios currently declare `IMPLEMENTATION_PENDING`; no native install,
upgrade, repair, uninstall or first-run result is claimed.

## Validation and evidence

The intended local gate is `./scripts/validate-install-acceptance.sh`; it runs
contract validation and 21 focused standard-library self-tests. CI runs only
this static/contract check and does not claim clean-machine acceptance. The
cross-document audit confirmed: P001/ADR-049 target Windows Setup, DEB, RPM,
AppImage and alternate verified Linux installer while P002 still labels absent
v0.2 implementation honestly; installer completion ends at verified payload,
integration and launch while Connect/Host, authentication and sync remain
separate; `synveil-client` owns sync and OS SecretStore owns credentials;
ordinary uninstall preserves durable user/server data and startup remains
opt-in; ambiguous effects reconcile before retry and unknown schema fails
closed; Host belongs to a separate bootstrap flow with unresolved database and
service decisions left open; Windows prefers per-user install, DEB/RPM retain
visible native package-manager authority, and AppImage remains user-level.
P001 journey IDs and P012–P047 implementation/acceptance owners remain assigned
as in the authoritative roadmap and ADR-050. Exact OS versions remain pending
qualification. `INS-10` advances from P003's `ACCEPTANCE_PENDING` to
`ACCEPTANCE_CONTRACT_DEFINED`; native evidence remains pending under
P020/P028/P036/P045/P047.

Commands run for this gate:

| Command/check | Result |
| --- | --- |
| `./scripts/validate-install-acceptance.sh` | PASS; 11 definitions and 21 self-tests; native execution not performed. |
| `python3 -m py_compile scripts/install_acceptance.py` | PASS. |
| Parse both JSON schemas; validate `run` and `inventory` output as JSON | PASS. |
| `bash -n scripts/validate-install-acceptance.sh` | PASS. |
| Parse `.github/workflows/ci.yml` with PyYAML | PASS; validate-only job step. |
| `./scripts/validate-docs.sh` | PASS; DOC-UNIT-1 through DOC-UNIT-7. |
| `cargo fmt --all -- --check` | PASS. |
| `git diff --check` | PASS. |
| Safety scan for secrets and arbitrary scenario commands | PASS; definitions have typed operations only and contain no credentials. |

Phase A checkpoint conditions are P001–P004 contracts present and consistent,
P004 harness valid, and authoritative roadmap ownership unchanged. The
checkpoint does not mean installers work. P005 remains the unified release
artifact manifest; P048 remains the only v0.2.0 release/tag gate.

## Git completion policy

The requested single focused commit and `origin/main` push occur only after
validation, exact-path staging, staged-diff review, and remote verification.
Commit SHA and push results are recorded in the completion report after they
exist; no future SHA is asserted here.
