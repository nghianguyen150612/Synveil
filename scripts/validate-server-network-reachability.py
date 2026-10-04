#!/usr/bin/env python3
"""Network-free Prompt034 invariants; native reachability is a separate gate."""
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
errors = []
def read(path):
    target = ROOT / path
    if not target.is_file(): errors.append(f"missing {path}"); return ""
    return target.read_text(encoding="utf-8")
def require(path, *needles):
    value = read(path)
    for needle in needles:
        if needle not in value: errors.append(f"{path}: missing {needle}")

model = read("crates/server-config/src/model.rs")
network = read("crates/server-network/src/lib.rs")
api = read("crates/api/src/bin/synveil-api.rs")
edge = read("deploy/systemd/synveil-edge.service")
docs = read("docs/v0.2/SERVER_NETWORK_REACHABILITY.md")

for item in ("LocalOnly", "PrivateLan", "AdvancedExternalHttps", "Preparing", "Configured", "NetworkIntegrationId", "canonical_origin"):
    if item not in model: errors.append(f"finite durable network model missing {item}")
require("crates/api/src/bin/synveil-api.rs", '"127.0.0.1:3000"', "with_allowed_origin(network.canonical_origin")
if "TcpListener::bind(network" in api: errors.append("API binds the client-facing listener")
require("deploy/server/postgresql.conf", "listen_addresses = '127.0.0.1'")
require("crates/server-network/src/lib.rs", 'MANAGED_BACKEND: &str = "127.0.0.1:3000"', 'url.scheme() != "https"', "CandidateNotObserved", "ContainerBridge", "Tunnel")
require("crates/server-config/src/secret.rs", "NetworkCaKey", "NetworkTlsKey", '"network-ca-key"', '"network-tls-key"')
require("deploy/systemd/synveil-edge.service", "User=synveil-edge", "LoadCredential=network-tls-key:", "CAP_NET_BIND_SERVICE", "InaccessiblePaths=/etc/synveil/credentials")
if "network-ca-key" in edge: errors.append("CA key is permanently delivered to edge")
for forbidden in ("DATABASE_URL", "rebaseline-token", "network-ca-key", "BEGIN PRIVATE KEY"):
    if forbidden in edge: errors.append(f"edge service exposes {forbidden}")
for forbidden in ("danger_accept_invalid_certs", "accept_invalid_certs", "verify_tls: false", "trust_all", "0.0.0.0:3000", "UPnP", "upnp", "iptables -F", "nft flush"):
    if forbidden in network + api + model: errors.append(f"forbidden network behavior: {forbidden}")
require("docs/v0.2/SERVER_NETWORK_REACHABILITY.md", "no firewall change", "never reselection", "never rotates the CA", "P035", "P036", "mandatory")
if "SYNVEIL_V0_2_GUIDED_SELF_HOSTING_READY" in docs: errors.append("premature guided-hosting marker")
if "create first admin" in network.lower(): errors.append("P035 first-admin work entered network crate")
require("deploy/server-network/MANIFEST", "SERVER_NETWORK_INTEGRATION", "PRESERVED", "SERVER_SECRET")

if errors:
    print("server-network validation failed:\n- " + "\n- ".join(errors), file=sys.stderr)
    sys.exit(1)
print("server network reachability validation passed")
