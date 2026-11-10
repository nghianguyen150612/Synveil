# P028 Windows native acceptance qualification

Status: **BLOCKED / implementation incomplete**. This document does not qualify P028 for integration. The authoritative v0.3 roadmap was not recovered.

The integration base is `b8beb109e58c847cb136607e4948c564cf6d2aa7`, the merge of P027 source `441237bb42a83808044111f1eb90a08d5396fe0f`. The existing v0.2 native acceptance workflow, candidate producer/consumer, environment qualification, GUI journey, logon/logoff collectors, installed uninstall test, client probe, summary and evidence validators remain the starting architecture. Historical records are unchanged.

Prior [run 42799146489](https://github.com/nghianguyen150612/Synveil/actions/runs/42799146489) tested P027 source HEAD: candidate build, reproducibility and transfer authentication succeeded; standard-user-native failed at the required unavailable-surfaces checkpoint. This is hosted scoped evidence, not Windows 11 interactive acceptance. Candidate digest and size have not been independently recorded here. No new candidate has been qualified.

The standalone evidence validator now rejects a PASS without genuine interactive session classification and a positive session ID bound to a user SID digest. This is a necessary check, not independent verification of the component artifacts or a complete security qualification. Existing evidence tests include candidate identity, harness mismatch, component and session negative cases.

GUI interaction, actual new logon and LogonTrigger, installed IPC, repair, upgrade/downgrade, uninstall/reinstall, cleanup and clean-machine security matrix remain **BLOCKED**: an explicitly approved isolated Windows 11 AMD64 interactive environment with a disposable ordinary user and authorized event access is not available in this workspace. Missing trusted historical fixtures also block upgrade claims. No native execution was performed here. The hosted service-session blocker must remain.

Remaining independent release requirements include Linux APT non-root Qt/QML and Fedora DNF failures; P036 signing, keyring and runners; P045 clean-machine evidence; server self-host acceptance; independent SQLx SQLite patch security review; historical Windows missing-LICENSE and secret-marker investigations; P004 policy decisions; production RBAC and approval integration; physical mobile testing; real LAN/Tailscale qualification; and roadmap reconciliation. Production RBAC, role persistence, approval APIs, new token issuance and pairing activation remain disabled.
