# PROMPT021 — Durable invitation ledger manifest

Status: **PROVISIONAL / ROADMAP RECONCILIATION BLOCKED**.

| Provenance | Exact value |
|---|---|
| Repository | `nghianguyen150612/Synveil` |
| Target | `develop/v0.3` |
| Feature branch | `feat/v03-p021-pairing-invitation-ledger` |
| Exact base | `511de37361941bb55ffe119f63ce94182b4c7230` |
| Base verification | Both feature and target resolved to the specified base; P020 #180 merged |
| P021 implementation source | `019e1a765575194370c65f8946c023b4b5d5fa5a` |
| Draft PR | [#181](https://github.com/nghianguyen150612/Synveil/pull/181), targeting `develop/v0.3` |
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

Local Linux, Rust/Cargo 1.99.1, implementation source above:

| Check | Actual conclusion |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo test -p synveil-pairing-contracts --locked` | PASS: P020 7 unit tests and P021 12 integration tests (including child entrypoint), no failures |
| `cargo test -p synveil-install-engine --locked` | PASS: 681 unit/integration tests, no failures |
| `cargo clippy -p synveil-pairing-contracts -p synveil-install-engine --all-targets --locked -- -D warnings` | PASS |
| `bash scripts/validate-docs.sh` | PASS: 24 validation groups |
| `git diff --check` | PASS |

Additional local checks completed successfully on the same implementation:

| Check | Actual conclusion |
|---|---|
| `cargo test -p synveil-auth -p synveil-api -p synveil-client -p synveil-client-sync --lib --locked` | PASS: auth 16, API 123, client 132, client-sync 298; 569 total, no failures |
| `cargo clippy -p synveil-pairing-contracts -p synveil-auth -p synveil-api -p synveil-client -p synveil-client-sync -p synveil-install-engine -p synveil-server-network --all-targets --locked -- -D warnings` | PASS, no warnings |

Initial local broad-check attempts stopped at missing DBus development metadata;
successful retry used unprivileged extracted Debian libdbus/systemd development
files in a temporary build sysroot. No repository dependencies or host security
policy were changed to work around that environmental limitation. No production
PostgreSQL integration or native mobile/tailnet qualification was performed.

Hosted focused [run 41149252475](https://github.com/nghianguyen150612/Synveil/actions/runs/41149252475),
associated head `ccbf614807205dc64910cf95a0628a70ad1194ed`, was still **QUEUED**
on both Ubuntu and Windows at 2026-10-31 00:23 UTC. Neither job executed or passed
at that observation. That head differs from the implementation SHA only in the
manifest. This final evidence update also changes documentation only; replacement
workflow results must be checked on [draft PR #181](https://github.com/nghianguyen150612/Synveil/pull/181).
Hosted qualification and Windows Clippy repair verification remain **PENDING**,
not accepted. Earlier focused runs 41149003933 and 41149156161 were superseded
and cancelled, and are not counted as qualification. Thirteen stale runs on this
feature branch were cancelled to release capacity; other branches were untouched.
No hosted success is inferred from passing local Linux checks. Green reference
jobs cannot close any independent security or native qualification gates.

## Unresolved decisions

P004-D12/D13 remain pending approval, along with outstanding P004/P028/P036
gates, independent security review, native Android/iOS pairing qualification,
real-tailnet qualification and authoritative roadmap reconciliation. The source
roadmap remains unavailable. The approval fixture is not an approved proof
mechanism, TLS ceremony or production authority. SQLite reference tests do not
prove production enrollment security or resistance to physical power loss.
No automatic merge or P022 work is authorized or included.
