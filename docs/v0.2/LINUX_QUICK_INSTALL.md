# Verified Linux quick install

Prompt017 provides one terminal entry point, `deploy/install/quick-install.sh`,
for qualified Debian/Ubuntu x86_64 (`debian-x86_64`) and Fedora x86_64
(`fedora-x86_64`). The profile is mandatory. The installer validates that the
host agrees with it, but never detects or selects a profile, package family,
distribution version, or architecture automatically. That resolver belongs to
P018. There is no AppImage fallback.

## Authenticate the installer before running it

HTTPS delivery is not, by itself, the Synveil publisher identity. **Do not use
`curl | sh`.** Production publishing keys and the final public download URL do
not exist in this repository, and this document does not invent either.

Release engineering must publish an independently authenticated digest or
signature for the installer bundle. The safe command *template* is:

```text
download deploy/install/quick-install.sh, scripts/linux_quick_install.py,
         scripts/release_channel.py, scripts/release_download.py and
         scripts/release_manifest.py to a reviewed directory
verify the bundle against a digest/signature obtained from the trusted
         Synveil release identity
./deploy/install/quick-install.sh [trusted bootstrap arguments]
```

Do not execute the first step's output unless the independent comparison
succeeds. Production signing/key provisioning and the final public command are
later release-engineering/distribution-readiness work.

## Bootstrap and invocation

Pinned-digest mode is the supported deterministic P017 bootstrap. All values
below are reviewed local inputs, never values copied out of the remote channel:

```bash
./deploy/install/quick-install.sh \
  --platform-profile=debian-x86_64 \
  --channel-url=https://RELEASE_ORIGIN/SYNVEIL-RELEASE-CHANNEL.json \
  --trusted-origin=https://RELEASE_ORIGIN \
  --trusted-channel-sha256=REVIEWED_LOWERCASE_SHA256 \
  --minimum-channel-generation=REVIEWED_MINIMUM_GENERATION
```

Use `fedora-x86_64` only on qualified Fedora. `--yes` is an explicit automation
consent option; without it, an interactive confirmation follows the printed
plan. There is no insecure, skip-verification, force-lock, automatic-profile,
or destructive-purge option.

The trust chain is:

1. P006 HTTPS-only, allowlisted-origin, explicitly checked redirect and bounded
   transport fetches the channel;
2. P011 authenticates its exact bytes against the locally trusted pin, enforces
   the minimum generation, and selects the highest fresh-install release;
3. the selected release binds the exact manifest size, SHA-256, version and
   source commit, and P006 authenticates and parses precisely those bytes;
4. P006 requires exactly one `linux/x86_64/native_package` DEB or RPM match,
   downloads it into a private `0700` staging directory, bounds its size,
   verifies SHA-256, fsyncs it, and atomically promotes without clobbering;
5. the installer re-hashes the staged file immediately before mutation;
6. only then does visible `sudo apt-get install -y PATH` or
   `sudo dnf install -y PATH` request privilege.

Downloaded bytes are never piped into a shell or package manager. The complete
installer is not run as root. It never reads, stores, logs, or pipes a password,
and never deletes native lock files. A busy native manager fails with a retry
message. Network retries and package transaction replays are intentionally
absent; an interrupted ambiguous transaction is `OutcomeUnknown` and must be
reconciled from native state before retrying.

## Plan, evidence, reruns, and completion

Before privilege, the plan names the product version, DEB/RPM, x86_64, verified
identity, APT/DNF, data preservation, and authorization request. Result evidence
retains artifact ID/type/role/platform/architecture, version, source commit,
size and SHA-256, manifest digest/authentication state, and verified local path.
It contains no credentials.

After the manager returns, `dpkg-query` or `rpm` must confirm the expected
version and every launcher, client, desktop entry, icon and user unit must
exist. Manager exit status alone is insufficient. A correctly installed same
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

`linux-packages.yml` builds exact reproducible artifacts. Its Ubuntu job creates
a synthetic loopback TLS release, trusts only its generated test CA, runs the
installer as the runner through APT, verifies a same-version no-op, launch
assets/no autostart, and native removal/preservation. Its pinned `fedora:42`
job performs the equivalent ordinary-user quick install through DNF, then RPM
verification, no-autostart checks, smoke, erase and preservation. Synthetic
keys never represent production identity and are confined to disposable CI.
