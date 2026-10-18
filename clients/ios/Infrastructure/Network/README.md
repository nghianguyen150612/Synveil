# Synveil iOS — Network Infrastructure (`clients/ios/Infrastructure/Network/`)

## Purpose & Ownership
Implements `HTTPTransportProtocol` using Apple `URLSession`.

### Responsibilities
- Executing HTTPS requests to canonical Synveil origins.
- Production HTTPS certificate validation and TLS policy enforcement (numeric loopback `127.0.0.1` permitted in `#if DEBUG` builds only; `localhost` or `.local` strictly rejected).
- Dynamic `Authorization: Bearer svd1_...` header injection from in-memory session.
- Single-shot enrollment exchange (`sve1_`) with zero automatic retries or redirects.
- Response body buffer limits (64 KiB for health, 512 KiB for metadata).
- Custom User-Agent header: `Synveil-iOS/0.1.0 (iOS <OSVersion>; <DeviceModel>)`.

## Prohibited
- Persisting bearer tokens in network response logs.
- Trusting cleartext HTTP for non-loopback or production origins.
