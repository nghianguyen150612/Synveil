# Prompt020 Manifest — Linux Clean-Machine Acceptance

## Outcome

Prompt020 delivered the Linux clean-machine acceptance *harness*, the hosted
workflow, and an honest classification of the residual P015–P019 CI failures.

**Prompt020 did not close Phase C.** Native acceptance evidence does not exist.
No readiness token is emitted and the PR is not merged.

## Baseline

| Item | Value |
| --- | --- |
| Prompt019 squash merge | `1bd1e898a4dd4639bf342e5d097e7fa58628365d` |
| PR #22 head | `28b0b402d6a74b62086292ceffa8f30580142dbe` |
| Observed `origin/main` at authoring time | `179fd5f0c6bfd098cb10f29f38aea10a33711a2b` |

The stated baseline commit was verified through `gh pr view 22` and matches the
real squash merge. However `origin/main` had advanced to a **root commit with
no parents** whose tree is identical to the baseline tree apart from three
added iOS documents. The pre-existing history is therefore no longer reachable
from any ref.

This matters for `UPGRADE-TEMPLATE-1`, which names
`fa23232ff0154f627ebdd221ec5435134f177af0` as its v0.1.0 source. That commit is
currently only reachable as a dangling object.

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

* **Artifact diagnostics.** `synveil_assert_no_private_paths` now reports the
  full leaking path. Previously every leak printed an identical bare `/home/`,
  which named no cause and could not be acted on. Gate strength is unchanged:
  the detection pattern was not weakened.
* **Toolchain remap.** `RUSTUP_HOME` is now remapped, closing a real gap in the
  path policy on runners that install rustup under the account home.
* **`scripts/linux_acceptance.py`.** The real Linux execution layer: host fact
  collection, exact artifact binding with fail-closed digest verification, a
  clean-machine probe, a closed typed action dispatch, capability resolution,
  redaction, and result-v1 emission. 15 self-tests.
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
cargo check -p synveil-install-engine --locked
./scripts/validate-install-acceptance.sh          # 21 tests, OK
python3 scripts/linux_acceptance.py self-test     # 15 tests, OK
bash -n + shellcheck on the new shell sources
path-leak diagnostic regression asserted end to end
```

## Not verified, and why

The graphical-automation driver, the VM power-interruption driver and the
quick-install trust journey are **not implemented**. Writing them blind, with
no Qt toolchain, no virtualisation and a 180-second command ceiling in the
authoring environment, would have produced plausible-looking infrastructure
that could be mistaken for evidence.

The adapter reports `BLOCKED` with a specific reason for each unimplemented
step rather than degrading to a pass.

## Residual defects left open

1. `synveil-desktop` still embeds a private build path. Root cause is cxx-qt
   embedding absolute QML source paths as C++ string literals, which no
   path-remap flag rewrites. The improved diagnostic identifies the exact input
   once the build is next run.
2. `synveil-desktop` release reproducibility is still broken.
3. `synveil-install-engine` still fails to compile on Windows and macOS.
4. No OS image digest is pinned.

## Phase C status

```
SYNVEIL_LINUX_CLEAN_MACHINE_ACCEPTANCE_READY: NOT EMITTED
SYNVEIL_V0_2_LINUX_INSTALL_EXPERIENCE_READY: NOT EMITTED
```

See [Linux clean-machine acceptance](LINUX_CLEAN_MACHINE_ACCEPTANCE.md) for the
full evidence map.