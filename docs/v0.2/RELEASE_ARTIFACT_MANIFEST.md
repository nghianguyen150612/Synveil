# Release artifact manifest v1

## Purpose and boundary

`SYNVEIL-RELEASE-MANIFEST.json` is the authoritative, public, machine-readable
inventory of concrete Synveil release artifacts. It identifies artifacts; it
does **not** assert that a release is complete. The contract is JSON Schema v1
at `deploy/release/release-manifest-v1.schema.json`, and the dependency-free
reference implementation is `scripts/release_manifest.py`.

A SHA-256 digest identifies bytes and detects accidental corruption. **Digest
metadata is not authenticity.** A digest obtained beside an artifact from the
same untrusted source supplies no trust. P006 owns authenticated manifest
acquisition, signatures, trust roots, key rotation, and fail-closed downloads.
P011 owns channels and release selection; v1 consequently has no channel,
URL, latest-version, or recommendation fields.

## Closed contract

Top-level fields are exactly `schema_version` (integer `1`), `product`
(`Synveil`), `product_version`, full lowercase 40-hex `source_commit`, and
`artifacts`. Unknown and missing fields, malformed identities, and every
schema version other than `1` fail closed. The development release candidate
version is `0.2.0`, declared once in `Cargo.toml` under
`[workspace.package].version`. Package producers read that value; the manifest
creator rejects any supplied version that differs. A candidate version does
not authorize a release or a version-distinct upgrade.

Each artifact has exactly these common fields: `id`, `artifact_type`, safe
relative-basename `filename`, `platform`, normalized `architecture`, `role`,
`product_version`, non-negative `size_bytes`, lowercase 64-hex `sha256`, and a
bounded `components` inventory. `package_metadata` is allowed only for native
packages. IDs are stable release roles, not random instance identifiers.

Controlled vocabularies are:

* artifact types: `deb`, `rpm`, `appimage`, `windows_installer`,
  `windows_portable_zip`;
* platforms: `linux`, `windows`;
* architectures: `x86_64`, `aarch64` (a schema capability is not a released
  architecture claim);
* roles: `native_package`, `primary_installer`, `portable`, `server_runtime`,
  `database_runtime`;
* components: `synveil-desktop`, `synveil-client`, `scheduled-maintenance`,
  `synveil-api`, `synveil-worker`, `synveil-server-migrate`, `postgresql-17`,
  `appimagekit-type2-runtime`.

DEB metadata contains `format`, `package_name`, `package_version`, and native
`package_architecture`. RPM additionally contains `package_release`. Common
architecture remains normalized while native aliases such as `amd64` stay in
package metadata. Native package version must equal the manifest product
version. The Windows ZIP is explicitly `portable`, never an installer; the
separate Setup EXE uses the distinct `windows_installer` type. Producer code
exists for both Windows surfaces; R03 reports no Windows artifact bytes as
built or signed.

| Type | Schema support | Current producer |
| --- | --- | --- |
| DEB | yes | `deploy/packages/build.sh` |
| RPM | yes | `deploy/packages/build.sh` |
| Windows portable ZIP | yes | `deploy/packages/build-windows.sh` |
| AppImage | yes | `deploy/packages/build-appimage.sh` |
| Windows installer | yes | `scripts/build-windows-installer.ps1` |
| Synveil server runtime bundle | yes | No release-manifest producer; `scripts/build-host-server-input.sh` creates unsigned source input only |
| PostgreSQL runtime bundle | yes | No producer or authenticated upstream runtime input is configured |

Schema support never fabricates an artifact. Production creation reads a real,
regular, non-symlink file under the declared artifact root and derives its size
and digest. It accepts neither caller-supplied size nor digest.

## Determinism, composition, and safety

Serialization is UTF-8, newline-terminated, sorted-key JSON with two-space
indentation. Artifacts are sorted lexicographically by `id`; timestamps,
hostnames, users, temporary paths, and random IDs are absent. Filenames are
single relative basenames. Unix/Windows absolute paths, separators, traversal,
NULs, symlinks, missing/non-regular files, size mismatches, and digest
mismatches are rejected.

Independent builders emit valid partial manifests. `merge` requires identical
schema, product, version, and source commit, rejects duplicate IDs, and sorts
the union. The result inventories represented bytes only; it never claims
release completeness.

```sh
python3 scripts/release_manifest.py validate SYNVEIL-RELEASE-MANIFEST.json
python3 scripts/release_manifest.py validate --artifact-root target/packages \
  target/packages/SYNVEIL-RELEASE-MANIFEST.json
python3 scripts/release_manifest.py inspect --json SYNVEIL-RELEASE-MANIFEST.json
python3 scripts/release_manifest.py merge --output combined.json linux.json windows.json
./scripts/validate-release-manifest.sh
```

The Linux `SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt` remains provenance/build
compatibility evidence. The ZIP-internal `SYNVEIL-MANIFEST.txt` remains a
payload inventory. Neither has the release-level scope of this JSON manifest,
and neither is removed or weakened.

P004 acceptance evidence can later bind its artifact ID to this manifest ID
and digest without filename inference: acceptance result → artifact ID →
artifact digest → release manifest. P006 may authenticate and add trusted
acquisition/signature information without changing this fundamental identity.
