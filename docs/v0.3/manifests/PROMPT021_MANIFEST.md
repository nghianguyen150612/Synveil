# PROMPT021 — Durable invitation ledger manifest

Status: **PROVISIONAL / ROADMAP RECONCILIATION BLOCKED**.

| Provenance | Exact value |
|---|---|
| Repository | `nghianguyen150612/Synveil` |
| Target | `develop/v0.3` |
| Feature branch | `feat/v03-p021-pairing-invitation-ledger` |
| Exact base | `511de37361941bb55ffe119f63ce94182b4c7230` |
| Base verification | Both feature and target resolved to the specified base; P020 #180 merged |
| Separate P019 Windows Clippy repair | `a117c90d4b713c7cec7519c7b3e978621a05cd3f` |

## Scope and audit

See [P021_INVITATION_LEDGER.md](../security/P021_INVITATION_LEDGER.md) for the
P020/sve1_/P004/authorization/concurrency audit, reference trust boundary, schema,
transaction, recovery and security limitations. The adapter and schema are
integration-test-only under `crates/pairing-contracts/tests/`; production pairing
contracts and runtime dependencies are unchanged. Dev dependencies and lockfile
registration support isolated file-backed SQLite tests. Focused CI is
`.github/workflows/v03-p021-invitation-ledger.yml`.

The P019 repair is a separate commit: platform-specific imports and fields are
compiled only where consumed, unused placeholder types are removed and the
unsupported path explicitly discards its input path. It preserves unsupported
platform denial and does not implement Windows last-use authority. Evidence:
[historical Windows job 124054638678](https://github.com/nghianguyen150612/Synveil/actions/runs/41053355548/job/124054638678)
failed strict Clippy on unused OpenOptionsExt/Digest/Sha256, STREAM_CHUNK_BYTES and
private platform-only fields. No warning suppression or validation weakening.

## Validation evidence

Local and hosted results, exact implementation SHA and draft PR link will be
recorded after qualification. Pending checks are not passed checks.

## Unresolved decisions

P004-D12/D13 remain pending approval, along with outstanding P004/P028/P036
gates, independent security review, native Android/iOS pairing qualification,
real-tailnet qualification and authoritative roadmap reconciliation. The source
roadmap remains unavailable. The approval fixture is not an approved proof
mechanism, TLS ceremony or production authority. SQLite reference tests do not
prove production enrollment security or resistance to physical power loss.
No automatic merge or P022 work is authorized or included.
