# Prompt045 manifest — Cross-platform clean-machine matrix

## Continuation record: e234 Windows child failure investigation

The task resumed on the existing P045 branch at remote head
`6bde285334d1fdb057126e924d47c09629bc7469` (tree
`786ea75b0635b66c076fcca89a7cadb35ceb889f`), matching the local checkout.
PR #78 remains OPEN, DRAFT, and unmerged; `main` is
`a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`.

On e233 Windows native run `37765484348`, standard-user child job
`113278758080` failed after confirming the expected disposable SID and a
non-administrator token. Artifact `11546750732` (634 bytes, SHA-256
`d8c1a9bca53ea34830ab0ebba55e775c24ccceebb921a3555efedcbe1114e3c8`)
contains only child stdout/stderr. It records both inherited profile variables
as mismatched against the token profile; the first thrown condition is Setup
exit code 1 at `test-windows-per-user-installation.ps1:78`. The mismatch was
not an assertion and there is no captured Inno log, so the exact Setup failure
cause is not yet established.

The next harness revision derives the child profile from
`Win32_UserProfile.LocalPath`, captures only allowlisted files up to 262,144
bytes, stores evidence under that actual profile, and sets profile-scoped
environment variables from the token profile before Setup runs. This fixes the
observed harness environment/evidence gap. A fresh exact-candidate Windows run
must confirm whether it also resolves Setup exit 1; no standard-user PASS is
claimed until its installer log and acceptance record support that result.

At 2026-10-08 11:15 UTC, e234 P045 aggregate run `37768137891` had no jobs yet;
Windows candidate build `37768137606` was in progress, Windows installer run
`37768137540` was queued, Linux clean-machine artifact production
`37768137473` and AppImage run `37768137642` were in progress, Linux package
Qt reproducibility was in progress, P043 had four of seven jobs passed and
P044/Rust CI jobs remained queued. These hosted statuses are pending or source
evidence only; no native matrix row passes as a result.

## Current continuation: e233 Userenv import rerun

At the e233 observation recorded here, the existing branch was at
`32a7f55cf4a5b754c0b5732a5339b07a51f1cb29`, tree
`e3dba86a48dacdb112753dd48abf470352cb0a53`. PR #78 was OPEN, DRAFT, and
unmerged; main was `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`.

P045 aggregate run `37765484729` passed contract (`113273965143`) and source
gates (`113273965560`); its source-gates artifact `11544926493` binds PASS to
e233. The aggregate Windows candidate (`113277693547`) and Linux artifact
producer (`113277693805`) were running; Linux adapter/contract job
`113277693759` passed. No e233 result-v1 matrix artifact existed yet.

Windows candidate run `37765484348`, producer job `113271991216`, passed and
produced `SynveilSetup.exe` at 32,889,573 bytes, SHA-256
`1d603fe8d1aac7f983db21fc7bf50d38bdbdb39f4cd903756f8de9cde32f3115`, source
`32a7f55`. Release-manifest SHA-256 is
`09591e2c48d3a77ff8484ace4626e262004bc33850c16fc74fd24568487aa55c`; toolchain
identity SHA-256 is
`4bfc06ac04dcd026937e3f289cc7ccd1426d528ec9d818f92783716620c0769d`. Artifact
`11545650446` is a 32,349,622-byte ZIP, SHA-256
`dbe913f3695a8505e4a590eeb78bb794ceb889e93a1b2a09449cc19d8709a243`; its
downloaded payload and metadata matched independently. Child
`113278758080` was queued, so standard-user acceptance remains unproven.

Linux clean-machine run `37765484365` passed contract/adapters job
`113272145813`; artifact producer `113272145478` was still gating byte
reproducibility. Linux package run `37765484360` had Qt reproducibility
`113272565731` and package job `113272566080` in progress, with systemd job
`113272566017` passed. AppImage run `37765484292`, job `113271991388`, passed
reproduction, inspection, smoke, and lifecycle: payload 48,638,456 bytes,
SHA-256 `349cf8ab0b085d1edb0f318d7aa8f192486567a4305f6c0a8cf40472bc289436`;
artifact `11544284724` is 48,029,789 bytes with SHA-256
`aed210c0b3d8a03763d08eacfb211f52bbda1d1bf973774498cabb5914729663`. Producer
evidence does not qualify native Ubuntu/Fedora AppImage acceptance.

P043 run `37765484343` completed all seven jobs successfully. P044 run
`37765484302` had four jobs pass and six queued, so it remained incomplete.
Rust CI run `37765484394` found Windows-only test compilation failures from
Unix `server-config` test APIs, Windows fixture failures from `/tmp` paths, a
Windows named-pipe `Frame(Closed)` at `ping`, and three macOS IPC fixture
failures caused by symlinked temporary-directory ancestors. This follow-up
gates the Unix-only test module, uses native temporary paths in Windows
fixtures, canonicalizes the Unix IPC fixture roots, and arms the next named-pipe
instance before awaiting a client. Hosted confirmation is pending; Cargo, rustc,
and rustfmt are unavailable in this workspace.

The older Linux packaging test failure was a valid zstd-compressed DEB
(`control.tar.zst` and `data.tar.zst`) rejected by a gzip-only test assumption;
commit `fa72483` now extracts through `dpkg-deb` and accepts `gz`, `xz`, or
`zst`. The older QMP JSON `Extra data` failure was the extra closing brace from
`${2:-{}}`; `fa72483` corrected the argument framing and added guest boot
diagnostics. The prior VM jobs used TCG because KVM was unavailable, timed out
before SSH/readiness, and did not preserve serial/QEMU output, so their boot
cause remains unknown and no guest product journey ran. The e233 guest rerun is
still pending.

On e232 (`0765a9c`), Windows candidate production/reproducibility passed in run
`37763335735`, job `113264846876`. Candidate `SynveilSetup.exe` is 32,889,202
bytes, SHA-256
`8cd3147214986f1a1ab125cffe06d1c51057466938413442d3cbb95146dcd5ea`; artifact
`11543558488` is a 32,349,256-byte ZIP with SHA-256
`6e50a4be6c312f32a92d0cfd921a4c076a0bb5ba4b994dcad1339a9fb8b16ff8`. The
archive payload was independently verified. Its release-manifest and toolchain
identity hashes are `612163222f56bc72ebdee28c93a911b6da77bb038e693ab034757a8ff6ac8129`
and `db167c3ebaa3ae9c7cc0a52a60505c0e5f904d543329182aa1156b54180ec2f3`.

The standard-user child `113270883106` failed before its profile/SID checks.
Artifact `11544550956` records a missing `GetUserProfileDirectory` entry point
in `advapi32.dll`. The Unicode API is exported by `Userenv.dll` as
`GetUserProfileDirectoryW` ([Microsoft API reference](https://learn.microsoft.com/en-us/windows/win32/api/userenv/nf-userenv-getuserprofiledirectoryw)).
Commit `32a7f55` fixes the import and adds a static contract check. Local
Windows and P045 validators pass, with all 20 P045 unit tests passing. The
authoring environment has no PowerShell/.NET/C# compiler.

Exact e233 Windows native run `37765484348`, Windows installer run
`37765484543`, Linux packages `37765484360`, Linux clean-machine `37765484365`,
AppImage `37765484292`, P043 `37765484343`, P044 `37765484302`, and P045
aggregate `37765484729` were queued or in progress at observation time. Windows
candidate job `113271991216` and Linux clean-machine producer `113272145478`
were running; its contract/adapters job `113272145813` passed. P045 then began:
contract job `113273965143` and source-gates job `113273965560` were queued. No
e233 standard-user or result-v1 outcome is claimed.

On e232, Linux clean-machine contract/adapters passed but its exact artifact
producer was still running; package and Qt reproducibility jobs also remained
in progress. AppImage producer `37763335897` passed, with payload SHA-256
`b36b26adfa05e73c4a3babf15e7d74a2dd9d90ff36bc629c9c338e6ec8c3920c` and
artifact `11543577082` envelope SHA-256
`cf78e0a6989eba485ab03c9fe52a21f3840849ac53eab68fb248361d5bf13ee4`. This is
producer evidence only. P043 completed all seven e232 jobs; eight P044 jobs
passed and two were canceled as e233 was published. P045 e232 contract passed,
source-gates was canceled, Windows/Linux were skipped, and evidence gate
`113272284297` failed. It uploaded result-v1 `11544328344` (SHA-256
`284353f2919e4f22967440c4217b55acef7e8a8e0dacad8337caaa7e6a5cb2a0`) with
overall `BLOCKED`, zero candidates, 32 `BLOCKED` rows, and no source-gates
record. The e232 Linux clean-machine producer was canceled after
contract/adapters passed.
Native Windows 11 GUI/logon/IPC, Linux guest GUI, power-cycle, and first-run
server evidence remain unestablished.

## Current continuation: Windows child compile failure and focused correction

The branch advanced linearly from its previously reported remote head
`1215abe92af4d23936607f8f602d6a928d133bcf` to
`e230f3bbe04276ef2b21d2cfe13473993c6f9b46`. On e230, Windows producer run
`37760807332`, job `113256459800`, built two reproducible Setup candidates and
passed manifest and `asInvoker` inspection. Exact candidate: 32,892,067 bytes,
SHA-256 `cf4586beee88d4b37edc66f80c99494aa8817b1a6a5f0d6efa1bc34ac81c30fc`;
release-manifest SHA-256
`4d0d923e6b05fb4897df41b5cf9f6072edbbdef9c362ca18f3d68682a798935e`. Uploaded
artifact `11543515153` is a 32,352,120-byte ZIP with SHA-256
`fb35ae70226356dbfd4bc726e8cb8accffc437f839fe92ddf9d7806e697dae39`. Its
payload hash and size were independently checked from the downloaded archive.

The exact candidate's standard-user child, job `113263868099`, failed before
product acceptance during C# `Add-Type` compilation. Bounded diagnostic
artifact `11543445828` reports `CS1503`, because a null literal was passed to
the `IntPtr` buffer parameter in
`GetUserProfileDirectory(token, null, ref size)`. Thus the e230 run does not
establish profile resolution or a product install result. Separate PR native
run `37760812504` failed earlier at Chocolatey HTTP 504 and skipped its child;
Windows installer run `37760812568` reached and reproduced the same C# compile
failure.

Focused source correction `0765a9c883c8f8f68be2ba978380d281c1e931b8` replaces
the null pointer with `IntPtr.Zero`. It is published to the same branch, with
tree `d3a0fa4c0f22949ef9c09c8f5f0e3110680afb0d`. Local Windows installer,
Windows native, and P045 matrix validators pass; the P045 unit suite has 20
passing tests. The local environment has no PowerShell/.NET/C# compiler.

On e232, Windows native run `37763335735` (`113264846876`), Windows installer
run `37763335943` (`113264848382`), Linux package run `37763335789`
(`113265092444`), and Linux clean-machine run `37763335821` were active. Its
adapter/contract job `113265198295` passed; exact-artifact producer
`113265198542` was building DEB/RPM inputs. The Windows native and installer
jobs were building runtime payloads; package job `113265092444` was running
while systemd check `113265092896` passed. P043 run `37763335918` had passed
Windows ownership/compiler security (`113264848340`), Windows acquisition
security (`113264848398`), and Linux invocation security (`113264847940`), with
remaining jobs pending. P044 run `37763335846` had passed docs/scope
(`113264851826`) while Windows Inno interruption job `113264852062` ran.
AppImage run `37763335897`, job `113264847498`, was building/reproducing. P045
aggregate run `37763336484` still had no jobs while pending. No e232 standard-
user, Linux native guest, or matrix result-v1 outcome is claimed.

The e230 P045 aggregate `37760812819` passed contract, source gates, and Linux
adapter/contract, but the artifact producers and standard-user/VM consumers
were canceled after e232 was published. Phase C job `113265318915` failed with
“No native clean-machine acceptance evidence was produced”; aggregate gate
`113269729210` also failed. Its truthful result-v1 artifact `11544575509`,
SHA-256 `31f383d976e8e7608db2712b27a69b975da24c1c74629794455d56e942812924`,
binds to e230 and has overall `BLOCKED`, zero candidates, and 32 `BLOCKED`
records. This superseded-head result is not an e232 result.
P043 completed all seven e230 jobs successfully (`37760812344`). Eight e230
P044 jobs passed, while its docs/scope and strict-format jobs were canceled as
the e232 rerun began; the full e230 P044 workflow therefore remained
incomplete. Neither run establishes the missing native GUI/logon/IPC, Linux
guest, power-cycle, or first-run requirements.

AppImage e230 producer run `37760812476`, job `113256476845`, passed build,
independent reproduction, manifest/runtime inspection, bounded QML smoke, and
current-user lifecycle. Its payload was 48,638,456 bytes, SHA-256
`f514a4fe4ca682856ac96a5b643ba2e1296925d01e294dcd8cc3906ce319ce64`; artifact
`11543110787` has envelope SHA-256
`e486b283a867b8bf5c82f2186e6846f7ab2ae9b8cc07c3d1c1fc7f557001220d`. This is
not clean graphical guest acceptance. The e232 AppImage producer also passed
in run `37763335897`, job `113264847498`: payload size 48,638,456 bytes, SHA-256
`b36b26adfa05e73c4a3babf15e7d74a2dd9d90ff36bc629c9c338e6ec8c3920c`; artifact
`11543577082` is a 48,029,788-byte envelope with SHA-256
`cf78e0a6989eba485ab03c9fe52a21f3840849ac53eab68fb248361d5bf13ee4`. Both are
producer/smoke evidence, not Ubuntu/Fedora graphical guest results.

## Historical e230 snapshot before bounded child diagnostics

The existing `feat/cross-platform-clean-machine-matrix` branch was recovered
without reset or history rewrite. The prompt's previously reported remote head
was `1215abe92af4d23936607f8f602d6a928d133bcf`; the actual recovered remote
head had advanced to `e230f3bbe04276ef2b21d2cfe13473993c6f9b46`, tree
`0f310a02cb10de4ac4056baf52aa6d3c17f50cb8`. PR #78 remains OPEN, DRAFT, and
unmerged. Main remains `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`.

The additional source commit `e230f3b` fixes the proven Windows standard-user
profile resolution failure. The child now resolves its profile through the
current process token, verifies the disposable account SID and non-admin status
before file I/O, and derives user folders from that profile. Exact-head PR
Windows native run `37760812504` stopped before compilation because Chocolatey
returned HTTP 504 for the required ZIP package; its standard-user job
`113259044192` was SKIPPED, and no child result is inferred. Same-head push run
`37760807332` and Windows installer run `37760812568` were still building.

P045 run `37760812819` passed contract (`113259813324`) and source gates
(`113259813685`). It then queued Linux exact-artifact producer `113262657791`,
Linux contract/adapters `113262657818`, and Windows candidate producer
`113262657889`. Linux clean-machine run `37760812308` passed contract/adapters
(`113256581969`), while its exact DEB/RPM artifact producer (`113256581934`)
was still in progress. Native consumer results and matrix result-v1 records
were still pending.

AppImage PR run `37760812476`, job `113256476845`, passed build, independent
reproduction, exact manifest/runtime inspection, bounded QML smoke, and
current-user lifecycle. Payload size/hash: 48,638,456 bytes,
`f514a4fe4ca682856ac96a5b643ba2e1296925d01e294dcd8cc3906ce319ce64`.
Artifact `11543110787` is a 48,029,821-byte envelope with digest
`e486b283a867b8bf5c82f2186e6846f7ab2ae9b8cc07c3d1c1fc7f557001220d`; this is
not a clean graphical guest PASS. Linux native-package run `37760812392` and
same-head push run `37760807307` had passed their systemd 249 checks, but
package/static/reproducibility gates were still running.

P043 run `37760812344` completed successfully with all seven Windows/Linux
security and structural jobs passing. P044 run `37760812448` remained in
progress: AppImage recovery, Windows ENOSPC, and Windows ownership/Inno
interruption had passed; other jobs had not completed. Broad Rust CI run
`37760807437` failed Windows compilation of Unix-only tests in
`crates/server-config/src/store.rs` and three Windows client IPC/process tests.
The store file is unchanged from main, so this remains an inherited general-CI
failure rather than an unrelated change to the P045 profile fix.

These are point-in-time status records. They preserve the semantic difference
between PASS, FAIL, BLOCKED, ERROR, and SKIPPED. Later exact-head outcomes are
appended below when observed; no pending job or unavailable native journey is
promoted to PASS.

## Recovery from the existing published branch

This continuation recovered `feat/cross-platform-clean-machine-matrix` at
`1215abe92af4d23936607f8f602d6a928d133bcf` (tree
`ed058edc0a493668be05cea116d37e3437d5a2a3`), preserving the initial four P045
commits and twelve linear continuation commits. Main remains
`a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`.

On that head, P045 run `37717057981` passed contract and source gates but
failed its required native evidence gate. Its artifact `11525427650` contains
32 BLOCKED rows, four exact producer identities, and no consumer PASS. Linux
source and package results, the Windows child failure, the VM SSH timeout, and
the QMP JSON framing cause are recorded with job identities in
[CROSS_PLATFORM_CLEAN_MACHINE_MATRIX.md](CROSS_PLATFORM_CLEAN_MACHINE_MATRIX.md).

Targeted corrections from those observed failures are under review in this
continuation: current-token Windows known folders; dpkg-deb extraction for its
valid zstd-compressed three-member package; QMP argument parsing; bounded guest
console logs; and attributable BLOCKED result-v1 records for guests that do
not reach the scenario adapter. Local P045, Linux adapter, Windows structural,
formatting, shell syntax, and shell lint checks pass. These local checks do not
replace the required exact-head hosted producer, consumer, and aggregate runs.

The first recovery publication was `fa72483910e17a296d99f0f5326cf486a682f28b`.
P045 run `37751023429` and direct Linux run `37751016329` failed before
creating jobs, so neither produced acceptance evidence. Actionlint identified
an invalid `runner.temp` context at reusable-workflow job scope. The state
directory is now exported from `$RUNNER_TEMP` inside the VM steps; the next
published head requires fresh hosted validation.

## Continuation: observed hosted outcomes on 931736c9

The existing four P045 commits and PR #78 are preserved. Source
`931736c9be13230bf7c3389462c084bedde6a227`, tree
`a1255123c5b468971ac526bd5a2ca67525d05744`, has now completed hosted runs:

- P045 run `37635214022`: source-gates job `112841481512` PASS; contract job
  `112841481885` FAIL because the runner lacked `rg`. The 32 counted issues
  were five navigation queries, four Windows-wording queries, and 23 release-freeze
  queries (11 facts plus 12 paths), not contradictions in BLOCKED evidence.
  Other search scans also could not execute. The job now installs ripgrep and
  docs validation fails immediately and explicitly if that prerequisite is absent.
  No documentation checks or acceptance evidence requirements are suppressed.
  Evidence-gate job `112854085633` correctly FAILS with 32 required BLOCKED
  records; Windows/Linux reusable jobs were skipped after contract failure.
- Windows native run `37635213226`, producer job `112839512450`: authenticated
  MSVC linker selection remains correct. Runtime closure failed on the Qt
  Schannel backend's `ncrypt.dll` import; Setup was not produced, and standard-user
  consumer job `112850583911` was skipped. Windows installer also failed there.
  Classification and system ownership are under review; no candidate is invented.
- Linux packages run `37635213211`, job `112839527206`: independent desktop
  release bytes differ, each 8,496,328 bytes, A SHA-256
  `7af3981dd351b11c823fedd43c2f644ce5c3d1b71e7319d65944f4bcd06ba092`,
  B SHA-256 `5d04b8227ff3fa634db3a2ae455b8090e63538c57ceba132ed2f3d0bac36bd61`,
  first difference at byte 1,380,551. Client and maintenance bytes match; separate
  Qt diagnostic job `112839527799` passed. Equality remains mandatory.
- AppImage run `37635213434` PASS, uploaded artifact `11489334126`. The entrypoint,
  independent AppImage byte comparison, manifest identity and bounded xcb smoke
  passed. This is producer evidence on this source, not INSTALL-JOURNEY-4 PASS
  on either qualified Linux target. Later source changes require affected reruns.
- Linux clean-machine run `37635213585`: static job `112839522604` and producer
  job `112839522972` PASS; native consumer `112862996980` and Phase C gate
  `112862999147` skipped. Ordinary PR runs retain P020's explicit manual-run
  condition; P045's reusable call already sets `run_native: true`, but was not
  reached after its contract failed. No capability guard is removed.

These completed results supersede the earlier pending status. Acceptance remains
withheld, PR #78 remains draft/unmerged, and P046 deferred. Fix and rerun source,
artifact and job identities are recorded only after observation in the PR report.

The Windows classifier now recognizes canonical `ncrypt.dll` as the Windows CNG
OS component documented by Microsoft's
[NCryptOpenStorageProvider requirements](https://learn.microsoft.com/en-us/windows/win32/api/ncrypt/nf-ncrypt-ncryptopenstorageprovider)
(minimum client Windows Vista, hence present on qualified Windows 11). It never
copies that OS DLL into the package. Import names containing a directory or
noncanonical characters cannot establish OS ownership; any packaged file bearing
a reviewed system DLL name is rejected. MSVCP140/VCRUNTIME140 and unknown imports
still require product runtime resolution and exhaustive PE validation. Regression
tests execute the actual classifier/resolver and reject an attacker-controlled
packaged NCRYPT.dll. The native producer observes only the OS-selected System32
file's Microsoft signature, version, hash and AMD64 identity, separately from
app-local CRT provenance. This is producer diagnostics, not Windows 11 acceptance.

The first NCrypt ownership preflight on source `1a2d7c6` rejected the legitimate
OS version resource's OriginalFilename. The diagnostic rerun on `064e9eb5`,
Windows native push run `37702883042`, job `113070262847`, observed fixed path
`C:\Windows\system32\ncrypt.dll`, company Microsoft Corporation, Valid signature
from Microsoft Windows, version `10.0.26100.1591`, OriginalFilename
`ncrypt.dll.mui`, SHA-256
`b0faca7d27c9bea959d6494372bb24daa594f3727e9319af5d0ac67cde530e55`.
The predicate now accepts only the two explicit NCrypt resource names; all fixed
path/signature/company/PE requirements remain. The producer is Windows Server,
not a tested Windows 11 build, and no Setup identity is inferred from preflight.

## Desktop generator differential diagnosis

The uploaded `synveil-desktop-repro-diagnosis` artifact `11493000259` belongs to
the passing pinned Qt 6.7.3 job. It does not contain the failing system-Qt job's
trees. That failing job's log reports the real linked mismatch but the diagnostic
searched only top-level generated directories; actual CXX-Qt files live beneath
`release/build/<crate>-<hash>/out`. The diagnostic now discovers those nested
Qt/CXX inputs and the native-package job uploads its own distinctly named report.
No missing generated coverage is treated as a deterministic-generation proof.

Using the real Qt 6.4.2 qmlcachegen and the unchanged product Main.qml, 12 runs
with cleared environment produced 12 different C++ hashes; 12 with
`QT_HASH_SEED=0` also produced 12 different hashes. The diffs are uninitialized
AOT register declaration order. Qt's
[6.4.2 generator source](https://github.com/qt/qtdeclarative/blob/v6.4.2/src/qmlcompiler/qqmljscodegenerator.cpp)
iterates `QHash<int, QHash<QQmlJSScope::ConstPtr, QString>>`: allocation-dependent
type-pointer keys explain why a fixed hash seed does not fix this order.
Two actual raw outputs compiled with the same source path produced differing
objects (503,248 versus 503,240 bytes). Canonicalizing only contiguous independent
uninitialized registers of the observed built-in types made the objects exactly
identical, 503,272 bytes, SHA-256
`d74b27095398f7c73c5ec1474e65a1619f1f3b1e42097a9f85b5d70b6b235f3d`.
These are diagnostic objects, not candidate desktop binaries or native evidence.

The build-host Qt wrapper applies that deterministic declaration order only to
the observed Qt 6.4.2 generator output, before C++ compilation. It preserves the
complete original generated source alongside normalized source for diagnosis.
Names/types, initializers and executable statements are retained; later Qt
versions, including AppImage Qt 6.7.3, are unaffected by this normalization.
Four source/mechanics regressions cover equivalence, initializer/statement
ordering, untouched unknown C++, and newline preservation. Twelve invocations
of the actual production wrapper retained 12 distinct raw originals but produced
one exact normalized hash. The independent full linked-byte gate remains
unchanged; fresh A/B release and hosted candidate results are still required.

Native DEB/RPM acceptance production now uses Ubuntu 24.04's distro Qt runtime,
matching the package's declared distro dependencies; AppImage retains its pinned
bundled Qt 6.7.3 after native packages are frozen. Package workflow checkouts bind
the true source head. Actual release manifests are printed only after validation
to retain source/version/size/hash identities in the producer logs.

## Windows runtime audit after NCrypt authentication

Source `927ccff5c418b413c1c91a2187fcb870ddfb9e20`, Windows native push run
`37703460923`, producer job `113072153986`, observed the serviced NCrypt file
as a HardLink with Archive attributes (not a reparse point). Microsoft signature,
canonical System32 path and AMD64 identity passed. Active compiler version
`19.51.36260.0`, linker `14.51.36260.0`, and ten authenticated x64 CRT DLLs were
selected. The real runtime build then reached the exhaustive PE audit and failed
on `vc_redist.x64.exe`: Microsoft's redistributable uses a 32-bit bootstrapper.
No Setup was produced; standard-user job `113076190027` was skipped.

Qt deployment now explicitly disables the compiler-runtime bootstrapper while
the builder stages authenticated app-local x64 CRT DLLs. Unexpected staged
redistributable installers are rejected, and every shipped PE remains subject
to the unchanged AMD64 audit. The source validator checks this actual app-local
closure policy rather than requiring the obsolete bootstrapper option. No
global CRT installer or elevation dependency substitutes for ordinary install.
Actual candidate creation, byte comparison and native consumption must rerun.

On source `9b8cebcd`, Windows native push run `37705499583`, producer job
`113078759532`, the real desktop/client build, NCrypt system identity,
app-local CRT closure and exhaustive AMD64 PE/import audit passed. Runtime
staging was exported, but archive creation failed because the hosted runner
lacked `zip`; Setup and standard-user consumption were not reached. The
Windows producer workflows now install the observed Chocolatey Info-ZIP 3.0
package and verify `zip -v` before compiling. The existing sorted file list,
`zip -X`, fixed timestamps and byte-equality gates remain unchanged. Producer
checkouts bind the actual source head; Rust CI's Windows packaging path also
records the same compiler/linker/CRT identities before use.

AppImage PR run `37705503526`, job `113078773574`, passed on `9b8cebcd`.
Its validated payload is `Synveil-0.1.0-x86_64.AppImage`, 48,638,456 bytes,
SHA-256 `b98ca6f6ae5f97cc819864390afff49f827b8a9cc32a2e62db34b5d3c82467d0`;
uploaded artifact `11519273954` is the ZIP envelope, not that payload size.
This source is superseded by the ZIP prerequisite fix; it remains producer
history and does not qualify either native AppImage matrix row.

Native-image preflight found that Fedora's active-release URL now returns 404.
The official archive directory lists the exact same 42-1.1 cloud image. Its
archived CHECKSUM has a valid signature from the already reviewed fingerprint
`B0F4950458F69E1150C6C5EDC8AC4916105EF944`; the actual 532,217,856-byte
download hashes to the unchanged locked digest
`e401a4db2e5e04d1967b6729774faa96da629bcf3ba90b67d8d9cce9906bec0f`.
Only the acquisition URL and checksum-reference URL change to that confirmed
official archive. Neither image identity nor target qualification is broadened.
Ubuntu's published checksum still matches its existing lock. URL reachability
and verified image acquisition are prerequisites, not native journey evidence.

Source `e2afbc75`, Windows native PR run `37707975218`, producer job
`113086975910`, passed complete runtime/ZIP closure, two byte-identical Setup
builds and the compiled `asInvoker` manifest inspection. The real
`SynveilSetup.exe` is 32,891,794 bytes, SHA-256
`ea3b707ae73e429ba3638b13493e34820400a6d5288afb2a8933418256004608`.
Artifact `11521500851` was downloaded and its actual payload, release-manifest
and toolchain hashes independently verified against the producer identity.
The producer reports Windows Server 2025 Datacenter build 26100, not Windows 11.

Consumer job `113093002782` then failed before execution because it searched
`candidate/target/windows-installer/p028-candidate.json`. The actual ZIP strips
the shared `target/` prefix and contains `windows-installer/...` and
`windows-toolchain.json`. Download now restores that prefix under
`candidate/target`; the existing size/hash/source/manifest/toolchain checks
remain unchanged. This workflow-wiring fix requires a fresh source-bound
producer/consumer attempt; the successful candidate above is historical.

Source `06f0e9b2`, Windows native run `37710195745`, producer `113094074992`,
passed full runtime closure and two equal Setup builds. Its candidate is
32,891,837 bytes, SHA-256
`16adc0278fe5e6c9d744d84eb5d66265d91dab9d972e87ba401cee3892fb4e2b`,
artifact `11521814347`. Consumer `113099513317` passed transferred candidate
authentication, then failed before installation during disposable account
creation. Cleanup masked the original error by deleting a nonexistent account.
The generated name was 21 characters, exceeding the Windows SAM limit of 20.
The harness now uses a 16-character name and deletes only after confirmed
creation. No ordinary-user product result was established by this attempt.

The separate Windows installer run `37710195662`, job `113094074238`, passed
runtime production but stopped after expected negative fixture exits: the
Actions PowerShell wrapper propagated the last rejected child's exit code.
The step now logs each required rejection and exits successfully only after
all four checks complete; accepting any fixture still throws. Positive Setup
production and native acceptance remain separate mandatory subsequent steps.

AppImage run `37710195685`, job `113094074560`, passed build, reproduction,
runtime and smoke checks on `06f0e9b2`: 48,638,456 bytes, SHA-256
`5dc033e0f4a6c1e90e7a16398069e224b2004c36ac73973490e0d2e8ba225400`,
artifact `11521976987`. P045 contract `113095898112` and strict source gates
`113095898287` in run `37710195944` also passed. These are superseded producer
and source observations after the subsequent harness changes, not native PASS.
Final rerun identities and every required matrix record remain in PR #78.

On source `fbb76557`, Linux packages run `37712282029`, main job
`113100952811`, completed independent clean-target-root A/B release builds:
desktop 8,496,328 bytes, SHA-256
`b720de2090de069178d47371577cb282a342726dad617ba0e2942231ed6f2bf1`;
client 18,514,776 bytes,
`c001fedc50768fad53af7bf67d9540db7d21f3b1e479d0ec77e55ba45c2cc02b`;
maintenance 10,142,656 bytes,
`63d4429bd17f3e80d84b71b44be1b6211d88af37f6553692a68cd4e41c9b7c51`.
DEB (8,470,980 bytes, `cb2d2cd3aebaa0446283161af9489840338b2d51109ed18a5c492ae185d10c8a`)
and RPM (12,126,571 bytes, `01f09ffc82e4d7a7bdf2b9ad77165dc2968e25199b91a3d3962d72dfca38073c`)
also matched their rebuilds exactly, as did both manifests. The job then
failed provenance parsing: the pre-existing link policy omits GNU build IDs,
but its manifest wrote an empty third field while the validator required hex.
The writer now records `none`; validation verifies actual absence through the
existing ELF audit and retains exact hash, size and source-fingerprint checks.
Three bounded ELF-format regressions cover round-trip, byte mutation, false
identity and rejection of an actual GNU build ID. These fixtures are not
product/native evidence. Downstream APT/DNF scope jobs were skipped on this
failed producer; no qualification is inferred from the package byte equality.

Windows producer `113100709238` in run `37712282003` passed on the same source:
Setup 32,890,891 bytes, SHA-256
`88cc746f3ef894e62539bfcf2f23004c8eeb862db884691a1bdd9205e03b93a4`,
artifact `11522592387`. Consumer `113106210706` authenticated it and started
the ordinary-user child, then failed before installation with an empty Path.
Its downloaded bounded logs (`11522407812`, verified ZIP digest
`894764259d6491edc121f2c306de862e10f54704d8aed17f157638b5ff2b1f61`)
preserve that error. The harness now resolves current-token Windows known
folders without requiring newly loaded Desktop/Start Menu directories to
exist, uses an owned unrelated working directory, and passes the actual Git
checkout identity to both child evidence scripts. The PowerShell runtime
verifier still throws on failure; an unset native LASTEXITCODE is no longer
misinterpreted as that script's result. No GUI/logon qualification is added.

Installer run `37712281987`, job `113100709176`, confirmed all negative
rejections and reproducible Setup/lifecycle builds, then failed the same
standard-user qualification. Linux Rust CI `113100921360` passed desktop and
client units but exposed the legacy CRT-policy test expecting
`--compiler-runtime`. It now requires `--no-compiler-runtime`, authenticated
app-local CRT, missing-runtime rejection and bootstrapper rejection. All three
existing packaging tests pass locally. These subsequent corrections supersede
the source-specific candidates above and require another exact-head rerun.

Status: **infrastructure/source implemented; acceptance checkpoint withheld.**

| Field | Observed value |
| --- | --- |
| Repository | https://github.com/nghianguyen150612/Synveil.git |
| Repository path | `/workspace/Synveil` |
| Expected baseline | `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc` |
| Actual fetched origin/main baseline | `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc` |
| Starting branch / HEAD | `work` / expected baseline |
| Worktree before changes | clean |
| Required branch | `feat/cross-platform-clean-machine-matrix` |
| Naming verification | local name checked before creation; no `codex/` prefix; remote must be queried again before PR |
| P043 ancestry | merge `3a927708142b4bee3a4582cc54e6dab1ea47fdc8` remains ancestor |
| P044 ancestry | merge `631d448773598d789a49d4a7f078d2d66a94e2c2` and head `b5f46b3fd9dfb4b5e7662cf5524e2faa559aaeb8` remain ancestors |
| Intervening main commits / existing P045 | none after expected baseline; no P045 branch/PR found before work |
| Source heads / final reviewed head | commit-derived identities are recorded by each producer and the observed PR completion record; no future self-SHA is invented |
| Producer runs / exact artifact IDs, bytes, hashes | unavailable before publication and observed producer execution |
| Windows target | Windows 11 AMD64; exact tested edition/version/build unavailable |
| Linux targets | Ubuntu 24.04 x86_64; Fedora 42 x86_64 |
| AppImage environments | both exact Linux targets; no universal Linux qualification |
| Debian | detected-not-qualified |
| Authoring host | Debian GNU/Linux 13 x86_64; not a native matrix target |
| Interactive Windows / real logon | unavailable in known hosted topology; runner enumeration denied with HTTP 403 |
| VM/image provenance | Linux locked digests in matrix; no qualifying consumer image/build observed locally |
| GUI / installed IPC / lifecycle / first-run | required; unavailable qualifying final-head records |
| Interruption/power-cycle | minimum remains `interruption-power-cycle`; no process-kill promotion |
| Reproducibility | required final-head hosted reruns; historical desktop mismatch not claimed resolved |
| Cleanup | local diagnostic files scoped to work/; native scenario cleanup not run |
| PR / hosted run/job IDs | unavailable before publication; append only observed identities |
| Merge / resulting main | not merged; acceptance withheld |
| Release boundary | P046 deferred; no RC, release or v0.2.0 tag |

## Contract audit and delivered changes

Audited the roadmap and installation product contract; P004 scenario/result
schemas and inventory; Linux/Windows native acceptance and P020/P028 manifests;
P043/P044 manifests and security/resilience boundaries; existing Linux adapter
and VM control plane; image lock and platform qualification policy; Windows,
Linux package and AppImage producer/consumer workflows and validators.

[The matrix](CROSS_PLATFORM_CLEAN_MACHINE_MATRIX.md) contains every required
row, scenario ID, exact target, minimum evidence and unsupported status.
`deploy/acceptance/p045-matrix-v1.json` is the machine-readable row ledger.
The validator checks that ledger against policy and validates/aggregates real
result-v1 evidence without promoting missing executions. Its adversarial tests
cover weaker evidence, mislabeled fixtures/offscreen sessions, stale sources,
tampered candidate bytes, wrong platforms/images, unknown schema fields,
missing assertions/rows, and process kill mislabeled as power-cycle.

The orchestration reuses platform workflows, passes the true source head to
producers/consumers, separately exercises AppImage and packages on both exact
Linux targets, preserves failed/blocked evidence, and fails when a required
record is missing. Setup records are diagnostic rather than duplicate results.
Artifact acquisition directories and per-scenario authorization-secret lifetime
are corrected; no password is logged. Native handlers remain closed dispatch.

Source/build fixes are limited to owning defects: Windows candidate tool selection
uses authenticated MSVC compiler/linker identities; runtime closure stages x64
Microsoft CRT DLLs app-locally, retaining strict import checks; Unix-only
credential permission/link tests are separately cfg-gated while portable tests
remain enabled. The actual pinned linuxdeploy tool reproduced its custom-AppRun
wrapper when a hook exists. The builder removes only the generated cosmetic Qt
hook in its private AppDir, restores the reviewed AppRun and compares exact
entrypoint bytes. No validator is weakened to accept a broken entrypoint.

These are source/tool-boundary fixes, not clean-machine PASS evidence. Exact
rebuilt Setup/DEB/RPM/AppImage bytes and native reruns remain mandatory.

## Evidence and withheld gates

Every applicable scenario must emit result-v1 with its definition digest, source,
times, observed host, exact candidate and manifest identities, capabilities,
completed steps/assertions, minimum evidence and separately observed cleanup.
The aggregate supplies only contract-valid BLOCKED preflight if no consumer
record exists; its observed host is not rewritten as Windows/Fedora/Ubuntu.

Windows graphical Setup, actual standard-user logon and installed named-pipe
acceptance remain blocked by environment/implementation. Linux reviewed GUI,
verified quick-install trust, native preservation/lifecycle and installed first-run
handlers remain incomplete. The VM controller can issue a hard power cut but
does not yet establish the required product mutation/reboot/reconciliation
assertions at every interruption point. First-run Connect needs a controlled live
server and SecretStore; Host needs qualified production runtime identities and
its real supported UI path. No hidden manual provisioning qualifies them.

P044 historical Rust/client/watcher, desktop reproducibility and PostgreSQL
failures are diagnostic until rerun on this source. Missing
`acceptance/p036/production-artifacts.json` is an explicit Host prerequisite,
not a fabricated production identity. No unrelated server fix or new ADR is
invented without a reproduced owning defect.

Source checks, hosted results, artifact identities and the PR publication are
reported only after observation. A blocked infrastructure PR stays open; green
source checks alone cannot complete P045 or unlock P046.

P046 deferred. No v0.2.0 release/tag is created.

## Local validation before publication

- `cargo fmt --all -- --check`: passed after formatting the scoped cfg change.
- Strict Clippy for `synveil-install-engine --all-targets --locked`: passed.
- `cargo test -p synveil-install-engine --locked`: passed, 526 tests.
- `cargo deny --locked check`: passed; existing advisory warnings retained.
- P004 contract tests: 21 passed; P020 adapter tests: 15 passed.
- P045 adversarial matrix/controller tests: 14 passed, including bounded timeout
  and reject-before-SSH checks. These are fixtures, not native acceptance.
- Docs, installer security, resilience, Windows installer/native, AppImage
  source, Linux platform/quick-install/DEB/RPM/first-launch and Windows rustflags
  validators: passed. Shell syntax, YAML parsing and `git diff --check`: passed.
- Broad workspace strict Clippy: blocked locally at missing `dbus-1.pc` /
  `libdbus-1-dev`; no broad source PASS is claimed. The orchestration installs
  native DBus/Qt prerequisites and gates candidate consumers on the real hosted
  format/Clippy/dependency checks.
- The Linux first-launch source validator still referenced removed Qt settings
  persistence. It now checks the canonical client startup preference store's
  versioned enabled/disabled parsing and the desktop's persist-before-apply path.
  This reconciles a stale validator without changing startup product behavior.
- The VM controller script was not executable at baseline despite direct workflow
  invocation. Its executable mode is corrected along with bounded child metadata
  reload, QEMU output isolation and bounded artifact staging.

No native graphical install, launch, named-pipe, first-run, live server or product
power-cycle PASS was produced locally. Candidate SHA-256/size/build identities
remain unavailable until actual hosted producers pass.

## Initial publication and diagnostic correction

- PR [#78](https://github.com/nghianguyen150612/Synveil/pull/78) is open as a
  draft from exactly `feat/cross-platform-clean-machine-matrix` to `main`.
  GitHub's branch API verified that name and initial head
  `9e1ea1cb7ca1adaa730014d20d8a0885b9495ff8` immediately before creation;
  initial tree `690c924ab26001a8eea0d82af5ced86c1ca32d37` matched publication.
- Initial Windows native push run `37632007612`, job `112828403852`, failed
  before runtime compilation in the new tool-identity preparation. The active
  hosted toolchain was Visual Studio 18, MSVC `14.51.36231`; its redist lacked
  the helper's assumed `Microsoft.VC143.CRT` directory. This was a P045 harness
  selection defect, not an installed product FAIL. The helper now selects
  exactly one generation-named CRT directory strictly inside the active x64
  redist root and records the observed name. It does not search System32 or
  silently select another SDK.
- Initial Windows installer push run `37632007596` also failed before Setup.
  Initial AppImage push `37632007346` and aggregate PR `37632164383` had not
  completed at correction time. Their superseded-head results are diagnostic
  only; the correction requires final-head reruns and no PASS is reused.

Acceptance remains withheld. No Windows 11 build, candidate hash, native
journey, power-cycle, merge or resulting main is inferred from these runs.

## Runtime import-audit correction

On corrected source `9c9580cc2ca17c65db6696f9bb1d4d9cb9d23bb6`, Windows native
push run `37632560247`, job `112830314309`, selected the active toolchain,
compiled the runtime, staged the app-local CRT and reached the exhaustive import
audit. The remaining failure was `UIAutomationCore.DLL` imported by the Qt
Windows platform plugin. Microsoft's
[UiaReturnRawElementProvider API requirements](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationcoreapi/nf-uiautomationcoreapi-uiareturnrawelementprovider)
identify this as the OS UI Automation DLL. It is now classified as a Windows
system API; MSVCP140/VCRUNTIME140 remain non-system and must be shipped. A focused
classifier regression checks that boundary. This is a corrected audit defect,
not a claim that the complete installed product or Windows 11 journey passed.

Toolchain diagnostics now retain observed source, compiler/linker versions and
hashes, active CRT generation/DLL hashes, and detected producer OS caption,
version/build/architecture even after a later candidate build failure. Producer
Windows Server identity cannot qualify the Windows 11 target. Subsequent exact
candidate builds and final-head reruns remain required.

Broad workspace strict Clippy subsequently passed locally using scenario-owned
DBus development metadata and Qt 6.7.3 staged under work/. The earlier missing
local dependency result is preserved above. This remains source evidence on
Debian 13, not native acceptance.

## AppImage producer diagnostic and smoke-harness correction

AppImage PR run `37632569430`, job `112830353029`, on the previous source head
passed actual AppImage construction, independent byte comparison, entrypoint
inspection and release-manifest byte identity. APPIMAGE-8 did not recur. Its
later CI smoke failed because the harness requested an `offscreen` plugin while
the self-contained production artifact correctly shipped `xcb` only.

The source/build validator now requires the graphical xcb plugin and the bounded
smoke uses that exact bundled plugin on a disposable Xvfb display. It does not
inject a developer Qt plugin, bundle offscreen only to green CI, or claim native
GUI acceptance. The workflow installs only display-harness prerequisites for
that check, checks out the true source head, and preserves candidate bytes and
manifest even after a later failure. Fresh final-head AppImage/native reruns are
still required; the superseded run is diagnostic only.

The focused Linux notify watcher test executed one real test and passed locally
on the unchanged Rust source (`linux_notify_backend_handles_live_rename_move_delete_symlink_and_editor_save`).
This does not establish installed synchronization or acceptance on either
qualified Linux target. The historical hosted move-intent failure is not
converted into a native PASS.
