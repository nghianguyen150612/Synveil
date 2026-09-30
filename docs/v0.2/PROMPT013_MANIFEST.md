# Prompt013 evidence manifest

## Source and contract

- Starting SHA: `754032c381a58d81df990b0a7ad2884947370d3c`
- Task branch: `prompt013-debian-ubuntu-desktop-package-ux`
- Task commit / PR / final squash SHA: recorded by hosted source control after publication
- Expected final parent absent concurrent main changes: starting SHA above
- Contract: `deploy/linux/debian-desktop-ux-v1.json`, schema version 1 (closed)
- Package/product/native architecture: `synveil`; `x86_64`; `amd64`
- Product version: `0.1.0`; dependency count: 23
- Desktop identity/path: `synveil.desktop`; `/usr/share/applications/synveil.desktop`
- Executable/icon: `/usr/bin/synveil-desktop`; `synveil`

## Evidence

- Dedicated Prompt013 tests: 84 in `debian_desktop_package_ux_units`.
- Actual artifact validator: exact package/version/architecture/dependencies,
  required contents, root ownership, modes, desktop source parity, and native
  desktop validation. Artifact absence is explicitly `SKIPPED`, never passed.
- Native workflow: builds DEB and RPM, runs reproducibility and inspection, then
  supplies the DEB to the Prompt013 validator. RPM results are regression-only.
- Hosted native workflow installs the just-built DEB with APT on the disposable
  Ubuntu runner, verifies dpkg registration and installed launcher/icon payload,
  executes the installed /usr/bin/synveil-desktop as the non-root runner with
  the repository's supported Qt offscreen QML smoke mode, then performs native
  package removal and verifies exact test-created config/state sentinels survive.
  This is ci-native-scoped evidence. Genuine package-app double-click and
  clean-machine evidence remain P020-owned.
- P004–P012 gates remain retained in CI. P014–P020 remain pending.
- Changed paths are the Prompt013 contract/validator/test/docs, narrow DEB copy,
  CI wiring, roadmap/ADR index, and the behavior-preserving SC2155 correction.
- Immutable `v0.1.0`: expected annotated tag object
  `da6bd2fee5c0b266f95d0d7c53cd0aaf69fc4a9c`; no `v0.2.0` tag is created.


## Hosted package-build unblock

The first hosted artifact run after Prompt013 exposed an older RPM %install portability defect: rpmbuild executes the block with POSIX /bin/sh, while the template used `set -euo pipefail`. The DEB itself built successfully, but the combined DEB/RPM build step stopped before later DEB evidence could run. Prompt013 applies the minimal semantics-preserving POSIX correction to `set -eu`. This does not claim RPM desktop UX or complete P014.

## Hosted RPM reproducibility limitation

Final hosted Prompt013 evidence shows the DEB rebuild is byte-identical, while
the RPM rebuild is not: the same package payload and binaries produced RPMs
with different size/SHA-256 bytes. This is an RPM-only reproducibility defect
and remains P014-owned; it is not reported as a pass. The native workflow keeps
the strict RPM byte-comparison gate enabled, but sequences it after Prompt013's
DEB artifact/install/launch/removal evidence and retained static/systemd package
regressions so the P013 evidence remains observable before the P014-owned gate
fails.

- Hosted artifact validation verifies the dpkg-deb permission/ownership parser against the real output grammar; the initial P013 run exposed and corrected an overly narrow permission regex before native installation evidence was accepted.
