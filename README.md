# Synveil

> **Your data. Your devices. Your cloud.**

Synveil v0.1 is a self-hosted, PostgreSQL-backed file and library platform
with a native two-process desktop client. `synveil-client` owns synchronization
and durable local state; `synveil-desktop` is the Qt UI/tray/control surface.

## v0.1 support

- **Server:** Linux deployment with PostgreSQL, a durable object root, and
  operator-managed HTTPS termination and service supervision.
- **Desktop:** Linux x86_64 DEB/RPM packages and Windows x86_64 portable ZIP.
- **Deferred:** macOS, iOS, Android, and Synveil OS are not supported in v0.1.
- **Validation:** Windows cross-build/compile evidence is not native Windows
  runtime acceptance; see the operations guide for the distinction.

## Start here

1. Read the [v0.1 operations guide](docs/en/RELEASE_OPERATIONS.md).
2. Review [release notes](docs/en/RELEASE_NOTES_v0.1.md).
3. Follow [package and install policy](docs/en/RELEASE_PACKAGING.md).
4. Review [upgrade safety](docs/en/UPGRADE_SAFETY.md) before replacing a
   package or database-connected runtime.
5. Review the [security guide](docs/en/SECURITY.md) for trust and credential
   assumptions.

Vietnamese documentation is available under [`docs/vi`](docs/vi/PRODUCT.md).
The complete documentation index is [`docs/README.md`](docs/README.md).

## Workspace components

| Component | Role |
|---|---|
| `synveil-api` | PostgreSQL-backed API/router and server binaries. |
| `synveil-client` | Background sync process, local SQLite state, recovery, and IPC. |
| `synveil-desktop` | Native Qt/QML desktop UI and tray/control surface. |
| `synveil-worker` | Internal bounded maintenance/GC worker with no listener. |

The desktop package does not bundle PostgreSQL or a guided server installer.
Do not treat the architecture blueprint or historical Prompt reports as user
manuals; the release-facing guides above are the operational source of truth.

## Development checks

```sh
./scripts/validate-docs.sh
cargo fmt --all -- --check
```

See the architecture contribution guide at
[`docs/en/CONTRIBUTING_ARCHITECTURE.md`](docs/en/CONTRIBUTING_ARCHITECTURE.md).
