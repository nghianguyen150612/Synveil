# P030 audit outbox recovery: partial reference

Status: **P030_PARTIAL_RESUMABLE**. This is not a qualification or a production design. The authoritative P001–P060 roadmap remains unavailable; reconcile its scope before treating this work as authoritative.

The test-only SQL installs an AFTER INSERT trigger on P029 events. Insertion failure aborts the containing transaction. Each P029 event, including denial and revocation evidence, receives a distinct event-ID projection. P027's synthetic outbox only covers successful operations and is not a complete P029 evidence stream. The existing P029 event and checkpoint remain authoritative. The trigger must be installed before events are inserted; this partial implementation has no backfill or completeness check.

There is no dispatcher, claim protocol, receiver, acknowledgement, verification integration, recovery qualification, Windows qualification, or production delivery. The columns describing future delivery states do not implement transitions or fencing. Do not use this schema as a delivery system. Production authorization, audit export, external authentication, retention and integrity remain blocked pending separate approval.
