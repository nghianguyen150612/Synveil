# Prompt020 Manifest — Linux Clean-Machine Acceptance

## Outcome

Prompt020 delivered the Linux clean-machine acceptance *harness*, the hosted
workflow, and an honest classification of the residual P015–P019 CI failures.

**Prompt020 did not close Phase C.** Native acceptance evidence does not exist.
No readiness token is emitted and the PR is not merged.

## Baseline

| Item | Value |
| --- | --- |
| Prompt019 merge base | `1bd1e898a4dd4639bf342e5d097e7fa58628365d` |
| Starting `origin/main` verified on 2026-10-02 | `19abe60a7154e03d1db94f7f3e304d3065a99733` |
| Starting main parent | `02602382c0f372915b5037d40b992b4a464d61f2` |
| Unrelated iOS work preserved | `19abe60` |

The stated baseline commit was verified through `gh pr view 22` and matches the
real squash merge.

**History correction (verified against live GitHub before this implementation).** An
earlier revision of this manifest claimed that `origin/main` had advanced to a
*root commit with no parents* and that the pre-existing history was no longer
reachable from any ref. That claim was wrong. Live GitHub shows that
`179fd5f0c6bfd098cb10f29f38aea10a33711a2b` has parent
`1bd1e898a4dd4639bf342e5d097e7fa58628365d`, and that the annotated tag `v0.1.0`
still targets `fa23232ff0154f627ebdd221ec5435134f177af0`. The history is intact
and not flattened. No history repair was performed, and none is needed.

The practical consequence is limited but real: `UPGRADE-TEMPLATE-1` names
`fa23232ff0154f627ebdd221ec5435134f177af0` as its v0.1.0 source, and that commit
is reachable through the `v0.1.0` tag rather than only as a dangling object.

## Residual P015–P019 findings

Every run on the Prompt019 head failed. Classified from the real logs:

| Workflow | Observed failure | Class |
| --- | --- | --- |
| Linux AppImage | `private or temporary build path found in synveil-desktop` | LINUX_INSTALL_REGRESSION |
| Linux native packages | `synveil-desktop build-b differs` | LINUX_INSTALL_REGRESSION |
| Rust CI (windows/macOS) | `cannot find unix in os`, `nix::unistd`, `Permissions::mode` | LINUX_INSTALL_REGRESSION |
| Rust CI (ubuntu) | `systemd-tmpfiles: unrecognized option '--dry-run'` | ENVIRONMENT_LIMITATION |
| PostgreSQL 17 | `column d.daticulocale does not exist` | PRE_EXISTING_UNRELATED_FAILURE |

The `sysusers_tmpfiles_artifacts_match_authoritative_sources` failure was
reproduced locally and confirmed to be a systemd version limitation (249 vs the
required 250), not a product defect. It was deliberately **not** "fixed" by
changing product code.

## Delivered

### Follow-up round (this change)

* **`synveil-install-engine` builds on Windows and macOS.** The AppImage
  module and its re-export are gated to `target_os = "linux"`, and the
  Unix-only `nix` dependency moved under a Linux target table. The helper
  binary keeps its `[[bin]]` declaration, because Cargo cannot express a
  target-conditional binary and `build-appimage.sh` builds it by name on
  Linux; its body is gated instead, and on a non-Linux target it performs no
  integration and exits non-zero. No fake non-Linux AppImage behaviour is
  introduced: the API is genuinely absent there, which was verified by
  compiling a probe that references it on each target.
* **systemd 249 compatibility, without weakening any product semantics.**
  `--dry-run` (and `--graceful`) do not exist before systemd 250. The shipped
  `deploy/sysusers.d/synveil.conf` and `deploy/tmpfiles.d/synveil.conf` are
  unchanged. Validation now detects the capability per tool and otherwise
  performs a real `--root`-confined run seeded with a minimal account
  database. That path is *stronger* than `--dry-run`: it parses the fragment,
  creates the directory and applies the declared mode inside the isolated
  root. It is proven non-vacuous by a test that feeds an invalid tmpfiles
  mode and an unknown sysusers command type and requires both to be rejected,
  and it proves isolation by digesting `/etc/passwd`, `/etc/group`,
  `/etc/shadow`, `/etc/gshadow`, `/var/lib/synveil` and `/run/synveil` before
  and after every run. A new `systemd-249-compat` hosted job runs this on
  `ubuntu-22.04`, where `--dry-run` is genuinely absent, so the 249 path is
  exercised rather than assumed.
* **Qt/CXX-Qt root cause fixed in the reproducible Qt tooling.** The leak is
  `qt-build-utils` passing the *canonicalized absolute* QML source path to
  `qmlcachegen`, which records it in the generated C++ as the compiled unit's
  source file. No remap flag can reach a string literal a generator already
  wrote. The existing `qmake`/`rcc` wrapper now also intercepts
  `qmlcachegen` — it is resolved through the same `qmake -query` tool
  directories the wrapper already redirects, so no vendored crate is patched
  — and gives the generator a byte-identical copy of each QML source under a
  fixed, checkout-independent prefix (`/usr/src/synveil`, matching the Rust/C++
  remap prefix already in force, and explicitly *not* matched by the
  private-path scan). Checkout sources are never modified. Verified locally
  with a stub generator: two different checkout roots produce byte-identical
  generated C++, and out-of-root and non-QML arguments pass through untouched.
* **Differential reproducibility diagnostics.** Hosted CI is the authoritative
  environment for Qt validation, since the desktop cannot be built in a
  sandbox without Qt 6. `scripts/diagnose-desktop-reproducibility.sh`
  compares build A against build B across the generated qmlcachegen/qmldir/rcc
  inputs, the linked ELF section layouts, the first differing byte with
  surrounding text, and any surviving private path marker. It runs
  unconditionally in the new `desktop-qt-reproducibility` job — a *green*
  gate can still hide a checkout-dependent generated input, and a
  mismatch-only diagnosis would miss exactly that case.
* **Hosted CI/VM structure completed, truthfully.** The native matrix now
  enters through the real `linux-acceptance-vm.sh fetch-image` control plane,
  reading both the URL and the digest from `deploy/acceptance/images.lock`,
  and fails closed with a specific reason while both are unpinned. No digest,
  no GUI evidence and no native VM PASS was invented.

### Original round

* **Artifact diagnostics.** `synveil_assert_no_private_paths` now reports the
  full leaking path. Previously every leak printed an identical bare `/home/`,
  which named no cause and could not be acted on. Gate strength is unchanged:
  the detection pattern was not weakened.
* **Toolchain remap.** `RUSTUP_HOME` is now remapped, closing a real gap in the
  path policy on runners that install rustup under the account home.
* **`scripts/linux_acceptance.py`.** The Linux execution layer: host fact
  collection, exact artifact binding with fail-closed digest/size verification,
  manifest identity capture, a clean-machine probe, a closed typed action
  dispatch, capability resolution, redaction, result-v1 emission and result
  validation. 15 self-tests.
* **`scripts/linux-acceptance-vm.sh`.** VM control plane with pinned-image
  trust, bounded waits, snapshot and real power-cut support.
* **`deploy/acceptance/images.lock`.** Image trust anchor. Digests are
  deliberately unpinned because no reviewer has authenticated them; the driver
  refuses to download an unpinned image.
* **`.github/workflows/linux-clean-machine.yml`.** Dedicated P020 workflow kept
  out of ordinary Rust CI, whose `evidence-gate` job fails rather than skipping
  when native evidence is absent.
* **Scenario metadata.** P020-owned scenarios now name Ubuntu 24.04 and Fedora
  42 x86_64 with real package-manager and handler facts. Debian is recorded as
  detected-not-qualified. `availability` remains `IMPLEMENTATION_PENDING`
  everywhere, because no native evidence exists.

## Verified locally

```
cargo fmt --all -- --check
cargo clippy -p synveil-install-engine -p synveil-metadata --all-targets --locked
cargo check -p synveil-install-engine --locked --all-targets
cargo check -p synveil-install-engine --locked --all-targets --target x86_64-pc-windows-msvc
cargo check -p synveil-install-engine --locked --all-targets --target x86_64-apple-darwin
cargo test -p synveil-install-engine --locked
cargo test -p synveil-metadata --locked --test systemd_isolated_fragments
cargo test -p synveil-metadata --locked --test linux_service_identity_units
cargo test -p synveil-metadata --locked --test linux_install_lifecycle
./scripts/validate-install-acceptance.sh          # 21 tests, OK
python3 scripts/linux_acceptance.py self-test     # 15 tests, OK
bash -n + shellcheck on the new and modified shell sources
path-leak diagnostic regression asserted end to end (leak still rejected and named;
the canonical prefix is still accepted)
  qmlcachegen wrapper source reviewed; full Qt/CXX-Qt output remains a hosted
  gate rather than a local PASS claim
```

**Not verified locally:** the complete `synveil-desktop` release build and its
two-tree byte comparison. The local environment has Qt host tools, but no local
run is promoted to hosted evidence. The wrapper change is deliberately routed
through hosted CI for the real CXX-Qt build. No claim is made here that the
desktop binaries are byte-identical; that is precisely what the hosted
`desktop-qt-reproducibility` job measures, and its result is unknown until it
runs.

## Not verified, and why

The graphical-automation driver, the VM power-interruption driver and the
quick-install trust journey are **not implemented**. Writing them blind, with
no Qt toolchain, no virtualisation and a 180-second command ceiling in the
authoring environment, would have produced plausible-looking infrastructure
that could be mistaken for evidence.

The adapter reports `BLOCKED` with a specific reason for each unimplemented
step rather than degrading to a pass.

## Residual defects left open

1. `synveil-desktop` byte-identity is **unproven, not proven**. The identified
   root cause (the absolute QML source path in qmlcachegen output) has been
   fixed at its source, and the change is verified at the wrapper boundary. It
   has *not* been verified end to end, because that needs a Qt toolchain this
   environment does not have. The hosted `desktop-qt-reproducibility` job is
   the test; until it reports byte-identical, the private-path scan and the
   `build-b differs` gate must still be treated as failing.
2. The private-path scan was deliberately **not** weakened and `/home/` was not
   suppressed. If any second QML-adjacent absolute path exists, it will surface
   in the hosted run, which is the intended outcome.
3. No OS image digest is pinned, and no image URL is recorded, so the native VM
   matrix remains BLOCKED by construction.
4. The graphical-automation, VM power-cut and quick-install trust drivers are
   still unimplemented.
5. The PostgreSQL 17 `d.daticulocale` failure is untouched: it is a server
   schema issue outside Linux install scope.

## Phase C status

```
SYNVEIL_LINUX_CLEAN_MACHINE_ACCEPTANCE_READY: NOT EMITTED
SYNVEIL_V0_2_LINUX_INSTALL_EXPERIENCE_READY: NOT EMITTED
```

See [Linux clean-machine acceptance](LINUX_CLEAN_MACHINE_ACCEPTANCE.md) for the
full evidence map.
