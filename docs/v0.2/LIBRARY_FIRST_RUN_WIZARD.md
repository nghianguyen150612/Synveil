# Library first-run wizard

Prompt040 turns the existing authenticated library setup operation into one
focused first-run page. It asks for a library name and a local folder, using
the native directory picker. It ends when the authoritative client snapshot
shows the configured library; it does not claim that files have synchronized.

## Routing

The page is derived from a fresh client snapshot. It appears only when the
server profile is configured, this device is authenticated, and no configured
library is present. P037 Welcome, P038 connection setup, and P039 sign-in keep
their existing precedence. A configured library bypasses this page. No
first-run completion flag is saved by QML.

## Setup ownership

The page reuses the existing path:

```text
QML name + native folder selection
  → DesktopUiBridge
  → DesktopController
  → authenticated local client control
  → synveil-client / DesktopSyncHost
  → existing authenticated library API
  → durable local binding
  → existing sync runtime and observer
```

The desktop presentation owns only transient form input and safe feedback. The
client continues to own library identity, authenticated scope, path validation,
pending setup identity, response reconciliation, durable binding, and runtime
registration. The selected absolute folder is local state and is not included
in server library metadata.

## Existing folders and recovery

Following ADR-045, a new remote library may be initialized from a safe,
non-empty local folder. Ordinary files remain in place and are observed by the
existing bounded sync pipeline. An initially empty remote library is not
permission to remove local content. Root, overlap, managed control-tree,
profile, and platform safety checks remain in the client. A conflicting
`.synveil` control tree is reported with bounded product guidance; users are
not told to delete files or edit internal state.

An interrupted setup resumes through the existing pending identity. After a
restart the user may select the same folder again; the client reconciles the
same operation rather than inventing another library identity. An uncertain
response requests an authoritative refresh and never causes an automatic
second create request. Until a fresh snapshot confirms a library, the wizard
does not show completion.

The supported operation creates a new logical library and binds this device's
folder. Attaching this device to an arbitrary existing remote library remains
unsupported. Closing the desktop window does not stop the client or sync
runtime.

P041 owns truthful progress through first synchronization. P042 owns broader
repair and recovery UX. Neither is implemented here.
