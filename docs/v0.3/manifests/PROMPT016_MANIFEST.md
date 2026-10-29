# Prompt 016 manifest — PostgreSQL 17 desktop controller lifecycle

## Scope

This is a provisional P016A investigation record, not qualification of the
unavailable authoritative P016 roadmap. It investigates the PostgreSQL 17
desktop-process test mismatch where a restart expected controller generation 2
and observed generation 3.

- Repository: `nghianguyen150612/Synveil`
- Branch: `fix/v03-p016-pg17-desktop-controller-lifecycle`
- Starting baseline: `7d1d402b40845054b8c819f283ffc88e0dcc9170`
- Qualified implementation candidate: `e3a7f9cbbec693f46ac9ceb5deb54dd78cf3fa54`
- Original failure run: [Actions run 40808168068](https://github.com/nghianguyen150612/Synveil/actions/runs/40808168068)
- Original failing job: `123232324575`, PostgreSQL 17 live suites
- Pull request: [#176](https://github.com/nghianguyen150612/Synveil/pull/176), open against `develop/v0.3`

## Root cause and correction

ADR-039 assigns a new local generation to every connection attempt. A failed
attempt consumes its generation; the next attempt receives the next number.
The live test had created a stale Unix socket and restarted the process without
waiting for the controller's retry state. It assumed the next successful
snapshot would always be generation 2.

A test-only bounded trace and deterministic restart sequence now hold the
endpoint unavailable until generation 2's status handshake fails with
`EndpointUnavailable`. The process then restarts and generation 3 establishes
the controller relationship and publishes the fresh snapshot. The test
strictly asserts the observed attempt and publication sequences, one successful
relationship for each available process, one handshake per status/command/
events channel, no generation-2 success, and one runtime owner for the restarted
process. The stale-generation fence test seeds a fresh generation-2 snapshot
and asserts that an old generation-1 response leaves the complete snapshot
unchanged. The event reader continues to gate event hints on its active
generation.

The trace is behind `synveil-client/test-support`, retains at most 64
credential-free events, and records only generations, logical channel names,
and typed outcomes. No production connection semantics or security assertions
were relaxed.

## Verification record

Environment: disposable PostgreSQL 17.11 container. The earlier uninstrumented
baseline produced three passes (1.89–2.43 seconds) and did not expose attempts.
The later deterministic trace provided the relevant transition evidence.

| Check | Result |
| --- | --- |
| Deterministic `live_pg17_process_authentication_and_graceful_shutdown` | 5 passes: 11.93s, 12.70s, 16.60s, 16.67s, 16.70s; every trace shows attempt 1 success, attempt 2 status handshake failure, attempt 3 success |
| `generation_fence_rejects_old_response_and_event` | 1 passed; fresh generation-2 snapshot remained unchanged after a generation-1 response |
| P005 `authorization_postgres` | 1 passed |
| P006 API security tests | 2 passed |
| P006 content-service download test | 1 passed |
| P015 PostgreSQL ObjectStore authorization | 1 passed |
| Rust formatting, API all-target Clippy with `-D warnings`, docs validation, `git diff --check` | Passed |
| PostgreSQL 17 scheduled-maintenance workflow, push | [Run 40867335602](https://github.com/nghianguyen150612/Synveil/actions/runs/40867335602), PASS on candidate `e3a7f9c`; 101-step job concluded success |
| PostgreSQL 17 scheduled-maintenance workflow, pull request | [Run 40867342621](https://github.com/nghianguyen150612/Synveil/actions/runs/40867342621), PASS on candidate `e3a7f9c`; targeted desktop lifecycle step 22 concluded success |

Local run logs from the investigation are `/tmp/p016a-live-2.log`,
`/tmp/p016a-repeat-1.log`, `/tmp/p016a-repeat-2.log`,
`/tmp/p016a-repeat-3.log`, `/tmp/p016a-generation-fence.log`,
`/tmp/p016a-p005.log`, `/tmp/p016a-p006.log`,
`/tmp/p016a-p006-download.log`, `/tmp/p016a-p015.log`,
`/tmp/p016a-final-live.log`, and `/tmp/p016a-generation-fence-final.log`.

An early instrumented draft timed out because the pre-existing generation wait
helper imposed a one-second timeout on each watch update, shorter than the
test's five-second reconnect delay. The helper now waits for a generation
change under one bounded 30-second scenario timeout. The final five runs above
used that corrected synchronization. The earlier failed helper run is retained
at `/tmp/p016a-live-1.log` and is not counted as a product or controller
failure.

## Unresolved blockers

This work does not resolve P004-D02/D03/D11/D14, P028 Windows native
acceptance, P036 publisher signing and trust, the independent P015 security
review exception, or overall production release readiness.

## Delivery

Candidate `e3a7f9cbbec693f46ac9ceb5deb54dd78cf3fa54` is committed on the existing
P016 branch and PR #176 is open against `develop/v0.3`. Both exact-head
PostgreSQL 17 workflow runs recorded above concluded PASS. Do not merge
automatically or start P017.
