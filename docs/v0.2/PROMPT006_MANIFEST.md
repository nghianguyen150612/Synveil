# Prompt006 evidence manifest

## Starting state and scope

- Starting main: `e2361304e58cc61a28138b8ad8df7a938e100e6d`.
- Task branch: `work` (environment-provided isolated branch).
- Starting worktree: clean.
- Workflow: one focused commit, PR to `main`, review, squash merge, and live-main
  verification are required. This file intentionally does not predict the merge
  SHA.
- Prompt-owned implementation/schema/tests: `scripts/release_download.py`,
  `scripts/validate-release-download.sh`,
  `deploy/release/release-auth-v1.schema.json`, and
  `tests/release_download/test_release_download.py`.
- Prompt-owned architecture/docs plus focused navigation and CI edits comprise
  the remaining changed paths.

## Contract evidence

The trust model authenticates exact manifest bytes before P005 semantic use and
separates publisher authentication from artifact size/SHA-256 integrity.
Trusted roots are local bootstrap pins or locally configured key identities,
never remote metadata; there is no TOFU. Lowercase exact SHA-256 pinning is
implemented. Detached-signature metadata and an injected verifier interface are
defined; unknown/unavailable verification fails closed. Production signing-key
provisioning and an approved production crypto backend are pending.

Production URLs require allowlisted HTTPS origins with no userinfo. Redirects
are explicitly evaluated, trusted-origin-only, downgrade-safe, loop-safe, and
bounded at five. Manifests are bounded at 1 MiB. Selection uses exact version,
optional exact source commit, normalized platform and architecture, explicit
P005 type and role, and requires one match; it never performs channel or version
ordering. Streaming artifact verification enforces exact size/digest before
same-directory atomic promotion.

## Validation record

The focused suite contains **44 tests** and has no network dependency. Required
commands are:

```text
./scripts/validate-release-download.sh
./scripts/validate-release-manifest.sh
./scripts/validate-install-acceptance.sh
./scripts/validate-docs.sh
python3 -m py_compile scripts/release_download.py tests/release_download/test_release_download.py
bash -n scripts/validate-release-download.sh
python3 -m json.tool deploy/release/release-auth-v1.schema.json
cargo fmt --all -- --check
git diff --check
```

No native Windows VM, Qt runtime, rpmbuild, PostgreSQL, root, Docker, external
download, installer execution, or production private key is required or claimed.
P011 retains channels/latest/recommendations/version ordering; P017 retains
quick-install UX; P007+ retains installer mutation. Phase B remains open.
