# Prompt018 evidence manifest — Linux platform detection

Status: **source gates defined; final-head hosted execution required**.

The versioned qualification policy contains only Ubuntu 24.04 x86_64 → DEB/APT
and Fedora 42 x86_64 → RPM/DNF. Debian has no native acceptance evidence and is
therefore detected but unqualified. No migration, schema, protocol, database,
server-bootstrap, P019, or P020 behavior changes in this prompt.

| Evidence | Automated proof |
| --- | --- |
| PLATFORM-1–2 | bounded data parser and `/usr/lib/os-release` fallback fixtures |
| PLATFORM-3–5, 14–18 | exact `ID`/`VERSION_ID`, derivative and future-version fixtures; `ID_LIKE` never participates in matching |
| PLATFORM-6–9 | x86 aliases, ARM normalization, i686 and unknown-architecture rejection fixtures |
| PLATFORM-10–11 | strict policy validator checks schema, enums, exact versions, unique IDs and unique tuples |
| PLATFORM-12–13 | Ubuntu/Fedora exact target fixtures and real-host workflow assertions |
| PLATFORM-19 | missing required commands produces `MISSING_PACKAGE_MANAGER` only after exact policy match |
| PLATFORM-20–23 | unsupported, detect-only, and mismatched assertion tests prove acquisition/manager calls remain zero |
| PLATFORM-24, 26–27 | pinned Ubuntu 24.04 and Fedora 42 acceptance invoke quick install without a profile |
| PLATFORM-25 | existing P006/P011 suites and orchestrators remain the post-qualification trust chain |
| PLATFORM-28 | unchanged AppImage P015/P016 final-head workflow remains required |
| PLATFORM-29 | frontend assignment/readonly split plus warning-level ShellCheck gate closes SC2155 |

Run `./scripts/validate-linux-platform-detection.sh` for policy and fixture
validation. The Linux package workflow also runs P017, P006, P011, DEB/RPM
build/reproducibility, native install/rerun/removal/preservation, and detector
real-host assertions. Hosted completion must be reported from the final commit;
this source manifest does not claim that local evidence is hosted evidence.
