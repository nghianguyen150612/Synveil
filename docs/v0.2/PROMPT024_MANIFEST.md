# Prompt024 implementation manifest

Status: **source implemented; native final-head evidence pending**

| Field | Evidence |
|---|---|
| Starting SHA | `1b34236fb404eccafea2c562b9ec1ed6dc191912` |
| Branch | `codex/p024-windows-runtime-deployment` |
| Runtime producer | `deploy/packages/build-windows.sh` fresh validated staging export |
| Qt / windeployqt | Qt `6.8.3`, `win64_msvc2022_64`; native tool identity/version recorded in generated `SYNVEIL-MANIFEST.txt` |
| Runtime artifact identity | `windows-x86_64-portable` closure shared with `windows-x86_64-installer` |
| Required core | desktop, client, qt.conf, qwindows, LICENSE, NOTICE |
| P023 inherited status | Public current-main run `37100171189` for `1b34236fb404eccafea2c562b9ec1ed6dc191912` completed **failure** in **Build runtime payload** after linker selection and static validation passed; installer/runtime smoke steps did not run. Public annotations expose only exit 1, not the causal log. P023 PR-head runs `37099986540`/`37099984181` were still in progress when inspected. Classification: `INHERITED_WINDOWS_INSTALLER_REGRESSION`, exact cause pending accessible logs/final-head rerun. |
| Inherited fixes retained | authenticated absolute MSVC linker; `CARGO_ENCODED_RUSTFLAGS`; discrete direct-rustc flags |
| Staging digest | **PENDING native build**; never fabricated |
| PE architecture/import audit | Source gate implemented; **PENDING final-head hosted result** |
| Qt/QML closure | Native windeployqt plus audited closure implemented; **PENDING final-head hosted result** |
| Installed manifest verification | Full size/SHA-256 verifier implemented; **PENDING final-head hosted result** |
| Installed desktop smoke | Isolated installed-path QML smoke implemented; **PENDING final-head hosted result** |
| Installed client smoke | Bounded no-profile exit-78 probe implemented; **PENDING final-head hosted result** |
| Sibling resolution | Canonical executable-relative contract retained and staged siblings required; **PENDING final-head hosted result** |
| Portable parity | One staging/manifest authority implemented; **PENDING final-head hosted result** |
| Reproducibility | Two-build staging inputs, generated file list, and Setup byte comparison retained; **PENDING final-head hosted result** |
| Uninstall preservation | Runtime removal/state sentinel smoke retained; **PENDING final-head hosted result** |
| Hosted workflow ID/result | **PENDING** |
| Known blocker | Native Windows/Qt/Inno execution is unavailable in the local Linux environment |
| Deferred | P025 per-user qualification; P026 startup persistence; P027 full lifecycle; P028 native end-to-end acceptance |

## Changed source surfaces

The implementation changes the Windows runtime producer, installer inventory builder, installed-runtime verifier, focused Windows workflow, static validator, P024 documentation/roadmap, and release/deployment documentation. Exact committed paths are authoritative in Git.

## Local evidence

Local checks establish source/static correctness only. Native PE execution, windeployqt, Inno compilation/install, and installed Qt/QML launch must remain pending until the final-head hosted run completes.
