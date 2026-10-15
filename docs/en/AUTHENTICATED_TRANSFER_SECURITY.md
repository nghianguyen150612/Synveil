# Authenticated transfer redirect security

## Contract

Authenticated transfer API routes return their documented success or structured
error response directly. They do not redirect to a login page, canonical host,
object store, CDN, alternate port, or another scheme. The Rust API middleware
converts a handler-generated 3xx response other than the documented content
`304 Not Modified` into `502 transfer_redirect_prohibited` without a `Location`
header. Authentication and authorization failures remain JSON `401` or `403`
responses.

Expected successful route statuses are:

| Route | Success |
|---|---|
| `GET /api/v1/nodes/{node_id}/content` | `200`, `206`, or conditional `304` |
| `GET /api/v1/versions/{version_id}/content` | `200`, `206`, or conditional `304` |
| `POST /api/v1/upload-sessions` | `201` |
| `GET /api/v1/upload-sessions/{id}` | `200` |
| `PATCH /api/v1/upload-sessions/{id}` | `204` with authoritative `Upload-Offset` |
| `POST /api/v1/upload-sessions/{id}/complete` | `200` |
| `POST /api/v1/upload-sessions/{id}/abort` | `200` |

The `Location` response header on upload-session `201 Created` identifies the
new resource; it is not a 3xx redirect. The content API streams through the
application service and does not return storage-provider URLs. PATCH bytes are
streamed to the upload application service. API code does not redirect either
operation to external storage.

This contract applies inside the Synveil API process. It cannot prevent an
operator-managed reverse proxy, TLS terminator, CDN, or DNS endpoint from
redirecting a request before it reaches the API. The repository does not ship a
reverse-proxy configuration. The deployment guide describes operator-managed
HTTPS termination, so each deployment must validate its own public origin.
No deployment is automatically certified by this source-level contract.

## Assets and trust boundaries

Protected assets are DeviceBearer credentials, immutable downloaded content,
uploaded file chunks, upload-session identity and accepted offset, account /
Device / Library authorization scope, server and proxy identity, and durable
local transfer ownership records.

The request crosses the iOS app process, the OS-managed URLSession daemon,
DNS/TLS, an administrator-managed reverse proxy or CDN, the Synveil API and
authentication middleware, the content/upload application service, and the
configured storage backend. The app controls request construction and its own
durable records. It cannot inspect a redirect before iOS follows it, dictate
the proxy's behavior, or inspect the daemon's internal header persistence.

## Threat assessment

| Scenario | Direct API behavior | External edge behavior | Security result |
|---|---|---|---|
| API handler emits `301`, `302`, `303`, `307`, or `308` | Rewritten to structured `502`, no `Location` | Not reached if API generated the response | Prevented in the API process; negative middleware tests cover each status |
| Invalid, expired, revoked, or wrong-owner DeviceBearer; auth/login redirect | Structured `401`/`403`; no browser-login redirect route | A proxy may still redirect or rewrite the path | API behavior is structured; edge behavior is unverified |
| Missing immutable content, unavailable storage, or object-store/CDN offload | Content streams through the application service; errors are structured and no storage URL is returned | An edge may add an error-page redirect or offload content | No API offload; edge behavior remains unverified |
| Same-origin canonical host, trailing-slash, or path-normalization redirect | Router has no canonical-path redirect; any handler 3xx is rejected | Proxy rewrite or slash normalization may redirect before Axum | Unresolved at an uncontrolled edge; same-origin does not prove the route is safe |
| Cross-origin `301`/`302`/`303` on authenticated GET | API does not generate one | iOS background URLSession follows before app approval | DeviceBearer forwarding cannot be ruled out; disclosure risk unresolved |
| Cross-origin `307`/`308` on upload PATCH | API does not generate one | OS may preserve PATCH and forward its body | File-content disclosure remains possible even if Authorization is stripped |
| Same-origin `307`/`308` to an unexpected path | API rejects handler redirects | Proxy can send the body to another same-origin endpoint | Confidentiality and authorization semantics are unresolved |
| HTTPS downgrade, alternate scheme, hostname, or port | API does not generate one | DNS, TLS edge, proxy, or upstream can introduce it | TLS validates the reached peer, not equivalence to the originally authorized origin |
| Multi-hop redirect or redirect after transfer authorization | No redirect from direct handlers | Any hop may introduce a later origin or method change | Final-response checks happen too late; unresolved |
| Redirect to presigned object storage or CDN | API content handlers do not return provider URLs | Proxy or CDN may offload to another origin | Unresolved outside the API process |
| Redirect or task adoption after logout/account change | Local session fencing denies new app submissions | An already-running OS task can outlive the app process | Cancellation is best effort; a late response is not adopted or published, but prior disclosure cannot be reversed |
| Untrusted deployment capability assertion | No redirect-free capability endpoint exists | A proxy or status endpoint could claim support without proving it | No assertion opens either client capability |
| OS persistence of request headers | The app stores no raw bearer in SQLite | Submitted headers are managed by iOS outside the app process | Retention and deletion timing are not controlled by the application |

Apple documents that background URLSession transfers follow redirects without
calling `urlSession(_:task:willPerformHTTPRedirection:newRequest:completionHandler:)`.
TLS validation authenticates the TLS peer for each connection; it does not
establish that a redirected host is the originally configured Synveil origin.
The app cannot inspect the final response before the OS follows the redirect.
Final response or SHA-256 validation happens too late to undo a credential or
PATCH-body disclosure. Do not rely on undocumented Authorization-header
stripping behavior.

Apple background URLSession may persist request headers in OS-managed storage.
Keychain-backed app storage does not mean a submitted DeviceBearer remains only
in Keychain. The app controls its own request construction and task binding, not
the OS daemon's internal persistence or retention period.

## P046 background upload decision

P046's `URLSessionBackgroundUploadTransport` uses a file-backed PATCH request
with DeviceBearer authorization. Its redirect delegate returned `nil`, but
Apple does not invoke that decision callback for background sessions. That
callback therefore did not protect the transfer. A `307` or `308` may preserve
the PATCH method and upload body. Body exposure is a separate risk from bearer
exposure: stripping Authorization would not make forwarding private file bytes
safe.

New P046 background submissions and Resume are disabled. Foreground P045 upload
continues to use its existing redirect-rejecting transport. At app restoration,
the coordinator attempts to durably withdraw authorization for unresolved
background jobs before requesting OS cancellation. If that write fails, the
current process still keeps the capability closed and requests cancellation;
the prior unresolved record remains intact for another recovery attempt.
Unknown or duplicate OS tasks are recorded as bounded orphan task-ID
observations when SQLite is available, then canceled. These observations never
authorize adoption. The coordinator retains each known upload/chunk/task/evidence
identity and does not change accepted offsets, replay PATCH, create another
session, or auto-complete. Cancellation is best effort: an old task may already
have sent bytes or may continue until iOS processes cancellation. Any later
foreground Resume must wait for task disappearance and perform the existing
authoritative status GET. No old task is automatically adopted for further
background work.

## Operator validation checklist

For each production-like deployment and after any edge, DNS, CDN, path, TLS, or
authentication change:

1. Set the iOS server profile to the final public `https` scheme, hostname, and
   port. Do not submit an HTTP or alias origin that relies on a redirect.
2. Verify the certificate and hostname with a normal TLS client. Do not disable
   certificate validation to make a probe succeed.
3. Configure the proxy to route transfer paths directly to the API handler.
   Exclude them from host canonicalization redirects, auth-to-login redirects,
   slash normalization, CDN/object-store offload, and storage URL rewriting.
4. Preserve `PATCH`, `Upload-Offset`, content type, and request body unchanged.
   Do not rewrite the method or body.
5. Do not cache authenticated transfer responses. Preserve streaming and Range
   behavior without buffering entire files.
6. Send authenticated test requests for immutable content (`Range: bytes=0-0`),
   upload status, and a one-byte synthetic PATCH to a disposable open upload
   session. The only success statuses are those in the table above.
7. Send an invalid DeviceBearer to each transfer route. Require JSON `401` or
   `403`; reject any `3xx`, HTML login page, alternate host, or scheme downgrade.
8. Use a client that does not follow redirects and inspect status plus `Location`
   headers. Never use a real bearer or user file in an adversarial redirect
   fixture. Use a test DeviceBearer and synthetic payload only.
9. Verify that no redirect is emitted for `301`, `302`, `303`, `307`, or `308`
   by the API and by the tested deployment edge. Test same-origin, cross-origin,
   multi-hop, HTTPS downgrade, unexpected port, login, and storage/CDN redirect
   rules in a controlled non-production fixture.
10. Record public origin, proxy/CDN version and configuration hash, API build,
    test time, request IDs, observed status codes, and `Location` presence. Do
    not record credentials, file content, private paths, or response bodies.

For a repeatable live probe, use a dedicated test DeviceBearer in a temporary
curl config file with mode `0600` (never put the value in shell history or a
committed file). `curl` does not follow redirects unless `--location` is set;
omit that option and inspect the status and effective URL:

```sh
curl --config "$SYNVEIL_TEST_CURL_CONFIG" --max-redirs 0 \
  --silent --show-error --dump-header /tmp/synveil-transfer-headers \
  --output /dev/null --write-out '%{http_code} %{url_effective}\n' \
  --range 0-0 "$SYNVEIL_PUBLIC_ORIGIN/api/v1/versions/$TEST_VERSION_ID/content"

curl --config "$SYNVEIL_TEST_CURL_CONFIG" --max-redirs 0 \
  --silent --show-error --dump-header /tmp/synveil-transfer-patch-headers \
  --output /dev/null --write-out '%{http_code} %{url_effective}\n' \
  --request PATCH --header 'Upload-Offset: 0' \
  --header 'Content-Type: application/octet-stream' \
  --data-binary @sentinel-one-byte.bin \
  "$SYNVEIL_PUBLIC_ORIGIN/api/v1/upload-sessions/$TEST_UPLOAD_SESSION_ID"
```

Use an immutable test version and a disposable test upload session prepared
for exactly the synthetic one-byte payload. The PATCH may durably accept that
byte; abort the disposable session afterward. A correct nonempty content range
probe returns `206`; the exact valid test PATCH returns `204`. Remove the
temporary curl config and response-header files after recording status and
whether any `Location` header appeared. This procedure validates the tested
public edge at that time. It does not model iOS redirect forwarding, and it
does not certify a deployment after its proxy/DNS configuration changes.

An API-level negative test proves only that the API handler rejects an
unexpected redirect response. It does not prove that an external proxy is
configured correctly. A live validation is evidence for the tested deployment
at that time, not a guarantee for an administrator-managed configuration that
can change later.

## Capability state

Background upload and download have independent typed capability decisions.
Both remain unavailable in the production iOS composition while redirect policy
cannot be bound to trusted, current deployment evidence. A client preference,
successful mocked test, API status response, or valid TLS certificate does not
open the gate. Foreground P043 downloads and P045 uploads retain their existing
redirect protections. Do not enable an OS-managed background task until a
separately reviewed mechanism can establish and maintain the necessary
deployment security properties for the lifetime of the task.

The selected remediation is defense in depth at the API process plus an
operator validation contract, while both native background capabilities stay
closed. The repository has no controlled proxy configuration or live
deployment evidence, so these changes do not certify any installation.

Rejected approaches:

- Relying on `willPerformHTTPRedirection` for a background session: Apple does
  not invoke that callback for the redirect decision.
- Relying on TLS, final response origin, status, or SHA-256 after transfer:
  these checks cannot reverse a credential or upload-body disclosure.
- Treating a clean direct-Axum test, a client preference, or an unauthenticated
  server capability response as proof about an operator's proxy.
- Continuing to submit the general DeviceBearer credential in an OS-managed
  request while assuming iOS strips it on cross-origin redirects.
- Introducing a scoped short-lived transfer ticket now: it would need a new
  authorized server protocol, exact operation/method/audience/expiry scope,
  replay and revocation rules, and logout/account-switch behavior. A ticket
  alone does not protect a `307`/`308` upload body from forwarding, so it would
  not resolve the P046 content-disclosure risk without a separate body-safe
  request design and independent review.

No transfer credential endpoint or capability assertion was added. This keeps
the existing foreground contracts intact and leaves the next safe step as
obtaining a reviewed, deployment-bound design before reopening either native
background capability.

## iPhone validation procedure and current evidence

Physical-device evidence for this remediation is `NOT_AVAILABLE`: no signed
P051A build, iPhone, iOS version, API deployment, proxy configuration hash, or
runtime transfer capture was available. The procedure below is preparation for
a future reviewed build; the current background upload and download actions
remain disabled and must not be treated as testable production features.

After the security gate is independently approved, use a disposable test
account and DeviceBearer with no access to real user content, a test Library,
an immutable fixture version, and synthetic upload files with known length and
SHA-256. Run against every topology the operator intends to support and the
no-follow deployment probes above. The repository does not ship a direct API
TLS listener or a reverse-proxy configuration, so such a deployment must
identify and test its own TLS-terminating edge; no topology is certified here.
For any intentional redirect-capture test, use only a throwaway credential and
synthetic sentinel bytes, and a controlled destination; never use an ordinary
DeviceBearer or user file.

Record the signed build SHA, device model, iOS version, server/API build,
deployment topology, proxy/configuration hash, test timestamps, task callback
evidence and observed server offsets. Exercise:

1. Start one explicitly authorized immutable download and one file-backed
   background upload, then lock and background the app while each is active.
2. Verify native progress, iOS scheduling delays, Wi-Fi/cellular transition,
   Low Power Mode, protected-data availability, and large-file disk pressure.
3. Verify a normal system termination and relaunch restores only exact owned
   tasks. Test user force-quit separately and record that automatic completion
   or relaunch is not guaranteed.
4. Exercise credential revocation, logout, account switch, and late callback;
   verify no old-account bytes publish or authorize continuation.
5. Exercise an unexpected redirect with throwaway test authorization and
   synthetic upload bytes. Observe credential/body forwarding only as a test
   result; never infer it from a mock or from TLS validation.
6. Exercise ambiguous upload cancellation and reconcile only from the
   server-authoritative offset. Truncate/corrupt a download fixture and verify
   it cannot reach Quick Look, Share Sheet, VERIFIED state, or a P049
   completion notification.
7. Run background upload and download concurrently with P048 metadata refresh;
   confirm independent session/task ownership. Verify P049 eligibility only
   after durable VERIFIED content and verify Quick Look/export reuse that same
   validated artifact.

Do not run step 1 or any credential-bearing OS-managed transfer with the current
production build: both capability gates are closed. A future implementation
must first satisfy the API and deployment contract and receive independent
security acceptance. Simulator tests cannot substitute for this device record.
