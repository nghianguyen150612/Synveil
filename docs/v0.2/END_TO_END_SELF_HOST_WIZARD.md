# End-to-end self-host wizard

Status: **Coordinator implemented; Phase-E native qualification blocked.**

## Product path and support boundary

The explicit **Host Synveil on this device** action enters the UI-neutral Host
controller. It never runs at desktop startup or from Connect. Personal/Home is
designed for Ubuntu 24.04 x86_64 and Fedora 42 x86_64; neither target is called
qualified until its installed-artifact native gate passes. Windows, macOS,
ARM, and generic Linux are not managed-Host claims.

Visible stages are Checking this device, Choose server storage, Choose access,
Review, Preparing Synveil, Create your Synveil owner account, Checking server,
and Ready. Choices are limited to a native-folder-picker storage location,
LocalOnly (recommended) or a qualified PrivateLan candidate, and owner login
and password. The password is transient and must be cleared after submission.
No ordinary step asks for a database URL, port, service, unit, Caddyfile,
certificate, firewall command, or terminal command.

Before mutation Review says that this device hosts Synveil, identifies server
data and access scope, explains background operation and preservation of
existing data, and says desktop removal does not delete hosted data. The plan
binds owner identities, generation/fingerprints, artifacts, platform, and
bounded effects. Stale plans return to inspection. Elevation uses a native,
purpose-specific adapter; the desktop never remains root.

## Composition and recovery

`synveil-server-bootstrap` coordinates, in order, P031 configuration, P032
storage, P033 dependencies/services, P034 reachability, and P035 machine-local
bootstrap plus normal login. It owns none of them and never writes config,
initializes storage/PG, generates TLS, changes firewall, or inserts an admin.
Its durable state is derived from fresh owner inspection, not a readiness file.

Cancel before confirm has no durable effect. Cancel after confirm preserves a
resumable partial setup. Unknown effects are reconciled before retry. Missing
configured storage, database outage, or edge outage makes readiness fail
without replacement or identity rotation. Repair and server-software reinstall
may restore owned replaceable integration only; configuration, secrets,
database, object data, bootstrap, and logical Library remain preserved.

## Ready and handoff

Infrastructure-ready is not Server Ready. Final verification reinspects config,
storage identity, PG17 cluster/system identity, migrations, API live/ready,
worker topology, loopback backend, private PostgreSQL, selected HTTPS edge,
bootstrap Closed, explicit instance-admin login, and coherent initial logical
Library/root. The secret-free result contains installation ID, canonical
origin, trust descriptor, reachability mode, and logical-library availability.

P037 supplies the unified Welcome and routes Host here. P038/P039 consume the
origin/trust result; P036 does not create their final client profile. P040 owns
the local synchronized-folder choice; the Library checked here is server-side.

## Qualification and known limitations

No exact production PostgreSQL 17 artifact or Caddy artifact inventory/digest
is present in this baseline. This environment also has no configured Git
remote, native Ubuntu/Fedora disposable VMs, graphical session, systemd boot
control, or hard-power controller. Consequently restart/reboot, interruption,
repair/reinstall, LocalOnly HTTPS, PrivateLan, data-preservation, and clean
machine evidence cannot be honestly recorded here. FIRST-RUN-2 remains
`IMPLEMENTATION_PENDING`, and the Phase-E readiness marker is withheld.
