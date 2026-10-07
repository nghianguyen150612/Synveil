# Verified Linux quick install

Prompt017 provides one terminal entry point, `deploy/install/quick-install.sh`,
for exactly qualified Ubuntu 24.04 x86_64 (`debian-x86_64`) and Fedora 42
x86_64 (`fedora-x86_64`). P018 automatically detects and qualifies the host;
Debian and derivative distributions are not implicitly supported. There is no
AppImage fallback. See [Linux platform detection](LINUX_PLATFORM_DETECTION.md).

## Authenticate the installer before running it

HTTPS delivery is not, by itself, the Synveil publisher identity. **Do not use
`curl | sh`.** Production publishing keys and the final public download URL do
not exist in this repository, and this document does not invent either.

Release engineering must publish an independently authenticated digest or
signature for the installer bundle. The safe command *template* is:

```text
download deploy/install/quick-install.sh, scripts/linux_quick_install.py,
         scripts/linux_platform_detection.py,
         scripts/release_channel.py, scripts/release_download.py and
         scripts/release_manifest.py to a reviewed directory
verify the bundle against a digest/signature obtained from the trusted
         Synveil release identity
./deploy/install/quick-install.sh [trusted bootstrap arguments]
```

Do not execute the first step's output unless the independent comparison
succeeds. P043 adds `scripts/build-quick-install-bundle.py` as a closed deterministic
bundle producer, including the platform policy. Authenticate its exact digest
independently before extraction/execution. Production signing/key provisioning
and the final public command remain release-engineering/distribution-readiness
work. No key is downloaded from the artifact host and trusted implicitly.

## Bootstrap and invocation

P043 preserves pinned-digest bootstrap and adds the production-capable Ed25519
mode using an independently provisioned local `--trust-policy`; see
[installer security](INSTALLER_SECURITY_HARDENING.md). All values
below are reviewed local inputs, never values copied out of the remote channel:

```bash
./deploy/install/quick-install.sh \
  --channel-url=https://RELEASE_ORIGIN/SYNVEIL-RELEASE-CHANNEL.json \
  --trusted-origin=https://RELEASE_ORIGIN \
  --trusted-channel-sha256=REVIEWED_LOWERCASE_SHA256 \
  --minimum-channel-generation=REVIEWED_MINIMUM_GENERATION
```

The optional `--platform-profile` is an assertion only and must agree with the
automatically qualified host. `--detect-only` emits side-effect-free JSON and
exits before release acquisition. `--yes` is an explicit automation consent
option; without it, an interactive confirmation follows the printed plan.
There is no insecure, skip-verification, force-lock, qualification-bypass, or
destructive-purge option.

The trust chain is:

1. P006 HTTPS-only, allowlisted-origin, explicitly checked redirect and bounded
   transport fetches the channel;
2. P011 authenticates its exact bytes against the locally trusted pin, enforces
   the minimum generation and persistent caller-owned high-water, and selects
   the highest fresh-install release;
3. the selected release binds the exact manifest size, SHA-256, version and
   source commit, and P006 authenticates and parses precisely those bytes;
4. P006 requires exactly one `linux/x86_64/native_package` DEB or RPM match,
   downloads it into a private `0700` staging directory, bounds its size,
   verifies SHA-256, fsyncs it, and atomically promotes without clobbering;
5. the installer re-hashes the staged file immediately before mutation;
6. only then does visible sudo for the absolute OS-owned APT/DNF executable with
   `install -y -- VERIFIED_ABSOLUTE_PATH` request privilege.

Downloaded bytes are never piped into a shell or package manager. The complete
installer is not run as root. It never reads, stores, logs, or pipes a password,
and never deletes native lock files. A busy native manager fails with a retry
message. Network retries and package transaction replays are intentionally
absent; an interrupted ambiguous transaction is `OutcomeUnknown` and must be
reconciled from native state before retrying.

P044 classifies ENOSPC and quota exhaustion while creating, writing, syncing,
promoting, or syncing the artifact directory as `InsufficientDiskSpace` before
native package mutation. The staging file is discarded when owned cleanup is
possible; a failed directory durability operation never returns a verified
acquisition result. A later attempt may reuse a completed destination only
after re-verifying its exact authenticated size and digest. Partial bytes are
not range-resumed or trusted as cache. If interruption may have reached APT or
DNF, the installer inspects `dpkg-query`/`rpm` state and exact package identity
before it can choose a disposition. It never deletes dpkg, apt, rpm, or dnf lock
files, and it never treats a missing child-process result as proof of failure.

## Plan, evidence, reruns, and completion

Before privilege, the plan names the product version, DEB/RPM, x86_64, verified
identity, APT/DNF, data preservation, and authorization request. Result evidence
retains artifact ID/type/role/platform/architecture, version, source commit,
size and SHA-256, manifest digest/authentication state, and verified local path.
It contains no credentials.

After the manager returns, `dpkg-query` or `rpm` must confirm the expected
version and every launcher, client, desktop entry, icon and user unit must
exist. Native `dpkg --verify` or `rpm -V` must also verify the package payload. Manager exit status alone is insufficient. A correctly installed same
version returns `ALREADY_INSTALLED_VERIFIED` without mutation. Incomplete or a
different installed state is not blindly reinstalled; native P009 upgrade or
repair owns that lifecycle. Ordinary package removal remains native and
preserves configuration, credentials, sync metadata, libraries and server
data. P017 does not implement purge or `--repair`.

No GUI/client is launched and no service/autostart is enabled. Success ends:

```text
Synveil installed successfully.
Open Synveil from your application menu.
```

P019 owns the later friendly first-launch and autostart choices.

## Stable exit statuses

| Status | Meaning |
| ---: | --- |
| 0 | installed and verified, verified no-op, or user cancellation before mutation |
| 2 | command usage |
| 10 | unsupported/mismatched explicit profile or platform |
| 20 | release trust, integrity, selection, or staging failure |
| 25 | local staging disk full before native package mutation |
| 30 | bounded network/download failure |
| 40 | authorization required or failed |
| 50 | package manager busy/failure or incompatible installed state |
| 60 | post-install native verification failure |
| 70 | unresolved/unknown package transaction outcome |
| 80 | fail-closed internal failure |

Integrity and acquisition errors state that no package mutation occurred and
offer no bypass. Post-mutation errors say state may have changed and direct the
user to inspection/reconciliation.

## Hosted acceptance boundary

`linux-packages.yml` builds exact reproducible artifacts. Its pinned Ubuntu
24.04 job creates
a synthetic loopback TLS release, trusts only its generated test CA, runs the
installer as the runner through APT, verifies a same-version no-op, launch
assets/no autostart, and native removal/preservation. Its pinned `fedora:42`
job performs the equivalent ordinary-user quick install through DNF, then RPM
verification, no-autostart checks, smoke, erase and preservation. Synthetic
keys never represent production identity and are confined to disposable CI.
