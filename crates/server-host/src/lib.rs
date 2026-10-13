#![forbid(unsafe_code)]
//! Production composition only; resource ownership remains with P031--P035.
pub mod client;
#[cfg(target_os = "linux")]
pub mod native;
pub mod protocol;

pub fn supported_platform() -> Option<String> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return None;
    }
    let bytes = std::fs::read_to_string("/etc/os-release").ok()?;
    let value = |key: &str| {
        bytes.lines().find_map(|line| {
            line.strip_prefix(&format!("{key}="))
                .map(|s| s.trim_matches('"'))
        })
    };
    match (value("ID"), value("VERSION_ID")) {
        (Some("ubuntu"), Some("24.04")) => Some("ubuntu-24.04".to_owned()),
        (Some("fedora"), Some("42")) => Some("fedora-42".to_owned()),
        _ => None,
    }
}
