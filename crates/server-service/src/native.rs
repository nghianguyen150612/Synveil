//! Linux native primitives. Callers are fixed owner operations, never IPC argv.
use crate::runtime::{RuntimeManifest, digest_valid, sha256};
use nix::unistd::{Gid, Uid, chown};
use std::os::unix::process::CommandExt;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, &'static str>;
pub const OUTPUT_LIMIT: usize = 128 * 1024;

fn allowed_program(path: &Path) -> bool {
    const FIXED: &[&str] = &[
        "/usr/bin/firewall-cmd",
        "/usr/bin/ip",
        "/usr/bin/openssl",
        "/usr/bin/ss",
        "/usr/bin/systemctl",
        "/usr/bin/systemd-sysusers",
        "/usr/sbin/runuser",
        "/usr/sbin/ufw",
    ];
    if FIXED.contains(&path.to_str().unwrap_or_default()) {
        return true;
    }
    let Some(path) = path.to_str() else {
        return false;
    };
    let parts: Vec<_> = path.split('/').collect();
    match parts.as_slice() {
        [
            "",
            "opt",
            "synveil",
            "postgresql",
            "17",
            digest,
            "bin",
            program,
        ] => {
            digest_valid(digest) && ["initdb", "pg_controldata", "pg_ctl", "psql"].contains(program)
        }
        ["", "opt", "synveil", "caddy", digest, "bin", "caddy"] => digest_valid(digest),
        _ => false,
    }
}

pub fn root_path(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err("absolute owned path required");
    }
    let mut part = PathBuf::from("/");
    for component in path.components().skip(1) {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err("redirected path");
        }
        part.push(component);
        let meta = fs::symlink_metadata(&part).map_err(|_| "owned path missing")?;
        if meta.file_type().is_symlink() || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
            return Err("untrusted owned path");
        }
    }
    Ok(())
}

pub fn read_owned(path: &Path, limit: usize) -> Result<Vec<u8>> {
    root_path(path)?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| "owned file unavailable")?;
    let meta = file.metadata().map_err(|_| "owned file unavailable")?;
    if !meta.is_file() || meta.nlink() != 1 || meta.len() > limit as u64 {
        return Err("invalid owned file");
    }
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "owned file unreadable")?;
    if bytes.len() > limit {
        return Err("owned file too large");
    }
    Ok(bytes)
}

pub fn directory(path: &Path, mode: u32) -> Result<()> {
    if !path.exists() {
        root_path(path.parent().ok_or("invalid directory")?)?;
        fs::create_dir(path).map_err(|_| "directory creation failed")?;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|_| "directory permissions failed")?;
        File::open(path)
            .and_then(|file| file.sync_all())
            .map_err(|_| "directory sync failed")?;
        File::open(path.parent().ok_or("invalid directory")?)
            .and_then(|file| file.sync_all())
            .map_err(|_| "directory parent sync failed")?;
    }
    root_path(path)?;
    if !fs::symlink_metadata(path)
        .map_err(|_| "directory unavailable")?
        .is_dir()
    {
        return Err("directory type mismatch");
    }
    Ok(())
}

/// Compare-and-swap replacement. Missing and exact previous bytes are the only
/// accepted states; no caller can overwrite an unrelated file.
pub fn write_owned(path: &Path, bytes: &[u8], previous: Option<&[u8]>, mode: u32) -> Result<()> {
    let parent = path.parent().ok_or("invalid owned path")?;
    root_path(parent)?;
    if path.exists() || fs::symlink_metadata(path).is_ok() {
        let current = read_owned(path, 16 * 1024 * 1024)?;
        if current == bytes {
            return Ok(());
        }
        if previous != Some(current.as_slice()) {
            return Err("foreign or changed owned file");
        }
    } else if previous.is_some() {
        return Err("owned file disappeared");
    }
    let temp = parent.join(format!(".synveil-write-{}", uuid::Uuid::now_v7()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temp)
        .map_err(|_| "atomic write failed")?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "atomic write failed")?;
    fs::rename(&temp, path).map_err(|_| "atomic publication failed")?;
    File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|_| "directory sync failed")?;
    if read_owned(path, bytes.len() + 1)? != bytes {
        return Err("atomic write readback failed");
    }
    Ok(())
}

pub fn run(program: &Path, args: &[String], input: &[u8], timeout: Duration) -> Result<Vec<u8>> {
    let (status, output) = run_status(program, args, input, timeout)?;
    if status != 0 {
        return Err("native operation failed; inspect canonical owner");
    }
    Ok(output)
}
pub fn run_status(
    program: &Path,
    args: &[String],
    input: &[u8],
    timeout: Duration,
) -> Result<(i32, Vec<u8>)> {
    if !allowed_program(program) {
        return Err("native command is outside the fixed owner allowlist");
    }
    root_path(program)?;
    if input.len() > OUTPUT_LIMIT
        || args.len() > 64
        || args.iter().any(|a| a.contains('\0') || a.len() > 4096)
    {
        return Err("invalid native command bounds");
    }
    let mut child = Command::new(program)
        .process_group(0)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .current_dir("/usr/libexec")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "native operation unavailable")?;
    let mut stdin = child.stdin.take().ok_or("native stdin unavailable")?;
    let input = zeroize::Zeroizing::new(input.to_vec());
    let writer = thread::spawn(move || stdin.write_all(&input));
    let stdout = child.stdout.take().ok_or("native stdout unavailable")?;
    let stderr = child.stderr.take().ok_or("native stderr unavailable")?;
    let reader = thread::spawn(move || {
        let mut output = Vec::new();
        stdout
            .take(OUTPUT_LIMIT as u64 + 1)
            .read_to_end(&mut output)
            .map(|_| output)
    });
    // Never return stderr: native errors can include passwords/connection URLs.
    let err_reader = thread::spawn(move || {
        let mut output = zeroize::Zeroizing::new(Vec::new());
        stderr
            .take(OUTPUT_LIMIT as u64 + 1)
            .read_to_end(&mut output)
            .map(|_| output.len())
    });
    let start = Instant::now();
    let mut completed = None;
    let status = loop {
        if completed.is_none() {
            completed = child
                .try_wait()
                .map_err(|_| "native operation wait failed")?;
        }
        if let Some(status) = completed
            && reader.is_finished()
            && err_reader.is_finished()
            && writer.is_finished()
        {
            break status;
        }
        if start.elapsed() > timeout {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(child.id() as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
            let _ = child.wait();
            return Err("native operation timed out; inspect before retry");
        }
        thread::sleep(Duration::from_millis(25));
    };
    let output = reader
        .join()
        .map_err(|_| "native output failed")?
        .map_err(|_| "native output failed")?;
    let error_length = err_reader
        .join()
        .map_err(|_| "native output failed")?
        .map_err(|_| "native output failed")?;
    writer
        .join()
        .map_err(|_| "native input failed")?
        .map_err(|_| "native input failed")?;
    if output.len() > OUTPUT_LIMIT || error_length > OUTPUT_LIMIT {
        return Err("native operation failed; inspect canonical owner");
    }
    Ok((
        status.code().ok_or("native operation was terminated")?,
        output,
    ))
}

pub fn systemctl(args: &[&str]) -> Result<Vec<u8>> {
    run(
        Path::new("/usr/bin/systemctl"),
        &args.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>(),
        &[],
        Duration::from_secs(
            if args
                .first()
                .is_some_and(|verb| ["show", "is-active", "is-enabled"].contains(verb))
            {
                10
            } else {
                120
            },
        ),
    )
}

pub fn service_user(name: &str) -> Result<(u32, u32)> {
    let user = nix::unistd::User::from_name(name)
        .map_err(|_| "service identity unavailable")?
        .ok_or("service identity missing")?;
    if user.uid.is_root() {
        return Err("service identity must be unprivileged");
    }
    Ok((user.uid.as_raw(), user.gid.as_raw()))
}
pub fn handoff_directory(path: &Path, uid: u32, gid: u32, mode: u32) -> Result<()> {
    let meta = fs::symlink_metadata(path).map_err(|_| "service directory missing")?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || ![0, uid].contains(&meta.uid())
        || meta.mode() & 0o022 != 0
    {
        return Err("foreign service directory");
    }
    chown(path, Some(Uid::from_raw(uid)), Some(Gid::from_raw(gid)))
        .map_err(|_| "service ownership failed")?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|_| "service permissions failed")
}

pub fn verify_runtime(root: &Path, expected: &str, os: &str) -> Result<RuntimeManifest> {
    if !digest_valid(expected) {
        return Err("invalid runtime digest");
    }
    let bytes = read_owned(&root.join("manifest.json"), 2 * 1024 * 1024)?;
    if fs::metadata(root.join("manifest.json"))
        .map_err(|_| "runtime manifest unavailable")?
        .mode()
        & 0o7777
        != 0o644
    {
        return Err("runtime manifest permissions mismatch");
    }
    if sha256(&bytes) != expected {
        return Err("runtime manifest identity mismatch");
    }
    let manifest: RuntimeManifest =
        serde_json::from_slice(&bytes).map_err(|_| "invalid runtime manifest")?;
    manifest.validate(os)?;
    let mut actual = std::collections::BTreeSet::new();
    fn walk(root: &Path, at: &Path, actual: &mut std::collections::BTreeSet<String>) -> Result<()> {
        root_path(at)?;
        for entry in fs::read_dir(at).map_err(|_| "runtime directory unreadable")? {
            let path = entry.map_err(|_| "runtime inventory unreadable")?.path();
            let meta = fs::symlink_metadata(&path).map_err(|_| "runtime inventory unreadable")?;
            if meta.is_dir() {
                walk(root, &path, actual)?;
            } else if meta.is_file() {
                actual.insert(
                    path.strip_prefix(root)
                        .map_err(|_| "runtime path escaped")?
                        .to_str()
                        .ok_or("invalid runtime filename")?
                        .to_owned(),
                );
            } else {
                return Err("runtime contains non-regular entry");
            }
        }
        Ok(())
    }
    walk(root, root, &mut actual)?;
    let mut expected_paths: std::collections::BTreeSet<_> =
        manifest.files.iter().map(|f| f.path.clone()).collect();
    expected_paths.insert("manifest.json".to_owned());
    if actual != expected_paths {
        return Err("runtime closed inventory mismatch");
    }
    for file in &manifest.files {
        let path = root.join(&file.path);
        let bytes = read_owned(
            &path,
            usize::try_from(file.bytes).map_err(|_| "runtime file too large")?,
        )?;
        let meta = fs::metadata(path).map_err(|_| "runtime file unavailable")?;
        if bytes.len() as u64 != file.bytes
            || sha256(&bytes) != file.sha256
            || (meta.mode() & 0o111 != 0) != file.executable
            || meta.mode() & 0o7777 != if file.executable { 0o755 } else { 0o644 }
        {
            return Err("runtime file identity mismatch");
        }
        if file.executable && file.path.starts_with("bin/") {
            use object::Object;
            let executable = object::File::parse(bytes.as_slice())
                .map_err(|_| "runtime program is not a valid executable")?;
            if executable.format() != object::BinaryFormat::Elf
                || executable.architecture() != object::Architecture::X86_64
                || !matches!(
                    executable.kind(),
                    object::ObjectKind::Executable | object::ObjectKind::Dynamic
                )
            {
                return Err("runtime executable platform/architecture mismatch");
            }
        }
        if file.path == "PROVENANCE" {
            manifest.validate_provenance(&bytes)?;
        }
    }
    Ok(manifest)
}
