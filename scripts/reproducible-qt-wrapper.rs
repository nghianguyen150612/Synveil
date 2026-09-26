//! Host-side qmake/rcc wrappers used by the release artifact builder.
//!
//! CXX-Qt clears the environment before invoking Qt build tools. The selected
//! paths and epoch are therefore embedded when this small wrapper is compiled.

#[cfg(not(windows))]
use std::fs::File;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    time::{Duration, SystemTime},
};

const REAL_QMAKE: &str = env!("SYNVEIL_REAL_QMAKE");
const REAL_RCC: &str = env!("SYNVEIL_REAL_RCC");
const QT_WRAPPER_DIR: &str = env!("SYNVEIL_QT_WRAPPER_DIR");
const SOURCE_DATE_EPOCH: &str = env!("SYNVEIL_SOURCE_DATE_EPOCH");

fn main() {
    let program = env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_stem()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    let result = match program.as_str() {
        "qmake" => run_qmake(),
        "rcc" => run_rcc(),
        _ => Err(format!("unsupported Qt wrapper invocation name: {program}")),
    };

    match result {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => {
            eprintln!("[synveil-reproducible-qt] ERROR: {error}");
            std::process::exit(1);
        }
    }
}

fn run_qmake() -> Result<ExitStatus, String> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() == 2 && args[0] == "-query" && args[1] == "QT_HOST_LIBEXECS/get" {
        println!("{QT_WRAPPER_DIR}");
        return Ok(success_status());
    }

    Command::new(REAL_QMAKE)
        .args(args)
        .status()
        .map_err(|error| format!("could not run qmake at {REAL_QMAKE}: {error}"))
}

fn run_rcc() -> Result<ExitStatus, String> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    let qrc_index = args.iter().position(|argument| {
        let path = PathBuf::from(argument);
        path.extension().is_some_and(|extension| extension == "qrc") && path.is_file()
    });

    let Some(qrc_index) = qrc_index else {
        return run_real_rcc(args);
    };
    if args
        .iter()
        .any(|argument| argument == "--list" || argument == "--list-mapping")
    {
        return run_real_rcc(args);
    }

    let qrc_path = PathBuf::from(&args[qrc_index]);
    let qrc_contents = fs::read_to_string(&qrc_path)
        .map_err(|error| format!("could not read {}: {error}", qrc_path.display()))?;
    if !is_qml_module_resource_collection(&qrc_contents) {
        return run_real_rcc(args);
    }

    let normalized_qrc = normalize_qml_resources(&qrc_path, &qrc_contents)?;
    let mut normalized_args = args;
    normalized_args[qrc_index] = normalized_qrc.into_os_string();
    run_real_rcc(normalized_args)
}

fn run_real_rcc(args: Vec<std::ffi::OsString>) -> Result<ExitStatus, String> {
    Command::new(REAL_RCC)
        .args(args)
        .status()
        .map_err(|error| format!("could not run rcc at {REAL_RCC}: {error}"))
}

fn is_qml_module_resource_collection(contents: &str) -> bool {
    contents.match_indices("<qresource").any(|(start, _)| {
        let Some(relative_end) = contents[start..].find('>') else {
            return false;
        };
        let tag = &contents[start..start + relative_end + 1];
        attribute(tag, "prefix").is_some_and(|prefix| prefix.starts_with("/qt/qml/"))
    })
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let marker = format!("{name}=");
    let start = tag.find(&marker)? + marker.len();
    let quote = tag.as_bytes().get(start).copied()?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let value_start = start + 1;
    let relative_end = tag.as_bytes()[value_start..]
        .iter()
        .position(|byte| *byte == quote)?;
    Some(tag[value_start..value_start + relative_end].to_owned())
}

fn normalize_qml_resources(qrc_path: &Path, contents: &str) -> Result<PathBuf, String> {
    let epoch = SOURCE_DATE_EPOCH
        .parse::<u64>()
        .map_err(|error| format!("invalid SOURCE_DATE_EPOCH {SOURCE_DATE_EPOCH:?}: {error}"))?;
    let modified = SystemTime::UNIX_EPOCH
        .checked_add(Duration::from_secs(epoch))
        .ok_or_else(|| format!("SOURCE_DATE_EPOCH is outside the supported range: {epoch}"))?;

    let parent = qrc_path
        .parent()
        .ok_or_else(|| format!("resource collection has no parent: {}", qrc_path.display()))?;
    let stem = qrc_path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            format!(
                "resource collection has no UTF-8 name: {}",
                qrc_path.display()
            )
        })?;
    let staged_dir = parent.join(".synveil-reproducible-rcc").join(stem);
    fs::create_dir_all(&staged_dir)
        .map_err(|error| format!("could not create {}: {error}", staged_dir.display()))?;

    let mut rewritten = contents.to_owned();
    let mut replacements = Vec::new();
    let mut search_from = 0;
    let mut index = 0usize;

    while let Some(relative_start) = contents[search_from..].find("<file") {
        let open_start = search_from + relative_start;
        let open_end = contents[open_start..]
            .find('>')
            .map(|offset| open_start + offset)
            .ok_or_else(|| format!("malformed <file> in {}", qrc_path.display()))?;
        let close_start = contents[open_end + 1..]
            .find("</file>")
            .map(|offset| open_end + 1 + offset)
            .ok_or_else(|| format!("missing </file> in {}", qrc_path.display()))?;
        let close_end = close_start + "</file>".len();
        let tag = &contents[open_start..=open_end];
        if attribute(tag, "alias").is_none() {
            return Err(format!(
                "QML resource has no explicit alias in {}",
                qrc_path.display()
            ));
        }

        let source_text = contents[open_end + 1..close_start].trim();
        let source_text = decode_xml_text(source_text)?;
        let source_path = PathBuf::from(source_text);
        let source_path = if source_path.is_absolute() {
            source_path
        } else {
            parent.join(source_path)
        };
        let source_path = fs::canonicalize(&source_path).map_err(|error| {
            format!(
                "could not resolve QML resource {}: {error}",
                source_path.display()
            )
        })?;

        let staged_name = format!("resource-{index:04}");
        let staged_path = staged_dir.join(&staged_name);
        fs::copy(&source_path, &staged_path).map_err(|error| {
            format!(
                "could not stage QML resource {}: {error}",
                source_path.display()
            )
        })?;
        set_modified_time(&staged_path, modified).map_err(|error| {
            format!(
                "could not normalize mtime for {}: {error}",
                staged_path.display()
            )
        })?;

        let replacement = format!(
            "<file{}>{staged_name}</file>",
            &tag["<file".len()..tag.len() - 1]
        );
        replacements.push((open_start, close_end, replacement));
        index += 1;
        search_from = close_end;
    }

    if replacements.is_empty() {
        return Err(format!(
            "QML resource collection has no file entries: {}",
            qrc_path.display()
        ));
    }

    for (start, end, replacement) in replacements.into_iter().rev() {
        rewritten.replace_range(start..end, &replacement);
    }

    let normalized_qrc = staged_dir.join("resources.qrc");
    fs::write(&normalized_qrc, rewritten)
        .map_err(|error| format!("could not write {}: {error}", normalized_qrc.display()))?;
    Ok(normalized_qrc)
}

#[cfg(not(windows))]
fn set_modified_time(path: &Path, modified: SystemTime) -> std::io::Result<()> {
    File::open(path)?.set_modified(modified)
}

#[cfg(windows)]
fn set_modified_time(path: &Path, modified: SystemTime) -> std::io::Result<()> {
    use std::fs::OpenOptions;

    let mut permissions = fs::metadata(path)?.permissions();
    if permissions.readonly() {
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?
        .set_modified(modified)
}

fn decode_xml_text(value: &str) -> Result<String, String> {
    let mut decoded = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(entity_start) = rest.find('&') {
        decoded.push_str(&rest[..entity_start]);
        let after_start = &rest[entity_start + 1..];
        let entity_end = after_start
            .find(';')
            .ok_or_else(|| format!("unterminated XML entity in resource path {value:?}"))?;
        let entity = &after_start[..entity_end];
        decoded.push_str(match entity {
            "amp" => "&",
            "lt" => "<",
            "gt" => ">",
            "quot" => "\"",
            "apos" => "'",
            _ => {
                return Err(format!(
                    "unsupported XML entity &{entity}; in resource path"
                ));
            }
        });
        rest = &after_start[entity_end + 1..];
    }
    decoded.push_str(rest);
    Ok(decoded)
}

fn success_status() -> ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(0)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(0)
    }
}
