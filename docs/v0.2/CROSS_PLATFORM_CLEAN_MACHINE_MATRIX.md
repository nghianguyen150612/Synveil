# Cross-platform clean-machine matrix (P045)

## Current continuation: e233 Userenv import rerun

At the e233 observation recorded here, the existing branch was at
`32a7f55cf4a5b754c0b5732a5339b07a51f1cb29` (tree
`e3dba86a48dacdb112753dd48abf470352cb0a53`). PR #78 was OPEN, DRAFT, and
unmerged; main was `a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`.

### Latest e233 hosted results and cross-platform CI findings

P045 aggregate run `37765484729` passed contract job `113273965143` and source
gates job `113273965560` (format, strict workspace Clippy, and cargo-deny).
Source-gates artifact `11544926493` records PASS for source
`32a7f55cf4a5b754c0b5732a5339b07a51f1cb29`. The aggregate Windows candidate
job `113277693547` and Linux artifact producer `113277693805` were still in
progress; Linux acceptance contract/adapters job `113277693759` passed. No
e233 result-v1 matrix artifact or native consumer result was available yet.

The independent Windows native run `37765484348`, producer job `113271991216`,
passed candidate creation and two-build reproducibility. Its exact
`SynveilSetup.exe` is 32,889,573 bytes, SHA-256
`1d603fe8d1aac7f983db21fc7bf50d38bdbdb39f4cd903756f8de9cde32f3115`, bound to
source `32a7f55`. Release-manifest SHA-256 is
`09591e2c48d3a77ff8484ace4626e262004bc33850c16fc74fd24568487aa55c`; toolchain
identity SHA-256 is
`4bfc06ac04dcd026937e3f289cc7ccd1426d528ec9d818f92783716620c0769d`. Candidate
artifact `11545650446` is a 32,349,622-byte ZIP with SHA-256
`dbe913f3695a8505e4a590eeb78bb794ceb889e93a1b2a09449cc19d8709a243`; the
archive was independently downloaded and its payload size, hash, manifest hash,
and toolchain identity hash matched the producer record. Standard-user child
`113278758080` was queued, so this is producer evidence only. Windows installer
run `37765484543`, job `113272247463`, was still building its runtime payload.

Linux clean-machine run `37765484365` passed adapter/contract job
`113272145813`; producer `113272145478` was still gating independent release
binary reproducibility. Linux package run `37765484360` had systemd 249 check
`113272566017` pass while Qt reproducibility job `113272565731` and DEB/RPM
producer/test job `113272566080` were still running. AppImage run
`37765484292`, job `113271991388`, passed independent build reproduction,
inspection, bounded smoke, and current-user lifecycle. Its payload is 48,638,456
bytes, SHA-256
`349cf8ab0b085d1edb0f318d7aa8f192486567a4305f6c0a8cf40472bc289436`; artifact
`11544284724` is a 48,029,789-byte ZIP with SHA-256
`aed210c0b3d8a03763d08eacfb211f52bbda1d1bf973774498cabb5914729663`. This
does not qualify a graphical Ubuntu or Fedora guest.

P043 run `37765484343` completed all seven jobs successfully. P044 run
`37765484302` had passed Windows ownership/interruption, journal durability,
Windows ENOSPC, and strict-format/installer-engine jobs; its six remaining jobs
were queued, so the full run was incomplete at this observation.

Separate Rust CI run `37765484394` exposed portability failures on e233.
Windows `Check` job `113272072884` and `Test` job `113272072944` could not build
the Unix-only tests in `crates/server-config/src/store.rs`. Windows native UI
job `113272072305` had five client-config tests fail because their fixtures used
Unix `/tmp` roots; its named-pipe command test reached `ping` but received
`Frame(Closed)`. macOS job `113272072511` failed three Unix IPC tests because
the temporary-directory path contained a symlink ancestor and the production
endpoint policy correctly rejected it as `UnsafeEndpoint`. This follow-up gates
the Unix-only test module, uses native temporary paths in Windows fixtures,
canonicalizes the Unix IPC fixture roots, and arms the next named-pipe instance
before awaiting a client. Hosted confirmation is pending. This authoring
environment has no Cargo, rustc, or rustfmt, so Rust tests cannot be run locally.

The earlier package failure from run `37717057722`, job `113115833800`, was not
a corrupt DEB. Its real three-member `ar` layout was
`debian-binary`, `control.tar.zst`, `data.tar.zst`; the test helper incorrectly
required `.gz` names and passed `data.tar.gz` to `tar`. Commit `fa72483` changed
extraction to `dpkg-deb --extract` and accepts deterministic `gz`, `xz`, or
`zst` members. The e233 package test result is still pending.

On the prior head, Linux consumer jobs `113124991887`, `113124991905`,
`113124991930`, and `113124991959` all reported KVM unavailable and fell back
to TCG, then failed their 900-second guest boot/readiness step. SSH to the guest
was refused afterward. The old job source did not preserve serial/QEMU logs, so
the underlying guest boot cause is unknown and no Linux product journey ran.
Their additional QMP `JSONDecodeError: Extra data` came from the old Bash
`${2:-{}}` default-argument expansion appending an extra `}` to the JSON command.
Commit `fa72483` separated the empty-argument default and added bounded boot
diagnostics; the e233 Linux consumer run has not reached a guest result yet.

On e232 (`0765a9c`), Windows native run `37763335735` passed candidate
production and two-build reproducibility. The exact `SynveilSetup.exe` was
32,889,202 bytes, SHA-256
`8cd3147214986f1a1ab125cffe06d1c51057466938413442d3cbb95146dcd5ea`; the
32,349,256-byte ZIP candidate artifact `11543558488` has SHA-256
`6e50a4be6c312f32a92d0cfd921a4c076a0bb5ba4b994dcad1339a9fb8b16ff8`. The
archive was downloaded and its exact payload hash and size independently
verified. Release-manifest SHA-256 is
`612163222f56bc72ebdee28c93a911b6da77bb038e693ab034757a8ff6ac8129`; toolchain
identity SHA-256 is
`db167c3ebaa3ae9c7cc0a52a60505c0e5f904d543329182aa1156b54180ec2f3`.

The e232 standard-user child `113270883106` failed after C# compilation, before
profile/SID assertions: bounded artifact `11544550956` (473 bytes, SHA-256
`d95fdce5d01ade7a4c3a93699bf703a10f05a1424a9bbae69c24ffa3b0f809d3`) says
Windows could not find `GetUserProfileDirectory` in `advapi32.dll`. Microsoft
documents the Unicode entry point as `GetUserProfileDirectoryW` in
`Userenv.dll` ([API reference](https://learn.microsoft.com/en-us/windows/win32/api/userenv/nf-userenv-getuserprofiledirectoryw)).
Commit `32a7f55` corrects that module and entry point and adds a static contract
check. Local Windows installer/native and matrix validators pass; all 20 P045
unit tests pass. This authoring environment has no PowerShell/.NET/C# compiler.

Exact e233 runs were queued or in progress at this observation: Windows native
`37765484348`, Windows installer `37765484543`, Linux packages `37765484360`,
Linux clean-machine `37765484365`, AppImage `37765484292`, P043 `37765484343`,
P044 `37765484302`, and P045 aggregate `37765484729`. Windows native candidate
job `113271991216` and Linux clean-machine producer `113272145478` were running;
its contract/adapters job `113272145813` passed. P045 had started contract job
`113273965143` and source-gates job `113273965560`, both queued. No e233
standard-user result or result-v1 matrix record is claimed.

On e232, Linux clean-machine adapter/contract job `113265198295` passed, but
artifact build `113265198542` was still in progress. Linux package and Qt
reproducibility jobs (`113265092444`, `113265092857`) had not completed. Its
AppImage producer `37763335897`/`113264847498` passed build, reproduction,
inspection, smoke, and current-user lifecycle; the exact payload was 48,638,456
bytes, SHA-256
`b36b26adfa05e73c4a3babf15e7d74a2dd9d90ff36bc629c9c338e6ec8c3920c`; artifact
`11543577082` envelope SHA-256
`cf78e0a6989eba485ab03c9fe52a21f3840849ac53eab68fb248361d5bf13ee4`. It does
not qualify Ubuntu or Fedora graphical guest acceptance.

P043 completed all seven e232 jobs successfully in run `37763335918`. Eight
P044 jobs passed in run `37763335846`; strict-format and journal-durability
jobs were canceled as e233 was published, so that full run remained incomplete.
The e232 P045 aggregate `37763336484` passed contract (`113270644133`); its
source-gates job `113270643844` was canceled after e233 was published; its
Windows/Linux jobs were skipped and evidence gate `113272284297` remained
queued before it failed. It uploaded result-v1 artifact `11544328344` (SHA-256
`284353f2919e4f22967440c4217b55acef7e8a8e0dacad8337caaa7e6a5cb2a0`), bound to
e232, with overall `BLOCKED`, zero candidates, 32 `BLOCKED` rows, and no
source-gates record. This is not an e233 result. The e232 Linux clean-machine
producer `113265198542` was canceled after its contract/adapters job
`113265198295` passed. Windows installer e232 run `37763335943` was canceled.
Windows 11 GUI/logon/IPC, Linux guest GUI, hard power-cycle, and live first-run
server evidence remain unestablished.

## Current continuation: exact Windows compiler failure and e232 rerun

The existing branch advanced linearly from the prompt's reported head
`1215abe92af4d23936607f8f602d6a928d133bcf` through
`e230f3bbe04276ef2b21d2cfe13473993c6f9b46`. Its exact-head Windows candidate
passed production and reproducibility in push run `37760807332`, producer job
`113256459800`. `SynveilSetup.exe` was 32,892,067 bytes, SHA-256
`cf4586beee88d4b37edc66f80c99494aa8817b1a6a5f0d6efa1bc34ac81c30fc`, bound to
source `e230f3b`; release-manifest SHA-256 was
`4d0d923e6b05fb4897df41b5cf9f6072edbbdef9c362ca18f3d68682a798935e`. Candidate
artifact `11543515153` is a 32,352,120-byte ZIP with SHA-256
`fb35ae70226356dbfd4bc726e8cb8accffc437f839fe92ddf9d7806e697dae39`. The
downloaded ZIP independently matched the producer's payload size and hash.

Its standard-user child `113263868099` failed during the C# `Add-Type` compile
before the profile or SID assertions ran. Bounded artifact `11543445828`
(567-byte ZIP, SHA-256
`062d1f5e0722db59e3887ade2868d3e5f494a4e9b869bafdf2918a9da1b8f802`) contains
the child logs. The compiler reported `CS1503: Argument 2: cannot convert from
'<null>' to 'nint'` for `GetUserProfileDirectory(token, null, ref size)` in
`scripts/windows-known-folders.ps1`. This is a harness compile failure; the
ordinary-user product acceptance did not run. The PR Windows native run
`37760812504` separately failed its Chocolatey ZIP prerequisite with HTTP 504
and skipped its child. Windows installer run `37760812568` reached the same
standard-user child and failed on the same C# compile error.

Focused correction `0765a9c883c8f8f68be2ba978380d281c1e931b8` passes
`IntPtr.Zero` to the native sizing call. It is published on the existing branch;
its tree is `d3a0fa4c0f22949ef9c09c8f5f0e3110680afb0d`. Local Windows installer,
Windows native, and P045 matrix validators pass, as do all 20 P045 unit tests.
No PowerShell/.NET/C# compiler is installed in this authoring environment, so
hosted compilation remains the decisive check.

Exact-e232 hosted reruns are underway: Windows native run `37763335735`, job
`113264846876`; Windows installer run `37763335943`, job `113264848382`; Linux
packages run `37763335789`, job `113265092444`; and Linux clean-machine run
`37763335821`, whose adapter/contract job `113265198295` passed while artifact
producer `113265198542` was building DEB/RPM inputs. The Windows native and
installer jobs were still building runtime payloads. The package job was
running and systemd check `113265092896` passed. P043 run `37763335918` had
passed Windows ownership/compiler security (`113264848340`), Windows
acquisition security (`113264848398`), and Linux invocation security
(`113264847940`), with remaining jobs pending. P044 run `37763335846` had
passed its docs/scope gate (`113264851826`); its Windows Inno interruption job
`113264852062` was running. AppImage run `37763335897`, job `113264847498`, was
building/reproducing. P045 aggregate run `37763336484` still had no jobs while
pending. No e232 standard-user, Linux guest, or result-v1 aggregate outcome is
claimed.

The e230 P045 run `37760812819` had passed contract (`113259813324`), source
gates (`113259813685`), and Linux adapter/contract (`113262657818`), but its
artifact producers and standard-user/VM consumers were canceled after e232 was
published. Phase C evidence job `113265318915` then failed because no native
acceptance artifacts existed; the log explicitly says “No native clean-machine
acceptance evidence was produced.” Aggregate `evidence-gate` `113269729210`
also failed, but uploaded truthful result-v1 artifact `11544575509` (SHA-256
`31f383d976e8e7608db2712b27a69b975da24c1c74629794455d56e942812924`). It binds
to source `e230f3b`, has result `BLOCKED`, zero candidates, and 32 records, all
`BLOCKED`. This superseded-head result is not an e232 result. The e230 direct
Windows child failure above is diagnostic, not acceptance.

P043 completed all seven jobs successfully on e230 in run `37760812344`; its
e232 rerun is active. Eight P044 jobs passed on e230 in run `37760812448`, but
its docs/scope and strict-format jobs were canceled as the e232 rerun began;
P044 therefore had no completed full-run result on e230. No Windows 11 GUI,
genuine-logon, installed IPC, Linux graphical guest, hard power-cycle, or live
first-run server evidence was established.

The AppImage producer passed on e230 in run `37760812476`, job `113256476845`:
48,638,456-byte payload SHA-256
`f514a4fe4ca682856ac96a5b643ba2e1296925d01e294dcd8cc3906ce319ce64`, artifact
`11543110787` envelope digest
`e486b283a867b8bf5c82f2186e6846f7ab2ae9b8cc07c3d1c1fc7f557001220d`. It is
producer/smoke evidence only. On e232, AppImage run `37763335897`, job
`113264847498`, also passed build, independent reproduction,
manifest/runtime inspection, smoke, and current-user lifecycle. Payload size is
48,638,456 bytes, SHA-256
`b36b26adfa05e73c4a3babf15e7d74a2dd9d90ff36bc629c9c338e6ec8c3920c`;
artifact `11543577082` is a 48,029,788-byte envelope with SHA-256
`cf78e0a6989eba485ab03c9fe52a21f3840849ac53eab68fb248361d5bf13ee4`. Neither
producer run qualifies Ubuntu/Fedora graphical clean-guest acceptance.

## Historical e230 pending snapshot before child diagnostics

This continuation resumed the existing branch and PR #78 from its published
history. The original externally reported branch head was
`1215abe92af4d23936607f8f602d6a928d133bcf`; the actual recovered branch had
already advanced linearly through `e230f3bbe04276ef2b21d2cfe13473993c6f9b46`
(tree `0f310a02cb10de4ac4056baf52aa6d3c17f50cb8`). Main remains
`a560a40c8f2fe5b91db27c1b63fa0b0ab87173bc`. PR #78 is OPEN, DRAFT, and
unmerged. No branch was recreated and no published history was rewritten.

On `e230f3b`, the exact-head P045 aggregate
[run 37760812819](https://github.com/nghianguyen150612/Synveil/actions/runs/37760812819)
passed contract job `113259813324` and source-gates job `113259813685`. The
aggregate then queued exact-artifact Linux producer `113262657791`, Linux
contract/adapters `113262657818`, and Windows candidate producer `113262657889`.
Linux clean-machine run `37760812308` had passed acceptance contract/adapters
job `113256581969`; exact-artifact producer `113256581934` was still building
DEB/RPM inputs. The result-v1 aggregate and native consumer rows were not yet
available. These are pending observations, not PASS records.

The new Windows fix is commit `e230f3b`: it obtains the running process's
profile directory from its token, asserts that the child runs as the disposable
non-administrator SID before file I/O, and derives LocalAppData, Programs, and
Desktop from that token profile. The exact-head PR Windows native run
`37760812504` failed before compilation at its Chocolatey ZIP prerequisite:
the community feed returned HTTP 504 while looking up Info-ZIP 3.0. The
standard-user job `113259044192` was consequently SKIPPED, not failed and not
passed. Bounded prerequisite artifact `11541749815` preserves producer
diagnostics. The independent push run `37760807332` had installed ZIP and was
still building the runtime; no ordinary-user child result was yet available.
Windows installer PR run `37760812568`, job `113256477022`, was also building
its runtime payload. Thus no standard-user result on the new profile fix had
yet been established at this observation point.

AppImage producer run `37760812476`, job `113256476845`, completed successfully:
build, independent byte reproduction, exact manifest/runtime inspection,
host-decontaminated QML smoke, and current-user integration lifecycle all
passed. Payload `Synveil-0.1.0-x86_64.AppImage` is 48,638,456 bytes with SHA-256
`f514a4fe4ca682856ac96a5b643ba2e1296925d01e294dcd8cc3906ce319ce64`;
artifact `11543110787` is the 48,029,821-byte ZIP envelope with SHA-256
`e486b283a867b8bf5c82f2186e6846f7ab2ae9b8cc07c3d1c1fc7f557001220d`. This is
producer/smoke evidence only; neither Ubuntu nor Fedora graphical AppImage
acceptance is established by it. The matching push AppImage job
`113256459756` also completed successfully.

Linux package run `37760812392` and same-head push run `37760807307` had passed
their systemd 249 sysusers/tmpfiles checks. Their package build, static
packaging-unit, and independent release reproducibility jobs were still in
progress; no full suite or exact A/B equality result was yet recorded.

P043 run `37760812344` completed successfully: all seven jobs passed, including
Windows ownership/options/reparse/compiler security (`113256476686`), Windows
and Ubuntu acquisition/channel/downgrade security (`113256476928`,
`113256477021`), crypto dependency policy (`113256476958`), Linux lifecycle
security (`113256477003`), Linux invocation/path/hook security (`113256477029`),
and structural/docs validation (`113256477019`). P044 run `37760812448` was
still running; AppImage partial integration recovery (`113256476354`), Windows
deterministic ENOSPC (`113256476717`), and Windows ownership/Inno interruption
(`113256476814`) had passed. Other P044 jobs had not yet completed, so its
overall result remained pending.

Broad Rust CI run `37760807437` exposed Windows test-compilation failures in
`crates/server-config/src/store.rs` (`std::os::unix` and `PermissionsExt`) and
three client IPC/process tests. `store.rs` is byte-for-byte unchanged from
`main`, so this is recorded as an inherited broad-CI failure outside the P045
profile fix; no unrelated source change is made here.

## Recovery observations on published head `1215abe92af4d23936607f8f602d6a928d133bcf`

PR #78 and all prior commits were recovered from the existing branch. The
latest P045 aggregate, [run 37717057981](https://github.com/nghianguyen150612/Synveil/actions/runs/37717057981),
passed contract job `113116960452` and source gates `113116960722`; Windows
producer `113118564763` and Linux artifact producer `113118565076` passed.
The aggregate artifact `11525427650` records 32 BLOCKED preflight rows and no
PASS rows. Its producer identities are Windows Setup `SynveilSetup.exe`
(32,889,807 bytes, SHA-256
`87c183deba8d82f9dcade890022a2cded66550db6b18eaab6bc179da4f2dcc31`),
Ubuntu 24.04 DEB (8,452,068 bytes, `aa803751016a1c95479146d0ebc3a5c35b1882a778b8690f30d3da9088f6d510`),
Fedora 42 RPM (12,104,648 bytes, `12a89a4b6ee4837a0f98450c6e6389ad759c52199e426fdfed77e520e3b4f982`),
and AppImage (48,871,928 bytes,
`dbaf4b78d4d8b47ee81c765c946818b69e6448d3de3976fd3d9f000ebb89ea1d`).
The manifest digest is
`acda01e006cf075712a414532d1aa25e2bc94749d53001d81174a6f481039355`.

Windows standard-user job `113123725250` failed in run `37717057981`; bounded
child artifact `11524998665` identifies the first assertion failure: the
standard-user child attempted to create `C:\Users\runneradmin\AppData` and
received Access Denied at `test-windows-per-user-installation.ps1:88`. The
child inherited the runner profile environment despite `-Credential
-LoadUserProfile`. Known-folder resolution now calls `SHGetKnownFolderPath`
for the current process token, retaining per-user token, ACL, registry,
runtime, and preservation assertions. This fix has local structural validation;
the next hosted consumer must establish its actual result.

Linux package run `37717057722`, job `113115833800`, built both native packages
and then failed 7 of 26 `linux_native_packaging_units` assertions because the
test helper assumed `control.tar.gz` and `data.tar.gz`. The real deterministic
DEB members were `debian-binary`, `control.tar.zst`, and `data.tar.zst` from
the installed dpkg-deb. The tests and package extraction helpers now delegate
compression handling to dpkg-deb and continue to enforce the exact three-member
order, recognized compression suffixes, package paths, metadata, modes, and
payload parity. The separate full desktop reproducibility job `113115833805`
passed. This package-test fix still requires the next hosted full-suite result.

All four Linux consumers in run `37717057981` authenticated the pinned image
and frozen producer artifact, then failed before a scenario adapter result:
Ubuntu DEB `113124991887`, Ubuntu AppImage `113124991930`, Fedora RPM
`113124991905`, Fedora AppImage `113124991959`. QEMU logged that KVM was
unavailable and selected TCG. Each job spent exactly the 900-second SSH boot
bound before later probes received connection refused. The uploaded bundles
omitted the serial console and QEMU process logs, so the boot cause cannot be
assigned to the image, cloud-init, networking, or TCG performance from this
attempt. A separate `JSONDecodeError: Extra data` came from Bash expanding
`${2:-{}}` into supplied QMP JSON with an extra closing brace; direct shell
reproduction confirmed the framing defect. QMP default parsing is corrected,
and the workflow now preserves bounded serial/QEMU logs and writes schema-valid
BLOCKED records when a guest never reaches the adapter. No VM scenario passed.

Other same-head results: installer security run `37717057661` PASS; resilience
run `37717057849` PASS; standalone AppImage build/smoke `37717057696` PASS;
ordinary Linux clean-machine run `37717057837` succeeded at workflow level,
with job-level native evidence still requiring inspection. Rust CI
`37717057800`, Windows installer `37717057758`, Windows native `37717057729`,
and Linux packages `37717057722` failed; failures remain scoped to their
observed jobs. These results never qualify Windows 11 GUI/logon/IPC, Linux
installed journeys, first-run dependencies, or power-cycle recovery.

## First recovery publication attempt on `fa72483910e17a296d99f0f5326cf486a682f28b`

The first publication triggered P045 run `37751023429` and direct Linux clean-
machine run `37751016329`. Both ended before creating any jobs, so they produced
no source-gate, package, consumer, or matrix evidence. Actionlint identified
the workflow-definition error: `runner.temp` was used in the reusable Linux
workflow's job-level `env`, where the `runner` context is unavailable. The
workflow now exports `$RUNNER_TEMP/vm` inside each VM-control step. This
correction is validated locally by actionlint; new hosted runs on the next
head must establish the actual workflow and native outcomes.

Completed hosted evidence on source `931736c9be13230bf7c3389462c084bedde6a227`
is diagnostic until the required native rows qualify. P045 run `37635214022`
passed source gates but failed docs validation because ripgrep was absent; its
32 BLOCKED aggregate records remain valid. The documentation prerequisite is
now explicit. Windows run `37635213226` failed before Setup on `ncrypt.dll` from
the Qt Schannel backend. Linux packages run `37635213211` failed exact desktop
byte equality; client/maintenance and the separate Qt job matched. AppImage run
`37635213434` passed build/reproduction/manifest/smoke checks. Linux clean-machine
run `37635213585` passed its producer/static jobs but skipped native consumers
under the ordinary PR manual-run condition. The P045 caller requests native
execution and was prevented by contract failure. None is a native row PASS.

Status: **infrastructure/source implemented; acceptance checkpoint withheld.**

Continuation on `9b8cebcd`: Windows native run `37705499583`, producer job
`113078759532`, passed actual runtime closure and AMD64 checks, then failed
archive creation because `zip` was absent. The Windows producers now install
and verify Info-ZIP 3.0 before building; no archive audit is removed.
AppImage PR run `37705503526`, job `113078773574`, passed producer checks:
48,638,456-byte payload, SHA-256
`b98ca6f6ae5f97cc819864390afff49f827b8a9cc32a2e62db34b5d3c82467d0`.
Those attempts remain historical after the prerequisite fix. Exact rerun
identities and subsequent native results are recorded in PR #78; no earlier
producer PASS is promoted into a final-head native PASS.

Fedora image acquisition now uses the confirmed official archive because the
active-release URL returned 404. The archived signed CHECKSUM was verified
against the existing reviewed Fedora 42 fingerprint, and the actual downloaded
532,217,856-byte image matched the unchanged locked SHA-256. The exact platform
and image digest remain the same; acquisition does not prove a native journey.

Windows producer job `113086975910` in run `37707975218` passed on `e2afbc75`
and produced a reproducible 32,891,794-byte Setup, SHA-256
`ea3b707ae73e429ba3638b13493e34820400a6d5288afb2a8933418256004608`.
The actual downloaded archive confirms all producer identity bindings and
`asInvoker`, but consumer `113093002782` failed on a stripped directory prefix
before running acceptance. Its download destination now restores `target/`
without weakening authentication. Windows Server producer/silent scope remains
separate from the required Windows 11 graphical/logon/IPC qualification.

On `06f0e9b2`, Windows producer `113094074992` in run `37710195745` passed and
produced Setup (32,891,837 bytes, SHA-256
`16adc0278fe5e6c9d744d84eb5d66265d91dab9d972e87ba401cee3892fb4e2b`).
Consumer `113099513317` authenticated the transferred candidate but failed
before product installation while creating its disposable account; cleanup
masked the original error. The invalid 21-character account name is shortened
to 16, and cleanup requires confirmed creation. The separate installer job's
expected negative exit handling is also corrected without allowing any fixture.
The exact-head AppImage producer passed (48,638,456 bytes, SHA-256
`5dc033e0f4a6c1e90e7a16398069e224b2004c36ac73973490e0d2e8ba225400`).
These attempts are superseded after the harness fixes; final consumer and
aggregate observations in PR #78 control acceptance.

Linux system-Qt fresh A/B builds on `fbb76557` passed exact whole-binary and
DEB/RPM equality (`37712282029`, `113100952811`). The producer then failed on
the absent GNU build-ID manifest field; it now records verified absence as
`none` while retaining all ELF/source/byte checks. Windows candidate production
also passed (`37712282003`, `113100709238`), but its ordinary-user child failed
early on an empty profile-folder path. Current-token OS known-folder resolution
and explicit checkout provenance correct that harness failure; Windows 11
graphical/logon acceptance remains withheld. The legacy CRT-policy test is
aligned with authenticated app-local deployment and still requires rejection
of missing runtime and elevated bootstrapper payloads. These fixes supersede
the earlier producer attempts; PR #78 records final reruns and the full matrix.

This is the current P045 target ledger. No target is newly accepted by this
document. The scenario JSON and result-v1 schema remain normative. P020/P028
historical records are preserved; P044 process interruption cannot close a
power-cycle gate. P046 remains deferred.

## Exact target and artifact ledger

Linux qualification comes exclusively from
[linux-platforms-v1.json](../../deploy/install/linux-platforms-v1.json).
The profile named `debian-x86_64` qualifies Ubuntu 24.04 only. Windows targets
Windows 11 AMD64; a Windows Server producer is build evidence only.

| Required row | Exact target | Architecture | Scenarios | Result / evidence |
| --- | --- | --- | --- | --- |
| `windows-11-amd64-setup` | Windows 11; tested version/build unavailable | `x86_64` | `INSTALL-JOURNEY-1`, `INSTALL-JOURNEY-6`, `INSTALL-JOURNEY-7`, `INSTALL-JOURNEY-8`, `FIRST-RUN-1`, `FIRST-RUN-2` | BLOCKED; required native evidence unavailable |
| `ubuntu-24.04-x86_64-deb` | Ubuntu 24.04 | `x86_64` | `INSTALL-JOURNEY-2`, `INSTALL-JOURNEY-6`, `INSTALL-JOURNEY-7`, `INSTALL-JOURNEY-8`, `FIRST-RUN-1`, `FIRST-RUN-2` | BLOCKED; required native evidence unavailable |
| `ubuntu-24.04-x86_64-appimage` | Ubuntu 24.04 | `x86_64` | `INSTALL-JOURNEY-4`, `INSTALL-JOURNEY-6`, `INSTALL-JOURNEY-7`, `INSTALL-JOURNEY-8`, `FIRST-RUN-1`, `FIRST-RUN-2` | BLOCKED; required native evidence unavailable |
| `ubuntu-24.04-x86_64-quick-install` | Ubuntu 24.04 | `x86_64` | `INSTALL-JOURNEY-5` | BLOCKED; required native evidence unavailable |
| `fedora-42-x86_64-rpm` | Fedora 42 | `x86_64` | `INSTALL-JOURNEY-3`, `INSTALL-JOURNEY-6`, `INSTALL-JOURNEY-7`, `INSTALL-JOURNEY-8`, `FIRST-RUN-1`, `FIRST-RUN-2` | BLOCKED; required native evidence unavailable |
| `fedora-42-x86_64-appimage` | Fedora 42 | `x86_64` | `INSTALL-JOURNEY-4`, `INSTALL-JOURNEY-6`, `INSTALL-JOURNEY-7`, `INSTALL-JOURNEY-8`, `FIRST-RUN-1`, `FIRST-RUN-2` | BLOCKED; required native evidence unavailable |
| `fedora-42-x86_64-quick-install` | Fedora 42 | `x86_64` | `INSTALL-JOURNEY-5` | BLOCKED; required native evidence unavailable |

Repair/uninstall/interruption and both first-run scenarios appear on each
applicable product format according to the normative families. The quick-install
row uses the same DEB/RPM candidate; its subsequent lifecycle/first-run gates
are listed in the native-package row. Host still requires its qualified runtime
bundle, PostgreSQL and product UI; family applicability alone does not establish
those capabilities. Missing dependencies block rather than create acceptance.

`UPGRADE-TEMPLATE-1` remains blocked/not release-applicable: no version-distinct
v0.2 release candidate exists. Its mechanics fixtures are diagnostic only.

## Machine provenance and blocked execution

| Target | Required provenance | Observed qualifying machine / OS build |
| --- | --- | --- |
| ubuntu-24.04-x86_64 | pinned image SHA-256 `6a81c37564db9b1ee84e141922625e1d7c5b389b99bb3c572e0243607d5bb4d2` | unavailable; pinning authenticates input, not an acceptance PASS |
| fedora-42-x86_64 | pinned image SHA-256 `e401a4db2e5e04d1967b6729774faa96da629bcf3ba90b67d8d9cce9906bec0f` | unavailable; pinning authenticates input, not an acceptance PASS |
| Windows 11 AMD64 | actual disposable interactive VM image, edition/version/build and standard-user identity | unavailable; no Windows build is inferred |

Authoring environment observed on 2026-10-07: Debian GNU/Linux 13 (trixie),
x86_64, no qualified Windows VM or local QEMU/KVM. This host is not a supported
acceptance substitute. GitHub self-hosted runner enumeration returned HTTP 403
(`Resource not accessible by integration`); availability is unknown, not proven
absent. Existing Windows hosted service-session and genuine-logon limitations
remain explicit. No inaccessible runner is selected blindly.

## Candidate and evidence boundaries

Every producer checks out the actual PR head (or dispatch SHA); consumers receive
its frozen bytes rather than rebuild. The Windows producer records detected
OS identity and compiler/linker SHA-256/version, builds Setup twice and compares
bytes. The Linux producer merges DEB/RPM and actual AppImage manifests.

Before observed producer success, artifact ID, filename, source, product version,
size, SHA-256, authenticated manifest identity, exact consumer OS/build, workflow
run/job ID, GUI/IPC probes and cleanup results are **unavailable**, not intended
evidence. Uploaded candidate manifests and run artifacts carry these fields when
produced; the final PR completion record links the observed runs. No hash or
Windows build is invented in this source ledger.

The orchestration workflow reuses the existing Windows and Linux workflows. Four
Linux disposable consumers exercise DEB/RPM and AppImage separately on both
qualified OS targets. Quick install is exercised on the native-package guests.
The aggregate preserves per-row result-v1 records and creates only `contract-valid`
BLOCKED preflight records when a required consumer result is missing. Their
platform facts describe the aggregate host, not a fictitious target execution.

No lower evidence can close a required gate: `ci-native-scoped` silent Setup,
fixture package ownership, unpacked AppDir, offscreen Qt, process SIGKILL and
container restart remain diagnostic. `INSTALL-JOURNEY-8` requires hard VM power
cuts at every named interruption boundary followed by reboot/reconciliation.
Connect (`FIRST-RUN-1`) requires `live-external-dependency`; Host (`FIRST-RUN-2`)
requires its supported installed product path at `native-clean-machine`.

## Current blockers and owning fixes

| Surface | Current observation | Classification / next evidence |
| --- | --- | --- |
| Windows native compiler/linker | Baseline workflow omitted authenticated linker selection; source wiring now uses the active Visual C++ installation and records identities | in-scope candidate build defect; hosted rerun required |
| Windows C++ closure | P044 observed missing MSVCP140.dll; windeployqt can stage an elevated redistributable installer without app-local DLLs | authenticated x64 toolchain CRT is now staged app-locally; exhaustive non-system import audit stays strict; native rerun required |
| Windows Unix-only test imports | Baseline unconditional std::os::unix import reproduced by source inspection | Unix permission/link tests now cfg-gated separately; portable credential tests remain enabled; Windows compilation rerun required |
| AppImage APPIMAGE-8 | Pinned linuxdeploy with a hook wraps custom AppRun into AppRun.wrapped; reproduced at real tool boundary with a diagnostic payload | builder discards only generated cosmetic Qt hook, restores reviewed AppRun and checks byte equality; exact AppImage/native reruns required |
| DEB/RPM desktop reproducibility | Hosted Qt 6.4.2 mismatch traced to allocation-dependent AOT register declarations; raw generator/object differentials reproduced | build-host wrapper canonicalizes only observed independent declarations before compilation and preserves raw provenance; full final-head A/B linked-byte gate must pass |
| Windows/macOS client tests / Linux move intent | baseline failures remain historical observations | broad final-head Rust workflow must rerun; no blanket test disable |
| PostgreSQL crash/restart | historical server failure | separate broad server regression; Host cannot PASS unless its live prerequisites pass |
| P036 production-artifacts identity | acceptance/p036/production-artifacts.json absent at baseline | Host acceptance prerequisite; no synthetic production identity |
| Windows graphical / real logon / installed IPC | hosted service session is insufficient; end-to-end IPC not fully wired | BLOCKED_BY_ENVIRONMENT / missing native implementation; no silent-Setup substitution |
| Linux GUI, trust chain, lifecycle, first run | reviewed guest assertion/action handlers remain incomplete | BLOCKED; adding orchestration does not implement missing product acceptance |
| VM interruption | control-plane power cut exists; no complete product mutation/recovery driver | BLOCKED; final scenario minimum stays interruption-power-cycle |

## Per-scenario evidence and cleanup

The machine-readable row/scenario inventory is
[p045-matrix-v1.json](../../deploy/acceptance/p045-matrix-v1.json).
Run `python3 scripts/install_acceptance.py inventory` for scenario digests and
minimum evidence. Run the P045 validator with `--aggregate`, `--source-commit`,
`--artifact-root`, `--output` and `--require-pass` to emit the actual run ledger.
It rejects ambiguous retries, stale source/scenario digests, changed bytes,
missing target identities, weaker evidence, unsupported versions and invalid
result-v1 structure. It never writes PASS after a missing execution.

No native scenario cleanup has completed in the authoring environment. Missing
executions have `cleanup_result.status=not-run`; actual consumer cleanup is
recorded separately, including failures. The reproduced AppRun diagnostic uses
only scenario-owned files under work/. No unrelated state is removed.

## Unsupported / detected-not-qualified

| Platform | Current status |
| --- | --- |
| Debian (all versions) | detected-not-qualified; no P045 Debian acceptance |
| Linux Mint, Pop!_OS | detected-not-qualified; APT/ID_LIKE does not qualify |
| RHEL, Rocky, AlmaLinux, CentOS, openSUSE | detected-not-qualified; RPM format does not qualify |
| Arch, Manjaro, CachyOS and other Linux distributions | not qualified for P045; AppImage passes on two targets would not prove universal compatibility |
| Ubuntu releases other than 24.04 / Fedora releases other than 42 | not qualified for P045 |
| Windows ARM64, other Windows releases, Linux ARM64/32-bit, macOS/mobile | outside this matrix |

Acceptance remains withheld until every required final-head gate passes at its
declared minimum and the final PR is merged and verified. No release/tag is
created and P046 is not begun.
