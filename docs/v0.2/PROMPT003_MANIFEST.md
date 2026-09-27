# Prompt003 manifest — installation architecture and component ownership

Status: **Architecture contract documented; implementation deferred.** This
manifest records Prompt003 scope and evidence. It does not claim installed-app
acceptance or delivered v0.2 behavior.

## Baseline

| Item | Value |
| --- | --- |
| Repository | `nghianguyen150612/Synveil` |
| Branch | `main` |
| Starting HEAD | `66fee43094717a0bca09936f1e1ed7173de14a17` |
| Starting `origin/main` | `66fee43094717a0bca09936f1e1ed7173de14a17` |
| Live remote `main` at start | `66fee43094717a0bca09936f1e1ed7173de14a17` |
| Starting worktree | Clean |
| Immutable product baseline | `v0.1.0` → `fa23232ff0154f627ebdd221ec5435134f177af0` |
| Frozen state | Product `0.1.0`; 36 server migrations; 7 client migrations; `LOCAL_SCHEMA_VERSION = 7` |

The baseline matches the requested starting SHA and all three main refs agree.
The `v0.1.0` tag remains unchanged.

## Prompt-owned paths

- `docs/v0.2/INSTALLATION_ARCHITECTURE.md` — normative layers, component
  boundaries, effect model, ownership taxonomy and matrix, lifecycle, privilege,
  first-run/server-bootstrap separation, blocker disposition and later prompt
  ownership.
- `docs/v0.2/PROMPT003_MANIFEST.md` — this evidence and scope record.
- `docs/adr/ADR-050-v0.2-installation-component-ownership.md` — accepted
  architecture decision refining ADR-049.
- `docs/adr/README.md` — ADR-050 index entry.
- `docs/v0.2/ROADMAP.md` — P003 completion record only.

## Blockers and decisions

Architecture aspects of INS-01 through INS-09 are resolved by ownership,
effect, privilege and lifecycle boundaries; implementation remains assigned to
the roadmap prompts. INS-10 remains `ACCEPTANCE_PENDING` for P004 and the named
native acceptance prompts. See the exact matrix in
[INSTALLATION_ARCHITECTURE.md](INSTALLATION_ARCHITECTURE.md#10-prompt002-blocker-disposition).

The architecture defines common installation coordination, scoped platform
adapters, native package-manager authority, twelve canonical resource classes,
effect-specific compensation, reconciliation after unknown outcomes, a
data-preserving ordinary uninstall, explicit purge, opt-in startup, and a
separate Host bootstrap domain. It does not choose installer technology,
artifact metadata/signatures, PostgreSQL distribution, service design, or
release implementation.

## Validation

| Check | Result |
| --- | --- |
| `./scripts/validate-docs.sh` | Passed: DOC-UNIT-1 through DOC-UNIT-7 |
| `cargo fmt --all -- --check` | Passed |
| `git diff --check` | Passed |
| Ownership and blocker audit | Passed: all 12 ownership classes present; exactly one blocker row for each INS-01 through INS-10 |
| Safety, roadmap and link audit | Passed: required safety ownership terms present; P003 only marked complete; documentation validator passed links |

No runtime builds, package rebuilds, native installation, database setup or
clean-machine acceptance were run; they are outside this documentation prompt.

## Commit and publication policy

After validation, stage only the five paths listed above, review the complete
staged diff, create exactly one focused commit with subject
`docs: define v0.2 installation architecture`, and push `origin/main`. Verify
local `HEAD`, `origin/main`, live remote `main` and worktree state. Do not
change the v0.1.0 tag or emit the Phase A checkpoint. Final commit/push evidence
is reported after those actions; no future commit SHA is embedded here.
