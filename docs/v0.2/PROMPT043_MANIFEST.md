# Prompt043 manifest — Installer Security Hardening

Status: **SOURCE IMPLEMENTED; final validation/publication evidence is recorded
only after observation**. P044 is explicitly deferred.

## Baseline and branch discipline

- Repository: `https://github.com/nghianguyen150612/Synveil.git`.
- Local repository: `/workspace/Synveil`.
- Expected and actual baseline: `348201593addc93a9760b7fb18dc540673983076`.
- Starting checkout: clean branch `work`, HEAD
  `3caf47d033101d596c7868820f896ccc2df9574b`; no work was reset/discarded.
- Fetched origin and verified fetch/push URL before changes.
- Verified PR #73 merged at the baseline and P042A head
  `7bfedee1ba5cba22db3b0af51a78f1a7ac87a955` remains an ancestor.
- No intervening main commits or prior P043 implementation were found.
- Task branch: `feat/installer-security-hardening`, created from verified
  `origin/main`. It does not start with `codex/`. Publication and pre-PR actual
  remote verification must use exactly this name; no `codex/*` branch is used.
- Product version stays `0.1.0`; no schema/data reset, release or v0.2.0 tag.

## Ownership audit and threat model

[Installer security hardening](INSTALLER_SECURITY_HARDENING.md) records the
complete current-source audit, trust/mitigation matrix, evidence mapping,
privilege model and limits. Audited P005/P006/P007/P008/P009/P010/P011 and native
Linux/Windows/AppImage boundaries remain the canonical owners. All required
contracts, source directories, relevant workflows and validators were inspected.
Existing strict trust, selection, bounded streaming, redirect, no-clobber,
protected-state, unknown-schema and reconciliation behavior was retained where
already sufficient; no speculative vulnerability is recorded.

Concrete gaps fixed:

1. P006/P011 had only injected detached-signature callbacks. Added the maintained
   Ed25519 backend, bounded exact-byte verification and explicit local public
   policy. Unknown/malformed/unavailable verification has no weaker fallback. Local public inputs
   reject link/device/writable-policy objects and bind the opened regular file;
   P006 owns this bounded reader.
2. Quick-install supplied no persistent high-water. Added caller-owned private,
   locked compare/update through P011's existing rollback/equivocation owner;
   unknown state remains untouched.
3. P009 accepted a `Supported` downgrade/equal-version upgrade and prerelease
   versions without independently comparing versions. Added bounded strict
   stable ordering; equality is Repair, downgrade/unknown/intermediate stops.
4. Windows considered every newer version compatible. Added release-owned
   compiled exact source policy; current production older-source list is empty.
   Numeric lifecycle fixtures remain separate from production release metadata.
5. P005 basename checks allowed normalization aliases/control/Win32 device/ADS
   and option-leading names. Added portable component/property checks and typed
   malformed-field failures.
6. P006 ancestry/temp identity and P008 journal ancestry/lock object checks were
   incomplete. Added ancestor link/reparse, regular-object, owned-state and
   no-follow and non-writable trusted ancestry checks; POSIX promotion/cleanup is directory-descriptor anchored.
7. AppImage swallowed invalid/newer record errors, used a predictable PID temp,
   did not check deletion ancestors, and left desktop/systemd expansion chars
   active. Preflight now preserves unknown ownership/version, schema 2 binds
   helper version, tempfiles are unpredictable/RAII, paths/expansion are guarded.
8. Linux native tools used caller PATH/environment and lacked explicit argument
   separation. Now only reviewed OS-owned executable identities, a small C
   locale/PATH environment, structured argv and `--` cross sudo. Native-adapter
   last-use hashing is required and native payload verification follows mutation.
9. Native direct-package downgrade/unknown source could reach unpack. DEB
   preinst/RPM pre gates reject unsupported transitions before unpack; hooks
   scope sysusers/tmpfiles, reject config links, and omit raw argument logs.
10. Windows cleanup checked only the final reparse object and used prefix
    containment. Now component-safe identities, full ancestor checks, fixed
    per-user root, pre-copy collision checks, explicit option parsing and
    manifest-bound helper rehashing protect mutation/deletion/execution. Existing
    copy destinations also require prior ownership during repair/upgrade; a newly
    added payload cannot overwrite an unknown adjacent file.
11. Client sibling resolution could follow a client link outside the desktop's
    canonical directory. It now requires the same canonical parent. Registered
    Windows uninstall discovery is a bounded owned executable identity.
12. Bootstrap trust had only a manual file-list template. Added a deterministic
    closed archive producer and hardened the existing wrapper against inherited
    shell/Python/path initialization. Independent bundle authentication is still
    required before execution; no final production URL is invented.
13. AppImage raw Debug IO errors and generic post-native retry guidance could
    leak paths or mishandle uncertain mutation. Errors are finite; post-native
    unknown errors preserve staging and require reconciliation.

14. Windows Task Scheduler accepted caller SystemRoot and predictable shared
    temp XML. The existing P026 owner now uses GetSystemDirectoryW and native
    LocalAppData, private unpredictable staging, exact-byte read-handle validation
    and FILE_SHARE_READ protection through native consumption. Environment
    substitution and replacement fixtures are Windows-only; no new task owner.

Windows remains ordinary per-user; AppImage remains ordinary-user only. Native
DEB/RPM authority remains visible and scoped; Host/server setup remains outside
installer ownership. Unknown adjacent files and every durable protected class
survive ordinary lifecycle scope. No package database/lock deletion, process-name
kill, untrusted shell command, purge shortcut or trust-bypass option was added.

## Signature/key/dependency status

[ADR-073](../adr/ADR-073-v0.2-installer-security-hardening.md) is the next unused
ADR number verified on baseline. Ed25519 uses `cryptography==50.0.2` with its
maintained OpenSSL backend; exact 32-byte public keys/64-byte signatures and at
most 16 entries are accepted. Public IDs bind SHA-256 of raw public bytes.
Production keys must come from a deliberately provisioned explicit local policy;
CI, metadata and filenames establish no trust. No production public root or
private key, signing seed, recovery material or CI signing secret was committed.
Synthetic test keys are generated only in memory. Production private-key custody
and independent public-root rollout remain release operations; P043 implements
verification, not a release-signing service.

`tempfile` moves from install-engine dev dependency to runtime dependency for
secure AppImage staging and is reused by the Windows client startup staging;
it was already locked at 3.27.0. Cargo.lock changes only the client dependency
edge, with no package version upgrade. Existing windows-sys adds feature gates
for native system-directory/known-folder APIs. Quick-xml remains 0.41.0. No advisory ignore is added.

## Validation evidence

Observed local validation on the reviewed source:

- `cargo fmt --all -- --check`: pass.
- `cargo test -p synveil-install-engine --locked`: 510 tests pass, including
  lifecycle, engine, error model, journal, native reconciliation and AppImage.
- `cargo clippy -p synveil-install-engine --all-targets --locked -- -D warnings`:
  pass.
- `cargo test -p synveil-client --locked`: 109 tests pass, including the new
  canonical sibling escape test. D-Bus development files were supplied only in
  an isolated scratch sysroot; no repository/system dependency workaround.
- `cargo clippy -p synveil-client --all-targets --locked -- -D warnings`: pass
  using the same scratch D-Bus development metadata.
- Python offline suites: manifest 34, acquisition 69, channel 101,
  focused security 22, quick-install 12, platform detection 11, Windows model 6:
  all pass.
- `cargo deny check`: advisories, bans, licenses and sources pass; no ignores.
- `python3 -m pip_audit -r scripts/requirements-installer-security.txt`: no
  known vulnerabilities in the pinned crypto dependency and its dependencies.
- `python3 scripts/validate-installer-security-hardening.py`, its `py_compile`,
  `./scripts/validate-docs.sh`, `./scripts/validate-install-acceptance.sh`,
  Windows installer source validator and workflow YAML parse: pass.
- Linux package integration, DEB/RPM validators and AppImage static/build
  contract: pass. Native AppImage artifact execution is explicitly skipped.
- ShellCheck warning gate for wrapper/all DEB hooks and `git diff --check`: pass.

The local ordinary-user container has no sudo or Qt development installation.
Initial whole-workspace Clippy stopped at missing D-Bus metadata; after the
scratch sysroot enabled client validation, it stopped at missing Qt. Neither
attempt establishes a whole-workspace strict quality pass. Existing same-head
CI owns that gate. Native Windows compilation/options/junction evidence is
pending hosted CI; it is not native runtime or clean-machine qualification.
A Windows cross-check was attempted locally but stops at the unavailable MinGW
C toolchain; actual Windows fixtures remain hosted evidence. Linux clean-machine,
full native package/AppImage, server/PostgreSQL and wider
platform acceptance are not claimed. Hosted failures are inspected against the
actual baseline, rather than relabelled from the historical handoff.

## Diff security review

Review checks authenticate-before-use, no new shell execution, component path
checks, fixed executable/argument boundaries, no environment trust bypass,
explicit test-key isolation, independent downgrade comparison, ancestor-link
rejection, protected unknown schema/state, finite diagnostics and
reconcile-before-retry. The complete diff, including new files, was reviewed before staging.
The reviewed fixes preserve these boundaries and add no shell execution or
unknown-state reset. The invalid inherited Inno privilege override value was
replaced with its documented disabled (blank) default; its source validator
now checks the actual valid policy. Staged paths are explicitly reviewed. No P044 implementation is included.

## Reviewed file inventory

- `Cargo.lock`
- `crates/client/Cargo.toml`
- `.github/workflows/installer-security-hardening.yml`
- `crates/client/src/launch.rs`
- `crates/install-engine/Cargo.toml`
- `crates/install-engine/src/appimage.rs`
- `crates/install-engine/src/bin/synveil-appimage-integration.rs`
- `crates/install-engine/src/journal.rs`
- `crates/install-engine/src/lifecycle.rs`
- `crates/install-engine/src/linux_package.rs`
- `crates/install-engine/tests/appimage_integration.rs`
- `crates/install-engine/tests/journal_contract.rs`
- `crates/install-engine/tests/lifecycle_contract.rs`
- `crates/install-engine/tests/linux_package_integration.rs`
- `deploy/install/quick-install.sh`
- `deploy/linux/package-integration-v1.json`
- `deploy/packages/build.sh`
- `deploy/packages/debian/postinst`
- `deploy/packages/debian/postrm`
- `deploy/packages/debian/preinst`
- `deploy/packages/debian/prerm`
- `deploy/packages/rpm/synveil.spec.tmpl`
- `deploy/release/windows-upgrade-policy.json`
- `deploy/windows/installer/Synveil.iss`
- `docs/adr/ADR-073-v0.2-installer-security-hardening.md`
- `docs/adr/README.md`
- `docs/v0.2/INSTALLER_SECURITY_HARDENING.md`
- `docs/v0.2/LINUX_QUICK_INSTALL.md`
- `docs/v0.2/PROMPT043_MANIFEST.md`
- `docs/v0.2/RELEASE_DOWNLOAD_INTEGRITY.md`
- `docs/v0.2/ROADMAP.md`
- `scripts/build-quick-install-bundle.py`
- `scripts/build-windows-installer.ps1`
- `scripts/linux_quick_install.py`
- `scripts/release_channel.py`
- `scripts/release_download.py`
- `scripts/release_manifest.py`
- `scripts/release_signature.py`
- `scripts/requirements-installer-security.txt`
- `scripts/test-windows-installed-runtime.ps1`
- `scripts/test-windows-installer-lifecycle.ps1`
- `scripts/test-windows-installer-security.ps1`
- `scripts/test_windows_lifecycle.py`
- `scripts/validate-docs.sh`
- `scripts/validate-installer-security-hardening.py`
- `scripts/validate-linux-package-integration.sh`
- `scripts/validate-repair-recovery-ux.py`
- `scripts/validate-windows-installer.py`
- `scripts/windows-security.ps1`
- `scripts/windows_lifecycle.py`
- `tests/installer_security/test_security.py`
- `tests/release_manifest/test_release_manifest.py`
- `tests/linux_quick_install/test_linux_quick_install.py`
- `tests/release_download/test_release_download.py`

## Publication and hosted evidence

Initial publication observed:

- Ordinary Git push succeeded; Git Data API publication was unnecessary.
- Local and remote initial commit: `bc623154044ea41f56e59096e1375b32b4afe6f5`.
- Local and remote initial tree: `b98684ff5e79b375a4638470e3aa8b6395264b75`:
  exact tree equivalence verified through GitHub before PR creation.
- GitHub verified actual branch `feat/installer-security-hardening`, base `main`,
  baseline parent, one intended initial commit/51 reviewed files and no unrelated
  commits. No `codex/*` ref was created or published.
- Real [PR #74](https://github.com/nghianguyen150612/Synveil/pull/74),
  **Synveil v0.2 P043: Installer Security Hardening**, is open.
- Initial focused run [37563255442](https://github.com/nghianguyen150612/Synveil/actions/runs/37563255442)
  passed Linux acquisition/invocation/engine, static/docs and crypto policy.
  Windows failures were handled as P043 gate blockers: tests launched Python
  scripts directly, and GUI toolchain installation was consumed without a
  process-completion boundary. Explicit interpreter and structured .NET argv/
  wait fixes preserve security. Later Windows evidence showed replacement of an
  open temp file is denied; the adversary now acts after handle closure on both
  platforms, retaining the last-use assertion rather than skipping it. The installed
  compiler then exposed an inherited invalid PE-resource version check. Upstream
  ISCC resources contain a placeholder; the producer now compiles a fixed
  Output=no probe and checks the actual engine version without changing the
  distribution hash pin. The next actual compiler result exposed inherited
  PowerShell array-expression grouping that merged generated directives; each
  directive is now separately grouped, with explicit count assertions.
- Review also closed writable-ancestor relocation of private Linux staging,
  channel high-water, journal and AppImage ownership paths. Root-owned sticky
  temporary roots remain supported. Tests prove zero write/delete under an
  unsafe ancestor. MacOS keeps only its fixed root-owned OS temporary aliases;
  user links remain rejected and no native qualification is claimed.
- Follow-up commits preserve published history; no forced rewrite. Superseded
  task-head CI runs were cancelled; final-head results remain required.

The actual baseline Rust CI run 37555904738 had a passing strict quality and
cargo-deny gate, but failed Ubuntu `package_unit_6_upgrade_preserves_state`,
macOS Unix-socket tests, Windows Unix-API compilation and Windows config/pipe
parity. Baseline native runs failed Windows payload private-path inspection
(37555904743), Windows candidate linker setup (37555904754), Linux desktop
reproducibility (37555904827), AppImage AppRun inspection (37555904765), and
PostgreSQL `d.daticulocale` (37555904835). These were read from actual baseline
job/log archives. They do not substitute for comparison with final-head failures.

Final-head hosted CI, merge confirmation and resulting main are pending actual
observation; they are not yet claimed. A commit cannot contain its own immutable
SHA/tree or future CI/merge identifiers: the final publication/head identifiers
and post-merge verification are also reported in the real PR and final handoff.
No planned push, PR, CI result or merge is called completed evidence.

P044 is explicitly deferred. Native clean-machine qualification, final URLs,
production signing-key operations, P045–P048 and the v0.2.0 release/tag remain
outside Prompt043.
