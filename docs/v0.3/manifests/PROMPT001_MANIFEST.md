# PROMPT001 — v0.2 release and repository baseline manifest

## Prompt identity and scope

| Field | Value |
|---|---|
| Prompt | `SYNVEIL v0.3 — PROMPT001` |
| Phase | A — Baseline, Architecture & Security Requirements |
| Objective | Evidence-backed, immutable v0.2/repository reference baseline |
| Evidence date | 2026-10-23 UTC |
| Roadmap | `Synveil-v0.3-Context-Roadmap-P001-P060.md` was not available in supplied context or anywhere under `/workspace`; requirements were limited to this prompt. |
| Scope | Read-only repository/release audit, documentation, and a non-production audit-artifact validator. No v0.3 behavior, production source, migrations, databases, release artifacts, or integrations were changed. |

## Baseline SHAs and Git state

| Ref or state | Exact evidence |
|---|---|
| Repository / origin | `https://github.com/nghianguyen150612/Synveil.git` |
| Initial local branch | `work`, clean working tree; `git status --porcelain` empty |
| `origin/main` at preflight | `604499fce0dfd134d82da3d5bb5d4a055c5eea8d` |
| `origin/develop/v0.3` at preflight | `604499fce0dfd134d82da3d5bb5d4a055c5eea8d` |
| Merge base / divergence | common ancestor is the same SHA; `git rev-list --left-right --count origin/develop/v0.3...origin/main` returned `0 0` |
| Candidate-source commit | `deef2db375764d3157afb5371eb963bbd9b3f1ea`; six later changes through baseline HEAD were release/readiness documentation only |
| Working branch | `audit/v03-p001-baseline`, created from verified `origin/develop/v0.3` after the clean-tree check |
| Initial audit commit / PR opening head | `ecb1c4da408e1db86fc77a4ee924aef56f053e59` |
| Target branch | `develop/v0.3`; never `main` |

The local `origin` fetch refspec tracked `main` only, so `develop/v0.3` was explicitly fetched to `refs/remotes/origin/develop/v0.3`. `git ls-remote --heads origin main develop/v0.3 audit/v03-p001-baseline` verified the two base refs and no pre-existing audit branch. No reset, force-push or rebase was used.

## Files changed

Only these four scoped audit files are intended to differ from the base:

1. `docs/v0.3/audits/P001_V02_BASELINE_AUDIT.md`
2. `docs/v0.3/audits/P001_COMPATIBILITY_BASELINE.md`
3. `docs/v0.3/manifests/PROMPT001_MANIFEST.md`
4. `scripts/validate_p001_audit.py`

The validator checks mandatory audit sections, local Markdown evidence links, full commit SHAs, GitHub Actions run URLs, and that the base-to-worktree diff contains exactly these four files. It does not access product state or databases.

## Evidence inventory

| Claim area | Primary evidence |
|---|---|
| Branch and release metadata | Remote refs and tags; GitHub [releases](https://api.github.com/repos/nghianguyen150612/Synveil/releases), [branches](https://api.github.com/repos/nghianguyen150612/Synveil/branches), and PR listing; repository history at base SHA above. Releases and open PRs were empty at the collection point; only `v0.1.0` was tagged. |
| R01–R05 integration and qualification | PRs #148–#155 and commits documented in `docs/v0.2/RELEASE_HARDENING_R05_MANIFEST.md`, `RELEASE_CANDIDATE_QUALIFICATION_R05.md`, and `RELEASE_FINAL_ARTIFACT_MATRIX.md`; exact-source candidate [run 39373799679](https://github.com/nghianguyen150612/Synveil/actions/runs/39373799679). |
| Candidate artifact identity | Candidate archive ID `12245434492`, 166,869,997 bytes, SHA-256 `1d0fe6d38d8d4362fc4306aa400bc98f436939f00f32b82a0177534671f9d5e9`, expiring 2026-11-03; assembled product manifest SHA-256 `e3a5755b4b036105a2b9f8baa5ffd9348656bcc8aa2b07b38e11247afa3eb2c9`. Per-platform unsigned artifact hashes are recorded in the baseline report. |
| Reproduction / platform acceptance | Windows exact-source runs [39377172994](https://github.com/nghianguyen150612/Synveil/actions/runs/39377172994) and [39377177309](https://github.com/nghianguyen150612/Synveil/actions/runs/39377177309); Linux packages [39377178170](https://github.com/nghianguyen150612/Synveil/actions/runs/39377178170); PostgreSQL 17 [39377173109](https://github.com/nghianguyen150612/Synveil/actions/runs/39377173109); AppImage [39373764291](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764291); iOS runs [39373790719](https://github.com/nghianguyen150612/Synveil/actions/runs/39373790719), [39373790899](https://github.com/nghianguyen150612/Synveil/actions/runs/39373790899), [39373790837](https://github.com/nghianguyen150612/Synveil/actions/runs/39373790837), [39373790728](https://github.com/nghianguyen150612/Synveil/actions/runs/39373790728); Windows native gate [39373764284](https://github.com/nghianguyen150612/Synveil/actions/runs/39373764284); managed Host/P036 [39373791245](https://github.com/nghianguyen150612/Synveil/actions/runs/39373791245). |
| Current CI/security snapshot | Base commit checks [run 39885328513](https://github.com/nghianguyen150612/Synveil/actions/runs/39885328513), queried through GitHub Checks API on 2026-10-23. Dependency policy failed for `rustls-webpki 0.103.15` (through `rustls 0.23.46` / `reqwest 0.12.28`) with RUSTSEC-2026-0381 and RUSTSEC-2026-0382; latest supported version indicated by the scan is `0.103.16`. Other listed jobs were still in progress at query time. |
| Architecture and contracts | Root `Cargo.toml`; `crates/api/src/router.rs`, `api/openapi.yaml`, auth/session/device-auth modules, metadata/storage/sync/install/platform crates; `apps/web`, `clients/android`, `clients/ios`; source and configuration inventories in both audit reports. |
| PostgreSQL and client persistence | `migrations/README.md` plus all 36 timestamped SQL migrations; `crates/client-sync/migrations` and schema version; Android Room/DataStore; iOS SQLite/Keychain sources. Fresh PostgreSQL 17 migration/live suite evidence is run 39377173109. No real user database was accessed. |
| Installation and security | `deploy/install`, `deploy/packages`, `deploy/server`, `deploy/release`, `crates/install-engine`, `crates/server-host`, `crates/auth`, `crates/api`, and client credential-store sources; R05 P045/P036 runs and policies cited in the baseline report. |

## Validation commands and outcomes

Local environment: Linux workspace, Python 3.12.14, Node/npm available, Java available; `cargo`, `rustc`, `xcodebuild`, `psql` were not installed. `apps/web/node_modules` was absent. No local Android SDK was established. GitHub runs cited above are recorded as remote evidence and are not represented as local executions.

| Command | Result | Evidence / limits |
|---|---|---|
| `git diff --cached --check` | PASS | Final staged audit diff has no whitespace errors. |
| `./scripts/validate-docs.sh` | PASS on rerun | The initial run occurred before the manifest existed and failed on its three expected local references. After manifest creation, all 24 documentation validation units passed. |
| `bash scripts/validate-install-acceptance.sh` | PASS | Contract validation plus 21 Python self-tests; reports native execution was not performed. |
| `bash scripts/validate-release-manifest.sh` | PASS | 36 tests. |
| `bash scripts/validate-release-download.sh` | PASS | 79 download tests, followed by 36 manifest tests. |
| `bash scripts/validate-release-channel.sh` | PASS | 101 tests. |
| `python3 scripts/validate-release-version.py --expected-version 0.2.0 --source-commit 604499fce0dfd134d82da3d5bb5d4a055c5eea8d` | PASS | JSON result reports product version 0.2.0 and 18 workspace crates. |
| `cargo fmt --all -- --check` | BLOCKED | `cargo: command not found`; no local Rust formatter/tests were claimed. |
| `cargo test --workspace --locked` | BLOCKED | `cargo: command not found`; no local Rust tests were claimed. |
| `npm --prefix apps/web test` | BLOCKED | Script starts `vitest run`, but `vitest` is unavailable because `apps/web/node_modules` is absent. Lint/typecheck/build were not run locally for the same reason; remote Web quality passed in exact-source Rust CI run 39377177309. |
| Android Gradle suite | NOT_RUN | No local SDK/runtime qualification was established; no Android native or device result is claimed. |
| Xcode/iOS native suite | NOT_RUN | `xcodebuild` is unavailable on this Linux environment; remote static/build/Simulator runs are cited separately. |
| `python3 scripts/validate_p001_audit.py --base origin/develop/v0.3 --allow-pending-pr` | PASS after validator correction | Required headings, local evidence links, full SHAs, GitHub run URLs, and exact four-file diff scope passed. An intermediate staged-tree run exposed that the validator omitted index entries; it now includes committed, staged, unstaged, and untracked paths. |

No production binary, migration, installer integration, real database, or user data was changed. Focused audit validation only reads the Git diff and Markdown artifacts.

## Verified findings

* At preflight `main` and `develop/v0.3` both pointed at `604499fce0dfd134d82da3d5bb5d4a055c5eea8d`; ancestry and divergence were consistent and zero.
* Official v0.2.0 release status is `BLOCKED`: product source is version 0.2.0 and exact-source unsigned candidate artifacts exist, but there is no v0.2.0 tag, GitHub Release, production signing, activated stable channel, complete P047 qualification or P048 approval.
* R05 candidate bytes were built from `deef2db375764d3157afb5371eb963bbd9b3f1ea`. Later code at the recorded baseline is unchanged, but documentation changed; the candidate is not silently promoted or treated as a release for the baseline SHA.
* The workspace contains 18 Rust crates, API v1, a React web client, desktop Rust/Qt client, Kotlin Android app and SwiftUI iOS app. OpenAPI metadata still labels the contract `0.1.0-blueprint` while product source says 0.2.0.
* PostgreSQL uses 36 forward SQLx migrations; desktop SQLite has 8 migrations/schema version 8; Android Room is schema 4; iOS mutation SQLite is schema 9 and photo observation SQLite schema 3. No downgrade guarantee or end-to-end PostgreSQL-plus-object restore rehearsal was found.
* Linux/Windows packaging and first-admin bootstrap have implementation; the future three-step installer wizard is absent. macOS desktop packaging is not part of documented v0.2 support.
* Existing authentication separates browser sessions from device credentials, stores server-side credential digests, and applies owner/device resource scopes. General RBAC, protected areas, approval workflow, passkey/FIDO2, SynveilNet/Tailscale integration, CLI, network mount and protected push were not found.
* Baseline dependency-policy check is a security blocker: two current rustls-webpki advisories are reported by check run 39885328513.

## Risks, blockers and unresolved questions

1. Do not designate or publish v0.2 as stable until signing, P036 inventory/signature, P045 native qualification, P047/P048 approval, stable distribution and supported upgrade/recovery gates are satisfied.
2. Resolve RUSTSEC-2026-0381 and RUSTSEC-2026-0382 and obtain a passing dependency-policy check before merging this PR if that check is required by the target branch/repository policy.
3. The authoritative P001–P060 roadmap was unavailable; later phases must verify and supply it rather than infer its requirements from this audit.
4. The supported v0.2 client/server version-skew policy, web browser matrix, live Android/iOS cross-version acceptance and signed mobile artifacts remain unknown or blocked.
5. No database downgrade, supported v0.1-to-v0.2 upgrade, power-loss recovery, or combined PostgreSQL/object-store restore evidence was found.
6. GitHub Actions candidate artifacts have finite retention and are not a trusted public distribution channel.

## PR URL and head SHA

| Field | Value |
|---|---|
| PR | [#156 — docs(v0.3): establish P001 release and repository baseline](https://github.com/nghianguyen150612/Synveil/pull/156) |
| Target | `develop/v0.3` at `604499fce0dfd134d82da3d5bb5d4a055c5eea8d` when the PR was opened; reverified by `git ls-remote` |
| Initial PR head SHA | `ecb1c4da408e1db86fc77a4ee924aef56f053e59`, observed at PR creation on 2026-10-23 02:04 UTC |
| State | OPEN; not merged |
| Checks at opening | PR workflows were queued/in progress. Base CI run 39885328513 still contained the failed dependency-policy check; PR checks must be rechecked before any merge. |

This manifest follow-up updates PR metadata and therefore advances the live PR head beyond its initial SHA. The final branch SHA is stated in the completion report and can be verified from the PR.

## Final audit gate

**PASS — evidence-backed P001 audit completed; review and PR CI remain open.** The exact repository baseline, v0.2 release disposition, architecture, installation, migrations, compatibility and security boundaries are documented in the two reports; the four-file scope and local evidence links passed the audit validator; available repository checks were run and their blocked/unrun limits are recorded. No production code, database, migration or integration changed. This result does not certify v0.2 as stable.

Before accepting the PR for merge, review its four-file diff, wait for applicable PR checks, and resolve the failed dependency-policy gate for RUSTSEC-2026-0381/RUSTSEC-2026-0382. For P001 acceptance, keep the release blockers, unavailable roadmap and unverified client/platform combinations explicit; an unresolved v0.2 release is compatible with passing the audit. Before starting P002, supply or locate the authoritative P001–P060 roadmap and review its P002 scope, then begin only under a separate instruction. No P002 work is included here.
