# P016A: Desktop controller connection generation investigation

Status: provisional P016A qualification. This report records a deterministic
reproduction of the generation mismatch; it does not claim completion of an
authoritative P016 roadmap.

## Finding

The controller assigns a generation to each logical connection attempt, as
required by [ADR-039](../../adr/ADR-039-ipc-backed-desktop-controller-core.md).
The generation advances when an attempt begins, including attempts that fail
before the three-channel controller relationship is established. Therefore a
successful generation of 3 after process restart is valid when generation 2
was an unsuccessful reconnect attempt.

The previous live test restarted the process as soon as it had created a stale
Unix socket, then asserted that the next successful snapshot must be generation
2. The interval during which that socket was unavailable was not synchronized
with the controller's retry. The exact assertion could therefore fail when the
controller attempted and failed once before the replacement process was ready.

The test now keeps the endpoint unavailable until its test-only lifecycle trace
records the failed generation-2 status handshake. It then restarts the process
and strictly requires generation 3 to establish and publish. The trace also
checks that generations 1 and 3 each make exactly one status, command, and event
handshake, establish one relationship, and publish one fresh snapshot. No
successful relationship or snapshot is allowed for generation 2. The bounded
trace contains only generation numbers, channel names, and typed error kinds;
it never records endpoint paths or credentials and is compiled only with the
`test-support` feature.

## Reproduction evidence

The actual ignored test is
`live_pg17_process_authentication_and_graceful_shutdown` in
`crates/api/tests/desktop_process_postgres.rs`. It is Linux-only and requires
`SYNVEIL_TEST_DATABASE_URL` to point to a fresh, disposable PostgreSQL 17
database. The bounded reproduction command was:

```bash
cargo test -p synveil-api \
  --test desktop_process_postgres \
  --locked \
  live_pg17_process_authentication_and_graceful_shutdown \
  -- --ignored --test-threads=1 --nocapture
```

PostgreSQL 17.11 was used in a disposable local container. The targeted test
was run five times with one test thread. Each passing deterministic trace had
this transition sequence:

```text
attempt 1 -> status/command/events handshakes succeed -> relationship established
          -> fresh snapshot published at generation 1 -> relationship ends
attempt 2 -> status handshake fails (EndpointUnavailable)
attempt 3 -> status/command/events handshakes succeed -> relationship established
          -> fresh snapshot published at generation 3
```

This reproduces the reported expected-2/observed-3 mismatch through one failed
reconnect attempt, without a duplicate successful connection. The controller
manager executes attempts sequentially. The deterministic trace asserts attempt
generations `[1, 2, 3]`, published generations `[1, 3]`, and exactly two
successful relationships. The stale-generation unit test seeds a fresh
generation-2 snapshot, submits an old generation-1 response, and verifies that
the complete fresh snapshot remains unchanged. The event reader independently
checks its generation fence before applying event hints.

The original hosted run recorded only the final assertion (`expected 2`,
`observed 3`), not an attempt trace. The deterministic test establishes a
contract-compliant cause for that exact mismatch; it does not reconstruct an
internal trace that was not collected by the original run.

## Correction and safeguards

The live test no longer assumes every process restart yields exactly one
attempt. It synchronizes on the failed attempt, then retains a strict exact
generation assertion for the known sequence. No production generation
semantics, endpoint authorization, IPC authentication, retry classification,
or fail-closed behavior changed. The test continues to check stale snapshot
retention, freshness, restart usability, runtime identity, and library
ownership. Existing credential-redaction assertions remain active.

The deterministic generation-fence unit test remains in place. P005, P006, and
P015 security regressions were run unchanged.

## Remaining limits and blockers

The instrumented scenario proves the one-failed-attempt explanation and
distinguishes it from duplicate successful controller relationships. Because
the original workflow run lacked instrumentation, it cannot establish which
individual event occurred during that historical run beyond its observed final
generation.

This provisional milestone does not resolve P004-D02/D03/D11/D14, P028 Windows
native acceptance, P036 publisher signing and trust, the independent P015
security review exception, or overall production release readiness.
