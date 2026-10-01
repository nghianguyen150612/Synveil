# Prompt017 evidence manifest — verified Linux quick install

Status: **source and hosted gates defined; final-head hosted execution required**.

## Scope and invariants

The canonical shell frontend delegates to the standard-library Python
orchestrator. The latter imports P011 `release_channel.py` and P006
`release_download.py`; it does not create a weaker downloader. P017 adds no
PostgreSQL/SQLite migration, sync protocol, server bootstrap, product tag,
automatic P018 resolver, P019 launch UX, or P020 broad qualification.

P015/P016 review found no new source regression in the inherited tree. The
AppImage workflow retains Ubuntu 22.04, Qt 6.7.3, independent desktop/AppImage
reproducibility, closure inspection, QML smoke and serialized runtime lifecycle.
The required authoritative closure remains its final-head hosted result; this
manifest does not turn local/static evidence into a hosted claim.

## Test mapping

| IDs | Evidence |
| --- | --- |
| QUICK-INSTALL-1 | self-trust section rejects `curl | sh` and requires independent bundle authentication |
| 2–4 | P011 pinned channel/minimum generation/fresh selection plus exact selected-manifest binding |
| 5–8 | explicit profile maps to exact P006 tuple; P006 zero/multiple/size/hash tests |
| 9 | tampered-stage unit proves no manager call; all acquisition occurs before manager construction/mutation |
| 10–12 | P006 HTTPS/origin/redirect and private fsync/atomic no-clobber staging regressions |
| 13–16 | plan and explicit consent precede native APT/DNF; static test excludes hidden password handling |
| 17–18 | Ubuntu APT and pinned Fedora 42 DNF hosted quick-install jobs verify native database and payload |
| 19–20 | hosted jobs assert no process, autostart, system unit or silently enabled user unit |
| 21 | unit and both hosted paths require `ALREADY_INSTALLED_VERIFIED` without second transaction |
| 22–23 | typed busy/unknown handling never deletes locks or blindly replays |
| 24 | existing native removal jobs verify admin, application, user and external durable state |
| 25–26 | closed explicit enum and unit rejection; no automatic profile option exists |
| 27–28 | existing reproducible DEB/RPM validators and native hosted jobs remain required |
| 29 | unchanged P015/P016 AppImage workflow remains a required final-head gate |

Adversarial coverage is composed rather than duplicated: P006 tests wrong
manifest/artifact digest, truncation, extra bytes, wrong tuple, ambiguity,
untrusted redirect, HTTP downgrade and destination conflict; P011 tests absent
authentication, rollback/equivocation and selected-manifest binding; P017 tests
the last-use staged-file tamper boundary, consent/order, idempotence, installed
verification, explicit profile and root boundary. Every such trust failure
occurs before `NativeManager.install`.

## CI and validation commands

```text
./scripts/validate-linux-quick-install.sh
./scripts/validate-release-download.sh
./scripts/validate-release-channel.sh
./scripts/validate-debian-package-ux.sh
./scripts/validate-rpm-package-ux.sh
./scripts/validate-appimage-build.sh
./scripts/validate-appimage-runtime.sh
./scripts/validate-release-manifest.sh
./scripts/validate-docs.sh
cargo fmt --all -- --check
cargo check/test/clippy -p synveil-install-engine --locked
git diff --check
```

The hosted native fixture uses genuine loopback HTTPS with an isolated synthetic
CA. Production code retains ordinary TLS verification and has no HTTP/file/TLS-
disable switch. Final-head hosted results, hashes and workflow URLs must be
recorded in the PR; they cannot be truthfully predeclared here.
