# Installer security hardening — P043

P043 hardens the existing installer/distribution owners. It does not qualify a
clean machine, implement resilience, provision production keys, or release v0.2.
The decision is [ADR-073](../adr/ADR-073-v0.2-installer-security-hardening.md);
execution evidence is [PROMPT043_MANIFEST.md](PROMPT043_MANIFEST.md).

## Audit scope and trust model

The source baseline is `348201593addc93a9760b7fb18dc540673983076`, the verified
PR #73/P042A merge. Audit covers the roadmap; installation architecture/product
and clean-machine contracts; P005/P006/P011 manifests; release manifest and
acquisition; P009 lifecycle and P010 diagnostics; Linux detection/quick install;
Windows runtime/per-user/lifecycle; ADR-049 through ADR-055; install-engine;
DEB/RPM/AppImage/Windows producers, package hooks, startup helpers, native
validators and GitHub workflows. Historical readiness statements are evidence
of those prompts, not a substitute for current source.

The chain remains release identity → authenticated raw metadata → exact artifact
selection → verified bytes → private staging → explicit scoped privilege →
native package authority → verified postconditions. P005 inventories artifacts;
P006 authenticates/acquires; P011 selects and rejects rollback; P007/P008 retain
engine/journal ordering; P009 authorizes lifecycle; P010 bounds diagnostics.
Native package databases are never edited or replaced by Synveil.

Assets are installation-owned code/integration, release identity, caller-owned
freshness evidence, and protected durable user/server state. Untrusted parties
include artifact hosts/mirrors, unauthenticated metadata, malformed local
ownership evidence, other users, untrusted environment/command inputs and
unproven filesystem objects. Normal TLS validation stays enabled. A principal
controlling the installer's own user identity, executable code and local trust
policy can alter that authority; this work does not claim protection against
that principal or root/administrator, or a compromised deliberately trusted
publisher signing key. These limits do not authorize a larger deletion scope.

## Threat / mitigation matrix

Evidence abbreviations: **D** = `tests/release_download`; **M** =
`tests/release_manifest`; **C** = `tests/release_channel`; **Q** =
`tests/linux_quick_install`; **S** = `tests/installer_security/test_security.py`;
**E** = `crates/install-engine/tests`; **W** = Windows lifecycle model plus
`scripts/test-windows-installer-security.ps1`. Source guards complement tests;
W native execution is reported only after a real hosted result.

| Threat | Trust boundary / existing mitigation | Discovered gap / P043 disposition | Validation evidence |
| --- | --- | --- | --- |
| Malicious artifact host | P006 authenticated size/hash, allowlisted TLS | Already sufficient; exact-byte integrity remains mandatory | D digest/size/extra-byte cases; S real signature cases |
| Compromised mirror | Local origins, pins/keys independent of host | Already sufficient; remote source cannot add origins/keys | D untrusted-origin; S explicit local policy |
| Manifest replacement | Authenticate raw bytes before P005 parse | Already sufficient; production crypto callback gap closed | D authentication binding; S modified bytes |
| Artifact + checksum replacement | Publisher metadata authentication, separate artifact hash | Already sufficient; checksum beside payload is never trust | D wrong pin and modified artifact |
| Replayed channel metadata | P011 minimum generation / high-water evaluation | Quick-install passed `None` every run; private caller persistence added | S persistent 42→41 rollback; C rollback |
| Same-generation equivocation | P011 generation + exact digest | Persist exact bytes' digest; never use time/server preference to resolve | S persistent same-generation alternate bytes; C equivocation |
| Version downgrade | P011 ordering, Windows comparison, native install policies | P009 trusted `Supported` label; now independently compares. Native pre-unpack gates added | E supported-label adversary; Q zero mutation; S native hook |
| Platform substitution | P005 enum + exact P006 selector | Already sufficient; malformed field types now fail with typed errors | M/D; S field table |
| Architecture substitution | Structured normalized exact match | Already sufficient; no architecture fallback | D exact architecture; platform detection suite |
| Artifact-role/type substitution | Authenticated type/role, unique selector | Already sufficient; extension never supplies authority | M/D type/role/duplicate/zero match |
| HTTPS downgrade | HTTPS-only origin check, normal TLS | Already sufficient; URL controls/backslashes also rejected before parsing | D redirect downgrade; S URL parser |
| Untrusted redirect | Every redirect independently allowlisted | Already sufficient | D redirect origin tests |
| Redirect loop/excess | Seen set and maximum five | Already sufficient | D loop/limit tests |
| URL credentials | P006 rejects userinfo | Already sufficient; diagnostics do not include URL | D credentials; S URL table |
| Malicious filename | P005 basename semantics | Normalization aliases, controls, ADS, devices and option-leading basenames were accepted; now rejected | S portable basename property table; M |
| Absolute/traversal paths | P005/P006 relative basename and ownership scopes | Reject separators explicitly before normalization, plus Win32 aliases | S Unix/Windows path table; W |
| Symlink escape | Private download root and target link checks | Ancestor links/writable ownership were not checked consistently; now checked before creation/deletion (root-owned sticky temp roots remain supported) | D parent replacement; S ancestor; E AppImage/journal |
| Windows junction/reparse escape | Existing final-object attribute check | Ancestor junctions could redirect payload/cleanup; reject every component, including root | W actual junction fixture; source `RequireNoReparseAncestry` |
| Destination race | Atomic create-if-absent hard link | Preserve; POSIX promotion and cleanup use anchored directory descriptor | D no-clobber/parent race |
| Temporary-file race | P006 unpredictable create-new temp | AppImage used predictable PID temp; now RAII unpredictable tempfile. Journal create-new remains inside owned state | D temporary replacement/cleanup; E AppImage/journal |
| TOCTOU after verification | Private stage + Linux pre-mutation rehash | Linux revalidates at native adapter; Windows rehashes manifest-bound helpers before Exec. Byte identity/ancestors checked before promotion | D temp replacement; S last-use; W source/helper guards |
| Command-line injection | Python argv / Rust Command / fixed Inno Exec | Preserve structured native argv; reject malformed/duplicate Windows custom options | S argv assertions; W option fixtures |
| Option injection | Local artifact argument | Add `--` for APT/DNF and reject option-like basename/nonabsolute local argument | S package argv/path tests |
| Shell injection | No shell templates in acquisition/manager | Preserve; AppImage desktop/systemd expansion characters escaped | S metacharacter argv; E generated integration |
| PATH executable substitution | Client canonical sibling; OS tool boundaries | Reject client links resolving outside sibling directory; package tools/systemctl use OS identities; bootstrap fixes interpreter and search path | Client P043 resolver test; S PATH test; E source |
| Environment injection across privilege | Whole Linux installer already rejects root | Sudo receives only exact OS executable/argv and a small PATH/C locale environment; no loader, proxy, HOME or Python inheritance | S scoped environment assertions; Q root guard |
| Package-hook privilege confusion | Hooks never start persistent client / edit homes | Scope sysusers/tmpfiles to owned config; fixed PATH; reject config leaf links; no raw argument logs | Static gate, DEB/RPM validators, S preinst fixture |
| Process-name ownership confusion | P026 profile task identity; P042 kernel IPC peer PID | Already sufficient; no process-name kill or lookup added | Existing client supervisor/Task Scheduler tests; static gate |
| Repair/uninstall scope expansion | P009 protected-resource/ownership classes, explicit file inventory | AppImage swallowed record errors; now preflight requires valid ownership/version and preserves unknown files | E lifecycle/AppImage; W old-minus-new model |
| Unknown/newer schema reset | P008/P009/P011 strict schemas | AppImage repair error fallback removed; persisted channel schemas reject without overwrite; journal remains protected | E journal/lifecycle/AppImage; S unknown high-water schema |
| Signing-key substitution | Local trusted IDs; deterministic eligible signatures | Add explicit bounded public policy binding ID to key bytes; remote descriptor cannot establish root | S policy substitution / real crypto |
| Test key entering production policy | No CI default trust | Production purpose and fingerprint-bound namespace required; no committed production root or test seed; CI/environment irrelevant | S test/CI/filename policy rejection |
| Key retirement / overlap order | All locally eligible signatures tried deterministically | Implement real Ed25519 verification within existing ordering; no authentication fallback | S old/new/overlap/reversed/removed/malformed/all-invalid/one-valid; D/C |
| Secret leakage | P010 finite typed diagnostics | Remove package-hook argument echoes and AppImage raw Debug IO errors; unknown mutation errors reconcile first | P010 bridge/E redaction; S hook secrets; static gate |
| Untrusted bootstrap code | Existing explicit prohibition on remote shell pipelines | Add deterministic closed bundle producer; independent digest/signature must be verified before extraction/execution | S deterministic inventory/no-clobber; bootstrap source guard |
| Unsafe uninstaller discovery | Exact HKCU registration | Parse only one quoted owned uninstaller identity with known optional SILENT; component containment + ancestry | Shared Windows guard fixtures; lifecycle script |
| Unknown upgrade compatibility | P009 explicit disposition; P011 exact upgrade_from | Windows previously accepted any newer version; now compiled explicit sources only. Production list currently empty | S Windows compatibility; W fixture 1.0.0→1.1.0 isolation |
| Unknown adjacent install files | Inno explicit file list / native package ownership | Fresh Windows payload collisions fail; no scan-and-claim cleanup. Ordinary uninstall still uses native ownership | W model/native lifecycle source; E protected state |
| Blind replay after uncertainty | P008 unmatched start → OutcomeUnknown | Already sufficient; generic post-native errors now retain staging and require reconciliation | E journal/lifecycle; Q/S error cases |
| Verification bypass | No supported insecure/force switches | Preserve; static AST/callsite gate checks production literals and executable boundaries | Static validator plus existing Q/M/D suites |

## Privilege, native authority and last-use identity

Windows retains `PrivilegesRequired=lowest` and an exact current-user
LocalAppData package root. `/DIR`, `/ALLUSERS`, `/LOADINF`, duplicate/malformed
security options, redirected roots and ancestor reparse points fail before
payload mutation. Setup/repair/cleanup use trusted closed manifests; helper
execution requires manifest-bound SHA-256. Shortcut destinations remain the
current user's reviewed integration. Native Inno owns removal; unknown files
never enter old-minus-new cleanup.

Linux quick install qualifies the host, authenticates channel and selected
manifest, selects the exact native package, verifies its stream/size/hash,
asks consent, and rechecks the local regular-file identity immediately before
native mutation. Only the exact OS-owned APT/DNF operation is passed to sudo
with structured arguments and `--`. The complete installer refuses root.
Native `dpkg --verify` / `rpm -V` checks follow native version/payload checks.
No native lock/database is deleted and no failed transaction is blindly replayed.
DEB/RPM pre-unpack hooks reject unsupported/downgraded native source versions.

AppImage integration remains user-level and refuses root. The already selected
AppImage runs its owned helper; integration records are not publisher metadata.
The helper records its product version and cannot repair/remove unknown state.
It never takes over a native package database or user library. Absolute package
sibling resolution, escaped desktop/systemd data and scoped systemctl replace
untrusted PATH/expansion behavior.

Exact release metadata bytes and payload digests are authenticated in P006.
Linux's native manager ultimately opens the private staged path; owning-user or
root replacement after the final check remains possible when that principal
also controls the installer code/local trust policy. No immutable-file claim is
made. Other users cannot replace the private stage. Windows embeds the verified
producer inventory in Setup and rechecks owned executable bytes before use;
its entire ordinary lifecycle uses the same non-elevated user identity.

## Production signatures and bootstrap

Install the reviewed crypto backend from trusted dependency distribution in the
interpreter used for signature verification. `scripts/release_signature.py`
implements Ed25519 verification only; there is no signing implementation, private
key, seed, recovery material, committed project root, environment trust switch,
remote key import, or fallback after a signature failure.

A deliberately provisioned local policy has this shape (placeholders are not
keys and fail validation):

```json
{
  "schema_version": 1,
  "purpose": "production-release-verification",
  "keys": [{
    "key_id": "synveil-release-SHA256_OF_RAW_PUBLIC_KEY",
    "scheme": "ed25519",
    "public_key_hex": "32_BYTE_PUBLIC_KEY_AS_64_LOWERCASE_HEX"
  }]
}
```

The manifest CLI accepts either `--expected-manifest-sha256` or `--trust-policy`
with a bounded detached descriptor/signature directory. Quick install accepts
either its independent trusted channel pin or `--trust-policy`; signed channel
bytes authenticate the exact manifest identity through P011. Local keys are
established before downloads; descriptors provide only scheme/ID/signature name.
Both modes remain explicit, mutually exclusive and fail closed.

Build a closed, deterministic bundle with
`python3 scripts/build-quick-install-bundle.py --output BUNDLE.tar.gz`.
Its printed digest must be independently authenticated through the Synveil
release identity. Verify that digest with trusted OS tooling **before** any
archive extraction or code execution. The bundle contains only the explicit
bootstrap modules, platform policy, wrapper and crypto dependency declaration;
there are no archive links, arbitrary discovered files or final production URLs.
The wrapper uses `/bin/bash -p` to suppress inherited Bash startup code, a fixed
OS search path, and `/usr/bin/python3 -E -s`; it does not elevate. Production
key custody, deliberate public-root delivery, final URLs and ordinary-user
distribution instructions remain release operations/P046. Do not use `curl | sh`.

## Test coverage and evidence limits

The focused security suite supplements existing P005/P006/P011/P017/P027 and
engine suites. Required adversarial cases 1–19 and 31–36 map to M/D plus S;
20–27 map to C and real Ed25519 S; 28–30 map to P009 E, Q and Windows S/W;
37–41 map to S/D, AppImage E, client resolver and hook checks; 42–44 map to
Windows S/W, existing native lifecycle sentinels and AppImage E; 45–48 map to
E/S, P010's bounded model and the AST/source validator. Deterministic assertions
use in-memory generated synthetic keys, fixed metadata/payloads and disposable
roots. Core security tests make no public network request. Native fixture and
compiler results are narrower than clean-machine/native runtime qualification.

The focused workflow separates acquisition/channel, lifecycle/journal,
Linux invocation/hooks, Windows ownership/compiler, static/docs and Python
crypto dependency policy. Whole-workspace formatting/strict Clippy and
`cargo deny check` remain in existing CI on the same head. P043 regressions must
be fixed without disabling gates; unrelated platform/native/server failures are
reported separately. P044 is explicitly deferred. P045–P048 remain open and no
v0.2.0 release/tag is created here.
