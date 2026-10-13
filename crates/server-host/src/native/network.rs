use super::*;
use std::{
    net::{IpAddr, Ipv4Addr},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use synveil_core::host_trust::ManagedHostTrust;
use synveil_server_config::{
    EdgeListener, FirewallManager, FirewallState, NetworkIntegration, NetworkIntegrationId,
    NetworkTrust, ReachabilityMode,
};
use synveil_server_network::{
    FirewallPlan, InterfaceKind, MANAGED_BACKEND, NetworkCandidate, NetworkInstallPlan, TrustMode,
    render_managed_edge_config,
};

pub fn candidates() -> Result<Vec<NetworkCandidate>> {
    let bytes = native::run(
        Path::new("/usr/bin/ip"),
        &[
            "-j".to_owned(),
            "-d".to_owned(),
            "address".to_owned(),
            "show".to_owned(),
        ],
        &[],
        Duration::from_secs(10),
    )?;
    let value: Vec<serde_json::Value> =
        serde_json::from_slice(&bytes).map_err(|_| "network interface inspection unavailable")?;
    let mut result = Vec::new();
    let routes = native::run(
        Path::new("/usr/bin/ip"),
        &["-j", "route", "show", "default"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>(),
        &[],
        Duration::from_secs(10),
    )
    .ok()
    .and_then(|bytes| serde_json::from_slice::<Vec<serde_json::Value>>(&bytes).ok())
    .unwrap_or_default();
    for entry in value {
        let Some(name) = entry["ifname"].as_str() else {
            continue;
        };
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            continue;
        }
        let link_kind = entry["linkinfo"]["info_kind"].as_str().unwrap_or("");
        let kind = if ["bridge", "veth", "macvlan", "ipvlan"].contains(&link_kind)
            || Path::new("/sys/class/net")
                .join(name)
                .join("bridge")
                .is_dir()
        {
            InterfaceKind::ContainerBridge
        } else if ["tun", "wireguard", "vxlan", "gre", "gretap", "sit", "ipip"].contains(&link_kind)
        {
            InterfaceKind::Tunnel
        } else if Path::new("/sys/class/net")
            .join(name)
            .join("wireless")
            .is_dir()
        {
            InterfaceKind::Wifi
        } else if name.starts_with("docker")
            || name.starts_with("br-")
            || name.starts_with("veth")
            || name.starts_with("virbr")
        {
            InterfaceKind::ContainerBridge
        } else if name.starts_with("tun")
            || name.starts_with("tap")
            || name.starts_with("wg")
            || name.starts_with("tailscale")
        {
            InterfaceKind::Tunnel
        } else if Path::new("/sys/class/net")
            .join(name)
            .join("device")
            .exists()
        {
            InterfaceKind::Ethernet
        } else {
            InterfaceKind::Other
        };
        if let Some(addresses) = entry["addr_info"].as_array() {
            for address in addresses {
                if let (Some(address), Some(prefix)) = (
                    address["local"]
                        .as_str()
                        .and_then(|v| v.parse::<IpAddr>().ok()),
                    address["prefixlen"].as_u64(),
                ) {
                    result.push(NetworkCandidate {
                        interface_id: name.to_owned(),
                        address,
                        prefix_length: prefix.min(255) as u8,
                        kind,
                        operational: entry["operstate"].as_str() == Some("UP"),
                        default_route: routes
                            .iter()
                            .any(|route| route["dev"].as_str() == Some(name)),
                    });
                }
            }
        }
    }
    Ok(result
        .into_iter()
        .filter(|candidate| {
            candidate.suitable_private_lan()
                && matches!(
                    candidate.kind,
                    InterfaceKind::Ethernet | InterfaceKind::Wifi
                )
        })
        .take(64)
        .collect())
}
pub fn validate_selection(selection: &HostSelection) -> Result<()> {
    let origin =
        url::Url::parse(&selection.canonical_origin).map_err(|_| "invalid HTTPS address")?;
    if origin.scheme() != "https"
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
        || origin.as_str() != selection.canonical_origin
        || origin.port_or_known_default() != Some(443)
    {
        return Err("a canonical HTTPS origin on port 443 is required");
    }
    let address: IpAddr = selection
        .listener_address
        .parse()
        .map_err(|_| "invalid listener address")?;
    match selection.mode {
        ReachabilityMode::LocalOnly => {
            if address != IpAddr::V4(Ipv4Addr::LOCALHOST)
                || selection.interface_id.is_some()
                || origin.host_str() != Some("localhost")
            {
                return Err(
                    "Local Only uses the reviewed loopback listener and localhost certificate",
                );
            }
        }
        ReachabilityMode::PrivateLan => {
            let found = candidates()?.iter().any(|c| {
                Some(&c.interface_id) == selection.interface_id.as_ref()
                    && c.address == address
                    && c.suitable_private_lan()
            });
            if !found || origin.host_str() != Some(selection.listener_address.as_str()) {
                return Err("select an observed private LAN address and interface");
            }
            firewall_manager()?;
        }
        ReachabilityMode::AdvancedExternalHttps => {
            if selection.profile != DeploymentProfile::AdvancedExternal
                || !address.is_loopback()
                || selection.interface_id.is_some()
            {
                return Err("external HTTPS is operator-owned and proxies to the private backend");
            }
        }
    }
    Ok(())
}

pub fn reviewed_plan(
    selection: &HostSelection,
    config: &ServerConfig,
    network_id: &str,
    fingerprint: &str,
) -> Result<NetworkInstallPlan> {
    validate_selection(selection)?;
    let manager = if selection.mode == ReachabilityMode::PrivateLan {
        Some(firewall_manager()?)
    } else {
        None
    };
    Ok(NetworkInstallPlan {
        network_integration_id: NetworkIntegrationId::parse(network_id.to_owned())
            .map_err(|_| "invalid reviewed network identity")?,
        server_installation_id: config.server_installation_id.clone(),
        service_integration_id: "synveil-server.target".to_owned(),
        config_generation: config.generation,
        config_fingerprint: fingerprint.to_owned(),
        mode: selection.mode,
        backend_endpoint: MANAGED_BACKEND.to_owned(),
        listener: selection
            .listener_address
            .parse()
            .map_err(|_| "invalid reviewed listener")?,
        port: 443,
        interface_id: selection.interface_id.clone(),
        canonical_origin: selection.canonical_origin.clone(),
        trust_mode: if selection.mode == ReachabilityMode::AdvancedExternalHttps {
            TrustMode::PublicWebPki
        } else {
            TrustMode::ManagedPrivateCa
        },
        firewall: match manager {
            Some(manager) => FirewallPlan::CreateOwned {
                manager,
                rule_id: format!("synveil-{network_id}"),
                interface_id: selection
                    .interface_id
                    .clone()
                    .ok_or("selected interface missing")?,
                port: 443,
            },
            None if selection.mode == ReachabilityMode::AdvancedExternalHttps => {
                FirewallPlan::OperatorOwned
            }
            None => FirewallPlan::None,
        },
    })
}
pub fn input_fingerprint(selection: &HostSelection) -> Result<String> {
    let manager = if selection.mode == ReachabilityMode::PrivateLan {
        Some(firewall_manager()?)
    } else {
        None
    };
    let selected = if selection.mode == ReachabilityMode::PrivateLan {
        candidates()?.into_iter().find(|candidate| {
            Some(&candidate.interface_id) == selection.interface_id.as_ref()
                && candidate.address.to_string() == selection.listener_address
        })
    } else {
        None
    };
    serde_json::to_vec(&(manager, selected))
        .map(|bytes| sha256(&bytes))
        .map_err(|_| "network input encoding failed")
}
fn openssl(args: &[String], input: &[u8]) -> Result<Vec<u8>> {
    native::run(
        Path::new("/usr/bin/openssl"),
        args,
        input,
        Duration::from_secs(30),
    )
}
fn create_key(owner: &NativeOwners, ca: bool) -> Result<()> {
    if owner.store.host_network_key(ca).is_ok() {
        return Ok(());
    }
    let bytes = zeroize::Zeroizing::new(openssl(
        &[
            "genpkey".to_owned(),
            "-algorithm".to_owned(),
            "EC".to_owned(),
            "-pkeyopt".to_owned(),
            "ec_paramgen_curve:P-256".to_owned(),
        ],
        &[],
    )?);
    let pem = std::str::from_utf8(&bytes).map_err(|_| "invalid generated TLS key")?;
    owner
        .store
        .create_host_network_key(ca, pem)
        .map_err(|_| "TLS key exists or needs preserving repair")
}
fn certificates(owner: &NativeOwners, selection: &HostSelection) -> Result<ManagedHostTrust> {
    native::directory(Path::new("/etc/synveil/network"), 0o755)?;
    let journal = owner.journal()?.ok_or("network intent missing")?;
    if Path::new("/etc/synveil/network/ca-certificate.pem").exists()
        && owner.store.host_network_key(true).is_err()
    {
        return Err(
            "approved CA private key is unavailable; restore it or explicitly review CA replacement",
        );
    }
    create_key(owner, true)?;
    create_key(owner, false)?;
    for name in ["network-ca-key", "network-tls-key"] {
        native::root_path(&Path::new("/etc/synveil/credentials").join(name))?;
    }
    let ca_path = Path::new("/etc/synveil/network/ca-certificate.pem");
    if !ca_path.exists() {
        let args = vec![
            "req",
            "-new",
            "-x509",
            "-sha256",
            "-key",
            "/etc/synveil/credentials/network-ca-key",
            "-days",
            "3650",
            "-subj",
        ]
        .into_iter()
        .map(str::to_owned)
        .chain([format!("/CN=Synveil {}", journal.server_id)])
        .chain(
            [
                "-addext",
                "basicConstraints=critical,CA:TRUE",
                "-addext",
                "keyUsage=critical,keyCertSign,cRLSign",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .collect::<Vec<_>>();
        owner.publish(ca_path, &openssl(&args, &[])?, 0o644)?;
    }
    let ca = String::from_utf8(native::read_owned(ca_path, 16 * 1024)?)
        .map_err(|_| "invalid public CA")?;
    let config = owner.config()?.ok_or("configuration missing")?;
    let descriptor = ManagedHostTrust::from_ca(
        journal.server_id.clone(),
        journal.network_id.clone(),
        selection.canonical_origin.clone(),
        ca,
        config.generation,
    )
    .map_err(|_| "invalid managed CA")?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "clock unavailable")?
        .as_secs() as i64;
    descriptor.certificate_der(&selection.canonical_origin, now)?;
    let ca_public = openssl(
        &[
            "pkey",
            "-in",
            "/etc/synveil/credentials/network-ca-key",
            "-pubout",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>(),
        &[],
    )?;
    let certificate_public = openssl(
        &[
            "x509",
            "-in",
            "/etc/synveil/network/ca-certificate.pem",
            "-pubkey",
            "-noout",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>(),
        &[],
    )?;
    if ca_public != certificate_public {
        return Err("protected CA key does not match the approved public CA");
    }
    if let NetworkConfiguration::Preparing(network) | NetworkConfiguration::Configured(network) =
        &config.network
    {
        if network.network_integration_id.as_str() != journal.network_id
            || network.trust
                != (NetworkTrust::ManagedPrivateCa {
                    ca_certificate_sha256: descriptor.ca_certificate_sha256.clone(),
                })
        {
            return Err("CA or network identity changed; fresh operator approval is required");
        }
    }
    let leaf = Path::new("/etc/synveil/network/server-certificate.pem");
    let key_public = openssl(
        &[
            "pkey",
            "-in",
            "/etc/synveil/credentials/network-tls-key",
            "-pubout",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>(),
        &[],
    )?;
    let leaf_public = if leaf.exists() {
        openssl(
            &[
                "x509",
                "-in",
                "/etc/synveil/network/server-certificate.pem",
                "-pubkey",
                "-noout",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>(),
            &[],
        )
        .ok()
    } else {
        None
    };
    let san_args = if selection.mode == ReachabilityMode::LocalOnly {
        vec!["-verify_hostname".to_owned(), "localhost".to_owned()]
    } else {
        vec!["-verify_ip".to_owned(), selection.listener_address.clone()]
    };
    let san_valid = leaf.exists()
        && openssl(
            &[
                vec![
                    "verify".to_owned(),
                    "-purpose".to_owned(),
                    "sslserver".to_owned(),
                ],
                san_args.clone(),
                vec![
                    "-CAfile".to_owned(),
                    "/etc/synveil/network/ca-certificate.pem".to_owned(),
                    "/etc/synveil/network/server-certificate.pem".to_owned(),
                ],
            ]
            .concat(),
            &[],
        )
        .is_ok();
    let needs_renewal = leaf_public.as_deref() != Some(key_public.as_slice())
        || !leaf.exists()
        || !san_valid
        || openssl(
            &[
                "x509",
                "-checkend",
                "2592000",
                "-noout",
                "-in",
                leaf.to_str().ok_or("invalid leaf path")?,
            ]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>(),
            &[],
        )
        .is_err();
    if needs_renewal {
        let san = if selection.mode == ReachabilityMode::LocalOnly {
            "DNS:localhost".to_owned()
        } else {
            format!("IP:{}", selection.listener_address)
        };
        let csr = openssl(
            &[
                "req",
                "-new",
                "-sha256",
                "-key",
                "/etc/synveil/credentials/network-tls-key",
                "-subj",
                "/CN=Synveil",
                "-addext",
            ]
            .into_iter()
            .map(str::to_owned)
            .chain([format!("subjectAltName={san}")])
            .collect::<Vec<_>>(),
            &[],
        )?;
        let args = [
            "x509",
            "-req",
            "-CA",
            "/etc/synveil/network/ca-certificate.pem",
            "-CAkey",
            "/etc/synveil/credentials/network-ca-key",
            "-days",
            "90",
            "-sha256",
            "-copy_extensions",
            "copy",
            "-set_serial",
        ]
        .into_iter()
        .map(str::to_owned)
        .chain([format!("0x{}", uuid::Uuid::now_v7().simple())])
        .collect::<Vec<_>>();
        owner.publish(leaf, &openssl(&args, &csr)?, 0o644)?;
    }
    // Validate key/certificate/CA binding with OpenSSL before daemon activation.
    openssl(
        &[
            vec![
                "verify".to_owned(),
                "-purpose".to_owned(),
                "sslserver".to_owned(),
            ],
            san_args,
            vec![
                "-CAfile".to_owned(),
                "/etc/synveil/network/ca-certificate.pem".to_owned(),
                "/etc/synveil/network/server-certificate.pem".to_owned(),
            ],
        ]
        .concat(),
        &[],
    )?;
    Ok(descriptor)
}
fn firewall_manager() -> Result<FirewallManager> {
    let firewalld = native::systemctl(&["is-active", "--quiet", "firewalld.service"]).is_ok();
    let ufw = native::run(
        Path::new("/usr/sbin/ufw"),
        &["status".to_owned()],
        &[],
        Duration::from_secs(10),
    )
    .is_ok_and(|bytes| {
        String::from_utf8_lossy(&bytes)
            .lines()
            .any(|line| line == "Status: active")
    });
    match (ufw, firewalld) {
        (true, false) => Ok(FirewallManager::Ufw),
        (false, true) => Ok(FirewallManager::Firewalld),
        _ => Err(
            "device access requires one supported active firewall manager or operator reconciliation",
        ),
    }
}
fn firewall_rule(network: &NetworkIntegration) -> Result<(FirewallManager, Vec<String>)> {
    let (FirewallState::Planned { manager, rule_id } | FirewallState::Applied { manager, rule_id }) =
        &network.firewall
    else {
        return Err("owned firewall intent missing");
    };
    let interface = network
        .listener
        .interface_id
        .as_ref()
        .ok_or("firewall interface missing")?;
    if *manager == FirewallManager::Ufw {
        Ok((
            *manager,
            [
                "allow",
                "in",
                "on",
                interface,
                "to",
                &network.listener.address.to_string(),
                "port",
                "443",
                "proto",
                "tcp",
                "comment",
                rule_id,
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        ))
    } else {
        // A dedicated permanent policy is scoped to this interface and exact
        // destination. Never change the default zone or reload global rules.
        Ok((
            *manager,
            vec![format!(
                "rule family=\"ipv4\" destination address=\"{}\" port port=\"443\" protocol=\"tcp\" log prefix=\"{}\" limit value=\"1/m\" accept",
                network.listener.address, rule_id
            )],
        ))
    }
}
fn firewall_zone(interface: &str) -> Result<String> {
    let program = Path::new("/usr/bin/firewall-cmd");
    let bytes = native::run(
        program,
        &[format!("--get-zone-of-interface={interface}")],
        &[],
        Duration::from_secs(10),
    )?;
    let zone = std::str::from_utf8(&bytes)
        .map_err(|_| "firewall zone unavailable")?
        .trim()
        .to_owned();
    if zone.is_empty()
        || zone.len() > 64
        || !zone
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err("invalid firewall zone");
    }
    for permanent in [false, true] {
        let mut base = vec![format!("--zone={zone}")];
        if permanent {
            base.insert(0, "--permanent".to_owned());
        }
        let mut command = base.clone();
        command.push("--list-interfaces".to_owned());
        let bytes = native::run(program, &command, &[], Duration::from_secs(10))?;
        if std::str::from_utf8(&bytes)
            .map_err(|_| "invalid firewall interface set")?
            .split_whitespace()
            .collect::<Vec<_>>()
            != [interface]
        {
            return Err(
                "selected firewall zone has other interfaces; operator reconciliation is required before scoped Host access",
            );
        }
        let mut command = base;
        command.push("--list-sources".to_owned());
        if native::run(program, &command, &[], Duration::from_secs(10))?
            .iter()
            .any(|byte| !byte.is_ascii_whitespace())
        {
            return Err(
                "selected firewall zone has other sources; operator reconciliation required",
            );
        }
    }
    Ok(zone)
}
fn firewall_presence(network: &NetworkIntegration) -> Result<(bool, bool)> {
    let (manager, args) = firewall_rule(network)?;
    if manager != firewall_manager()? {
        return Err("firewall manager changed");
    }
    if manager == FirewallManager::Ufw {
        let bytes = native::run(
            Path::new("/usr/sbin/ufw"),
            &["status".to_owned(), "verbose".to_owned()],
            &[],
            Duration::from_secs(10),
        )?;
        let id = match &network.firewall {
            FirewallState::Planned { rule_id, .. } | FirewallState::Applied { rule_id, .. } => {
                rule_id
            }
            _ => return Err("firewall rule identity missing"),
        };
        let text = std::str::from_utf8(&bytes).map_err(|_| "invalid UFW inspection")?;
        let lines: Vec<_> = text
            .lines()
            .filter_map(|line| line.split_once('#'))
            .filter(|(_, comment)| comment.trim() == id)
            .collect();
        if lines.is_empty() {
            return Ok((false, false));
        }
        if lines.len() != 1 {
            return Err("ambiguous UFW rule identity");
        }
        let tokens: Vec<_> = lines[0].0.split_whitespace().collect();
        let ip = network.listener.address.to_string();
        let interface = network
            .listener
            .interface_id
            .as_deref()
            .ok_or("firewall interface missing")?;
        if tokens
            != [
                ip.as_str(),
                "443/tcp",
                "ALLOW",
                "IN",
                "Anywhere",
                "on",
                interface,
            ]
        {
            return Err("owned UFW rule definition was changed");
        }
        return Ok((true, true));
    }
    let zone = firewall_zone(
        network
            .listener
            .interface_id
            .as_deref()
            .ok_or("firewall interface missing")?,
    )?;
    let mut states = Vec::new();
    for permanent in [false, true] {
        let mut command = vec![
            format!("--zone={zone}"),
            format!("--query-rich-rule={}", args[0]),
        ];
        if permanent {
            command.insert(0, "--permanent".to_owned());
        }
        let (status, bytes) = native::run_status(
            Path::new("/usr/bin/firewall-cmd"),
            &command,
            &[],
            Duration::from_secs(10),
        )?;
        let value = std::str::from_utf8(&bytes)
            .map_err(|_| "invalid firewall query")?
            .trim();
        match (status, value) {
            (0, "yes") => states.push(true),
            (1, "no") => states.push(false),
            _ => return Err("firewall rule state is unknown"),
        }
    }
    Ok((states[0], states[1]))
}
fn firewall_apply(owner: &NativeOwners, network: &NetworkIntegration) -> Result<()> {
    let (manager, args) = firewall_rule(network)?;
    let id = match &network.firewall {
        FirewallState::Planned { rule_id, .. } | FirewallState::Applied { rule_id, .. } => rule_id,
        _ => return Err("firewall intent missing"),
    };
    let interface = network
        .listener
        .interface_id
        .as_ref()
        .ok_or("firewall interface missing")?;
    let receipt = Path::new("/etc/synveil/network/firewall-intent.json");
    let presence = firewall_presence(network)?;
    if !receipt.exists() && (presence.0 || presence.1) {
        return Err("foreign firewall rule identity already exists; adoption is forbidden");
    }
    owner.publish(
        receipt,
        &serde_json::to_vec(&(id, manager, interface, &args))
            .map_err(|_| "firewall intent encoding failed")?,
        0o600,
    )?;
    if manager == FirewallManager::Ufw {
        if !presence.0 {
            native::run(
                Path::new("/usr/sbin/ufw"),
                &args,
                &[],
                Duration::from_secs(30),
            )?;
        }
    } else {
        let zone = firewall_zone(interface)?;
        for (permanent, present) in [(false, presence.0), (true, presence.1)] {
            if present {
                continue;
            }
            let mut command = vec![
                format!("--zone={zone}"),
                format!("--add-rich-rule={}", args[0]),
            ];
            if permanent {
                command.insert(0, "--permanent".to_owned());
            }
            native::run(
                Path::new("/usr/bin/firewall-cmd"),
                &command,
                &[],
                Duration::from_secs(30),
            )?;
        }
    }
    firewall_verify(network)
}
fn firewall_verify(network: &NetworkIntegration) -> Result<()> {
    if network.mode == ReachabilityMode::LocalOnly {
        return if network.firewall == FirewallState::NotRequired {
            Ok(())
        } else {
            Err("unexpected Local Only firewall state")
        };
    }
    if network.mode == ReachabilityMode::AdvancedExternalHttps {
        return if network.firewall == FirewallState::OperatorOwned {
            Ok(())
        } else {
            Err("external firewall ownership mismatch")
        };
    }
    if firewall_presence(network)? != (true, true) {
        return Err("owned firewall rule missing or only partially persisted");
    }
    Ok(())
}
pub fn remove_owned_firewall(owner: &NativeOwners) -> Result<()> {
    let Some(config) = owner.config()? else {
        return Ok(());
    };
    let (NetworkConfiguration::Preparing(network) | NetworkConfiguration::Configured(network)) =
        config.network
    else {
        return Ok(());
    };
    if network.mode != ReachabilityMode::PrivateLan {
        return Ok(());
    }
    let journal = owner
        .journal()?
        .ok_or("firewall ownership intent missing")?;
    let receipt = Path::new("/etc/synveil/network/firewall-intent.json");
    let key = receipt.to_str().ok_or("invalid firewall intent path")?;
    if !receipt.exists() {
        return if journal.owned_files.contains_key(key) {
            Err("firewall intent missing; unknown rule preserved")
        } else {
            Ok(())
        };
    }
    let bytes = native::read_owned(receipt, 16 * 1024)?;
    if !journal.owns_file_outcome(key, &sha256(&bytes)) {
        return Err("firewall ownership changed; rules preserved");
    }
    let (manager, args) = firewall_rule(&network)?;
    let presence = firewall_presence(&network)?;
    if manager == FirewallManager::Ufw {
        if presence.0 {
            let mut command = vec!["--force".to_owned(), "delete".to_owned()];
            command.extend(args);
            native::run(
                Path::new("/usr/sbin/ufw"),
                &command,
                &[],
                Duration::from_secs(30),
            )?;
        }
    } else {
        let zone = firewall_zone(
            network
                .listener
                .interface_id
                .as_deref()
                .ok_or("owned firewall interface missing")?,
        )?;
        for (permanent, present) in [(false, presence.0), (true, presence.1)] {
            if !present {
                continue;
            }
            let mut command = vec![
                format!("--zone={zone}"),
                format!("--remove-rich-rule={}", args[0]),
            ];
            if permanent {
                command.insert(0, "--permanent".to_owned());
            }
            native::run(
                Path::new("/usr/bin/firewall-cmd"),
                &command,
                &[],
                Duration::from_secs(30),
            )?;
        }
    }
    if firewall_presence(&network)? != (false, false) {
        return Err("owned firewall rule removal outcome unknown");
    }
    owner.remove_owned_file(receipt)
}

pub fn configure(
    owner: &NativeOwners,
    catalog: &RuntimeCatalog,
    selection: &HostSelection,
) -> Result<()> {
    validate_selection(selection)?;
    let journal = owner.journal()?.ok_or("network intent missing")?;
    if let Some(config) = owner.config()? {
        if let NetworkConfiguration::Preparing(previous)
        | NetworkConfiguration::Configured(previous) = &config.network
        {
            let changed = previous.mode != selection.mode
                || previous.canonical_origin != selection.canonical_origin
                || previous.listener.address.to_string() != selection.listener_address
                || previous.listener.interface_id != selection.interface_id;
            if changed && previous.mode == ReachabilityMode::PrivateLan {
                remove_owned_firewall(owner)?;
            }
        }
    }
    let (trust, descriptor) = if selection.mode == ReachabilityMode::AdvancedExternalHttps {
        (NetworkTrust::PublicWebPki, None)
    } else {
        let descriptor = certificates(owner, selection)?;
        (
            NetworkTrust::ManagedPrivateCa {
                ca_certificate_sha256: descriptor.ca_certificate_sha256.clone(),
            },
            Some(descriptor),
        )
    };
    let manager = if selection.mode == ReachabilityMode::PrivateLan {
        Some(firewall_manager()?)
    } else {
        None
    };
    let mut network = NetworkIntegration {
        network_integration_id: NetworkIntegrationId::parse(journal.network_id.clone())
            .map_err(|_| "invalid network identity")?,
        mode: selection.mode,
        backend_endpoint: MANAGED_BACKEND.to_owned(),
        listener: EdgeListener {
            address: selection
                .listener_address
                .parse()
                .map_err(|_| "invalid listener")?,
            port: 443,
            interface_id: selection.interface_id.clone(),
        },
        canonical_origin: selection.canonical_origin.clone(),
        trust,
        firewall: match manager {
            Some(manager) => FirewallState::Planned {
                manager,
                rule_id: format!("synveil-{}", journal.network_id),
            },
            None if selection.mode == ReachabilityMode::AdvancedExternalHttps => {
                FirewallState::OperatorOwned
            }
            None => FirewallState::NotRequired,
        },
    };
    let (_, fingerprint) = owner
        .store
        .load_config_read_only()
        .map_err(|_| "configuration invalid")?
        .ok_or("configuration missing")?;
    owner
        .store
        .update_network_local_private(
            fingerprint,
            NetworkConfiguration::Preparing(network.clone()),
        )
        .map_err(|_| "network configuration needs repair")?;
    if selection.mode != ReachabilityMode::AdvancedExternalHttps {
        let config = owner.config()?.ok_or("configuration missing")?;
        let plan = NetworkInstallPlan {
            network_integration_id: network.network_integration_id.clone(),
            server_installation_id: journal.server_id.clone(),
            service_integration_id: "synveil-server.target".to_owned(),
            config_generation: config.generation,
            config_fingerprint: owner.fingerprint()?,
            mode: selection.mode,
            backend_endpoint: MANAGED_BACKEND.to_owned(),
            listener: network.listener.address,
            port: 443,
            interface_id: selection.interface_id.clone(),
            canonical_origin: selection.canonical_origin.clone(),
            trust_mode: TrustMode::ManagedPrivateCa,
            firewall: match &network.firewall {
                FirewallState::Planned { manager, rule_id } => FirewallPlan::CreateOwned {
                    manager: *manager,
                    rule_id: rule_id.clone(),
                    interface_id: selection
                        .interface_id
                        .clone()
                        .ok_or("firewall interface missing")?,
                    port: 443,
                },
                _ => FirewallPlan::None,
            },
        };
        let text = render_managed_edge_config(&plan).map_err(|_| "edge plan rejected")?;
        owner.publish(
            Path::new("/etc/synveil/network/Caddyfile"),
            text.as_bytes(),
            0o644,
        )?;
        // Validate the exact Caddy input before starting it. The helper supplies
        // only the leaf credential directory, never the CA private key.
        // Caddy validation reads the canonical leaf key path through this
        // private environment at process launch (no untrusted caller variables).
        validate_caddy(catalog)?;
        if selection.mode == ReachabilityMode::PrivateLan {
            firewall_apply(owner, &network)?;
            if let FirewallState::Planned { manager, rule_id } = network.firewall {
                network.firewall = FirewallState::Applied { manager, rule_id };
            }
        }
    }
    services::start_application()?;
    if selection.mode != ReachabilityMode::AdvancedExternalHttps {
        native::systemctl(&["restart", "synveil-edge.service"])?;
    }
    https_health(
        &selection.canonical_origin,
        descriptor.as_ref(),
        &journal.server_id,
        &journal.network_id,
    )?;
    let (_, fingerprint) = owner
        .store
        .load_config_read_only()
        .map_err(|_| "configuration invalid")?
        .ok_or("configuration missing")?;
    owner
        .store
        .update_network_local_private(fingerprint, NetworkConfiguration::Configured(network))
        .map_err(|_| "network commit outcome uncertain; inspect before retry")?;
    services::enable(selection)
}
fn validate_caddy(catalog: &RuntimeCatalog) -> Result<()> {
    let caddy = services::runtime_root("caddy", &catalog.artifacts["caddy"]).join("bin/caddy");
    // Adapt is a syntax check which does not load the key. Runtime activation
    // uses systemd LoadCredential and validation is completed by verified HTTPS.
    native::run(
        &caddy,
        &[
            "adapt",
            "--config",
            "/etc/synveil/network/Caddyfile",
            "--adapter",
            "caddyfile",
            "--pretty",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>(),
        &[],
        Duration::from_secs(30),
    )
    .map(|_| ())
}
pub fn backend_health(kind: &str) -> bool {
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(3))
        .build();
    client.is_ok_and(|client| {
        client
            .get(format!("http://127.0.0.1:3000/health/{kind}"))
            .send()
            .is_ok_and(|response| response.status().is_success())
    })
}
pub fn backend_bound_privately() -> bool {
    native::run(
        Path::new("/usr/bin/ss"),
        &["-H", "-ltn"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>(),
        &[],
        Duration::from_secs(5),
    )
    .is_ok_and(|bytes| {
        let text = String::from_utf8_lossy(&bytes);
        let listeners: Vec<_> = text
            .lines()
            .filter_map(|line| line.split_whitespace().nth(3))
            .filter(|address| address.ends_with(":3000"))
            .collect();
        listeners.len() == 1 && listeners[0] == "127.0.0.1:3000"
    })
}
fn https_health(
    origin: &str,
    descriptor: Option<&ManagedHostTrust>,
    server: &str,
    network: &str,
) -> Result<()> {
    let mut builder = reqwest::blocking::Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10));
    if let Some(descriptor) = descriptor {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "clock unavailable")?
            .as_secs() as i64;
        let der = descriptor.certificate_der(origin, now)?;
        builder = builder.tls_built_in_root_certs(false).add_root_certificate(
            reqwest::Certificate::from_der(&der).map_err(|_| "CA certificate rejected")?,
        );
    }
    let client = builder.build().map_err(|_| "HTTPS verifier unavailable")?;
    let response = client
        .get(format!("{}health/ready", origin))
        .send()
        .map_err(|_| "verified HTTPS endpoint unavailable")?;
    if !response.status().is_success()
        || response
            .headers()
            .get("x-synveil-server-installation")
            .and_then(|v| v.to_str().ok())
            != Some(server)
        || response
            .headers()
            .get("x-synveil-network-integration")
            .and_then(|v| v.to_str().ok())
            != Some(network)
    {
        return Err("HTTPS readiness or server identity mismatch");
    }
    Ok(())
}
pub fn descriptor(owner: &NativeOwners) -> Result<String> {
    let config = owner.config()?.ok_or("configuration missing")?;
    let NetworkConfiguration::Configured(network) = &config.network else {
        return Err("HTTPS integration is not configured");
    };
    if network.mode == ReachabilityMode::AdvancedExternalHttps {
        return serde_json::to_string(&serde_json::json!({"schema_version":1,"kind":"public_web_pki","server_installation_id":config.server_installation_id,"network_integration_id":network.network_integration_id.as_str(),"canonical_origin":network.canonical_origin,"config_generation":config.generation})).map_err(|_| "trust descriptor encoding failed");
    }
    let ca = String::from_utf8(native::read_owned(
        Path::new("/etc/synveil/network/ca-certificate.pem"),
        16 * 1024,
    )?)
    .map_err(|_| "invalid public CA")?;
    let descriptor = ManagedHostTrust::from_ca(
        config.server_installation_id,
        network.network_integration_id.as_str().to_owned(),
        network.canonical_origin.clone(),
        ca,
        config.generation,
    )?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "clock unavailable")?
        .as_secs() as i64;
    descriptor.certificate_der(&network.canonical_origin, now)?;
    if network.trust
        != (NetworkTrust::ManagedPrivateCa {
            ca_certificate_sha256: descriptor.ca_certificate_sha256.clone(),
        })
    {
        return Err("approved CA identity changed");
    }
    serde_json::to_string(&descriptor).map_err(|_| "trust descriptor encoding failed")
}
pub fn observe(
    owner: &NativeOwners,
    config: &ServerConfig,
    observation: &mut OwnerObservation,
) -> Result<()> {
    let NetworkConfiguration::Configured(network) = &config.network else {
        return Ok(());
    };
    let journal = owner.journal()?.ok_or("network intent missing")?;
    let selection = journal
        .pending_network_selection
        .as_ref()
        .unwrap_or(&journal.selection);
    if network.mode != ReachabilityMode::AdvancedExternalHttps
        && (owner.store.host_network_key(true).is_err()
            || owner.store.host_network_key(false).is_err())
    {
        observation.needs_repair = true;
        return Ok(());
    }
    if network.network_integration_id.as_str() != journal.network_id
        || network.mode != selection.mode
        || network.canonical_origin != selection.canonical_origin
        || network.listener.address.to_string() != selection.listener_address
        || network.listener.interface_id != selection.interface_id
        || network.backend_endpoint != MANAGED_BACKEND
    {
        observation.needs_repair = true;
        return Ok(());
    }
    let descriptor: Option<ManagedHostTrust> = if network.mode
        == ReachabilityMode::AdvancedExternalHttps
    {
        None
    } else {
        Some(serde_json::from_str(&descriptor(owner)?).map_err(|_| "invalid trust descriptor")?)
    };
    observation.client_https_healthy = https_health(
        &network.canonical_origin,
        descriptor.as_ref(),
        &config.server_installation_id,
        &journal.network_id,
    )
    .is_ok();
    observation.network_ready = observation.client_https_healthy
        && firewall_verify(network).is_ok()
        && validate_selection(selection).is_ok()
        && (network.mode == ReachabilityMode::AdvancedExternalHttps
            || services::unit_active("synveil-edge.service"));
    Ok(())
}
