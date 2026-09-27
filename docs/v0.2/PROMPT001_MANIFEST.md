# v0.2 Prompt001 documentation manifest

Scope: effortless installation product contract, architectural decision,
scope-oriented Prompt001–048 roadmap and minimal documentation navigation.

## Verified starting state

- Branch: `main`; starting worktree clean.
- Starting `HEAD`, `origin/main` and live remote `refs/heads/main`:
  `fa23232ff0154f627ebdd221ec5435134f177af0`.
- Released `v0.1.0` annotated tag object:
  `da6bd2fee5c0b266f95d0d7c53cd0aaf69fc4a9c`.
- Released tag peeled commit:
  `fa23232ff0154f627ebdd221ec5435134f177af0`.
- No unexpected advance from the requested baseline. The tag must remain
  unchanged locally and remotely; Prompt001 creates no tag/release/assets.

## Explicit path manifest

| Path | Purpose |
| --- | --- |
| [INSTALLATION_PRODUCT_CONTRACT.md](INSTALLATION_PRODUCT_CONTRACT.md) | Normative target UX, platform matrix, boundaries, ownership and acceptance journeys. |
| [ROADMAP.md](ROADMAP.md) | Seven phases spanning Prompt001–048 with scope allocation, dependencies and decision/evidence gates. |
| [ADR-049](../adr/ADR-049-v0.2-effortless-installation-architecture.md) | Accepted bilingual target architecture; supplements existing safety/lifecycle decisions. |
| [Documentation index](../README.md) | Distinguishes v0.2 planning from shipped v0.1 release instructions. |
| [ADR index](../adr/README.md) | Registers the next consistent ADR number, 049. |
| [PROMPT001_MANIFEST.md](PROMPT001_MANIFEST.md) | This exact scope and audit record. |

Only these six documentation paths belong in the Prompt001 commit. No product
source, package script/hook/manifest, dependency, lockfile, migration, schema,
version, generated artifact or existing release guide is changed.

## Validation and frozen baseline

The required checks passed and all six paths were reviewed. The completion
handoff reports the actual commit/push/remote SHA after source-control completion.
This source record does not invent its own future commit SHA.

| Check | Result |
| --- | --- |
| `./scripts/validate-docs.sh` | PASS; all seven documentation checks. |
| `cargo fmt --all -- --check` | PASS. |
| `cargo deny check` | PASS; advisories, bans, licenses and sources. Non-fatal duplicate-version warnings from the unchanged dependency graph. |
| `git diff --check` | PASS; whitespace/EOF checks also cover every new file. |
| All newly added relative file links and Markdown fragments | PASS; file existence and Markdown heading fragments checked across all six manifest paths. |
| Documentation-only explicit path audit | PASS; exactly six documented paths, no product/runtime/packaging/script/API/CI changes from baseline. |
| Workspace/product version remains `0.1.0` | PASS; all workspace crates retain inherited product version. |
| Server SQL migrations `36`; client SQL migrations `7`; `LOCAL_SCHEMA_VERSION = 7` | PASS; catalogs and source constant unchanged from baseline. |

The staged manifest/stat/diff hygiene check is mandatory immediately before
commit, as specified below; final Git evidence is reported in the completion
handoff rather than a self-referential source commit record.

Full Rust/Qt builds, clean release rebuild, two-root reproducibility, stress,
Windows toolchain setup and PostgreSQL environment construction are deliberately
not run for this documentation-only prompt. No clean-machine installation or
first-run runtime acceptance is claimed; the ten journeys are future gates.

## Source-control completion policy

Review every modified path, validate, stage these exact paths only, inspect the
cached names/stat/diff, and create exactly one commit:
`docs: define v0.2 effortless installation contract`. Push `origin main` and
verify `HEAD == origin/main == live remote main`, a clean worktree, exactly one
commit since the baseline, and the unchanged local/remote v0.1.0 tag. A failed
push preserves the local commit and prevents the completion gate.
