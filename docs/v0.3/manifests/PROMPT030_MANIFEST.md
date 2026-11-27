# P030 provisional manifest

Status: P030_PARTIAL_RESUMABLE. The test-only P030 projection is an initial increment, not a qualified delivery system. See [qualification](../security/P030_AUDIT_OUTBOX_RECOVERY_QUALIFICATION.md) for scope and limitations. The authoritative roadmap was unavailable.

The test-only installer now rejects a nonempty P029 event stream under a source-table lock; it does not backfill. Delivery, consumer and recovery remain unimplemented and unqualified.
