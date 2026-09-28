# Prompt005 manifest — unified release artifact metadata

Status: **IMPLEMENTED; publication is not part of this prompt.**

## Baseline and owned surface

- Starting checkout: branch `work`, HEAD `e18b43c9505a3797e49b91efc3846df5af09f0e9`.
- The supplied container had no `origin` remote or local `v0.1.0` tag; commit
  `fa23232ff0154f627ebdd221ec5435134f177af0` is present as the immutable v0.1
  release baseline in history.
- Product version remains `0.1.0`; server migrations remain 36; client
  migrations and `LOCAL_SCHEMA_VERSION` remain 7. Release manifest schema is
  independently versioned at `1`.
- Owned paths are the v1 schema, standard-library tool and 32 tests, validation
  wrapper, this manifest and normative contract, focused builder/CI wiring,
  roadmap progress, and documentation index entries.

The schema supports `deb`, `rpm`, `appimage`, `windows_installer`, and
`windows_portable_zip`. Builders currently generate entries only for DEB, RPM,
and the Windows portable ZIP. AppImage and Setup EXE remain schema-only and do
not appear as fake outputs.

Legacy `SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt` generation and validation remain
intact. The portable ZIP's internal `SYNVEIL-MANIFEST.txt` and payload audit
also remain intact. The unified file is always named
`SYNVEIL-RELEASE-MANIFEST.json` and describes release artifacts rather than
their internal files.

## Validation contract

`./scripts/validate-release-manifest.sh` executes 32 focused tests, including
all artifact types, schema drift, deterministic creation and merge, package
metadata/version consistency, strict unknown-field behavior, wrong size and
digest, missing files, duplicate IDs, source mismatch, traversal, Unix and
Windows absolute paths, and symlink escape. Package CI validates the generated
manifest against actual DEB/RPM/ZIP bytes and compares independently generated
manifest bytes.

Required repository gates are `./scripts/validate-release-manifest.sh`,
`./scripts/validate-docs.sh`, `./scripts/validate-install-acceptance.sh`,
`cargo fmt --all -- --check`, and `git diff --check`. Native Windows packaging
remains a CI responsibility where the Qt/Windows toolchain exists.

P006 still owns trust, signatures, authenticated metadata and downloads. A
manifest SHA-256 is identity/integrity metadata, not authenticity. P011 still
owns channels and version selection. Phase A contracts remain frozen.

## Git completion policy

One focused commit is created only after exact-path staging and complete staged
diff review. Push and remote verification are reported at execution time; this
document does not predict a future commit SHA.
