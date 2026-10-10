# First run and first synchronization

> **Preview status:** v0.2 is not released, and the end-to-end Host journey is not production-qualified. These steps describe the implemented Connect, sign-in, library, and progress flow plus the Host boundary. Current v0.1 instructions remain in the [operations guide](../RELEASE_OPERATIONS.md).

## Choose how to use Synveil

On a new client profile, Synveil opens Welcome and offers:

- **Connect to Synveil** for a server that is already set up and reachable.
- **Host Synveil** for the planned managed server setup on this device.

An existing configured profile goes to the normal app instead of showing Welcome again.

### Connect to an existing server

1. Choose **Connect to Synveil** and enter the server address provided by its administrator. If you leave out the scheme, Synveil uses secure HTTPS. Unencrypted HTTP is rejected.
2. Select **Connect**. Synveil checks that the server is reachable and ready before saving the connection. Check the address, network, server availability, and valid HTTPS certificate if this step fails. Do not disable TLS checks.
3. Sign in with the one-time device code issued by the server's supported sign-in flow. Enter it only in Synveil. Never send the code, password, or credential-store contents to support.
4. If the server has no library configured for this device, choose a library name and a local folder in the native folder picker, then select **Create library**. Existing files stay in place and can be added to the library.
5. Keep the selected folder available and writable. Synveil begins its normal sync work after the library is confirmed. If setup was interrupted, choose the same folder again so Synveil can reconcile the existing setup.

This flow connects to an existing server. It does not install a server or database. Attaching a device to an arbitrary existing remote library is not currently supported; the first-library flow creates a library and binds this device's folder.

### Host on this device

The Welcome screen contains a Host choice, but that choice is not evidence of a complete server setup. On the current source, hosting reports that it is not available on the device; the end-to-end P036 journey, including approved service setup, initial administrator setup, and native acceptance, remains open.

Do not rely on Host as a production service until an official release explicitly marks the journey qualified. Do not install PostgreSQL, edit database settings, or create service files as a workaround for the ordinary managed Host path. Existing v0.1 server operators should follow the current [deployment guide](../DEPLOYMENT.md).

### Advanced or externally managed server

Advanced administration is for an operator who already owns the server, database, storage, network, and TLS configuration. The v0.2 contract reserves an external-PostgreSQL path for this audience; it does not make that path part of ordinary first run or qualify the new Host journey. See [Advanced installation](ADVANCED_INSTALLATION.md) and the current v0.1 [deployment guide](../DEPLOYMENT.md).

## Read the progress states

The desktop reports bounded evidence for these stages:

| Stage | What it means |
| --- | --- |
| **Synveil ready** | The desktop and its local control process are ready. This does not report native installer progress. |
| **Server ready** | The configured server connection is available. |
| **Signed in** | This device has authenticated with the server. |
| **Library ready** | A library and this device's local folder are configured. |
| **First sync / Up to date** | The sync runtime has produced durable evidence that the current work is quiescent. |

Progress can say waiting or action required when the network is unavailable, sign-in is needed, the folder is missing, a conflict needs attention, or sync is paused. Synveil does not invent a percentage or item count. A requested sync or a temporary idle period alone does not mean the first sync is complete.

Synveil is ready to use when the server, sign-in, and library stages are ready and the first-sync stage reaches its completed state. If it does not, use [troubleshooting](TROUBLESHOOTING.md); do not reset the library or delete local files.

## Safe recovery

- If Connect fails, correct the address or restore server/network access, then use the app's Connect action again after the failure is resolved.
- If sign-in fails, request a fresh device code from the server's supported flow. Never reuse or share a code after an uncertain result.
- If library creation was interrupted, choose the same folder and let Synveil check the previous setup.
- If a folder is unavailable, reconnect its drive or restore access, then use **Restore missing folder** when the app offers it. An unavailable local folder does not mean remote files should be deleted.
- If an operation says it must check an earlier result, wait for reconciliation or use the named repair/recovery action. Do not repeat an uncertain mutation blindly.
