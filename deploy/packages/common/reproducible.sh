#!/usr/bin/env bash
# reproducible.sh — shared release-artifact policy for Synveil builders.
#
# This file is sourced by the Linux and Windows package builders. It keeps
# source/build provenance, path remapping, artifact scans, and byte-mismatch
# diagnostics in one owner so local and CI validation cannot drift.
# SPDX-License-Identifier: MIT
set -euo pipefail

synveil_cargo_home() {
    if [[ -n "${CARGO_HOME:-}" ]]; then
        printf '%s' "$CARGO_HOME"
        return 0
    fi

    local account_home=""
    if command -v getent >/dev/null 2>&1; then
        account_home="$(getent passwd "$(id -u)" 2>/dev/null | cut -d: -f6 || true)"
    fi
    if [[ -n "$account_home" ]]; then
        printf '%s/.cargo' "$account_home"
    fi
}

synveil_append_unique_build_flag() {
    local variable_name="$1"
    local flag="$2"
    local current_value="${!variable_name:-}"

    if [[ "$current_value" != *"$flag"* ]]; then
        if [[ -n "$current_value" ]]; then
            current_value+=" "
        fi
        current_value+="$flag"
        export "${variable_name}=${current_value}"
    fi
}

# CXX-Qt's QML-module build asks Qt's rcc tool to embed a generated qmldir and
# the QML source files. rcc records their filesystem mtimes in generated C++.
# Use a build-host wrapper that copies those QML resources under OUT_DIR and
# fixes only the copies to SOURCE_DATE_EPOCH before invoking the real rcc.
# This keeps source files untouched and makes clean release rebuilds stable.
synveil_prepare_reproducible_qt_tools() {
    local repo_root="$1"
    local cargo_target_dir="$2"
    local host_toolchain="$3"
    local real_qmake="${QMAKE:-}"
    local real_rcc=""
    local query
    local query_dir
    local candidate
    local source_date_epoch="${SOURCE_DATE_EPOCH:-}"
    local qmake_identity
    local rcc_identity
    local wrapper_source_identity
    local wrapper_key
    local wrapper_dir
    local wrapper_suffix=""
    local wrapper_binary
    local qmake_for_wrapper
    local rcc_for_wrapper
    local wrapper_dir_for_wrapper
    local qmake_export_path

    if [[ -z "$real_qmake" ]]; then
        real_qmake="$(command -v qmake6 || command -v qmake || true)"
    elif [[ ! -x "$real_qmake" ]]; then
        real_qmake="$(command -v "$real_qmake" || true)"
    fi
    if command -v cygpath >/dev/null 2>&1 && [[ "$real_qmake" =~ ^[A-Za-z]:[/\\] ]]; then
        real_qmake="$(cygpath -u "$real_qmake")"
    fi
    # Non-Qt package builds can share the policy helper without requiring Qt.
    [[ -n "$real_qmake" ]] || return 0
    [[ -x "$real_qmake" ]] || return 0

    if [[ -z "$source_date_epoch" ]]; then
        source_date_epoch="$(git -C "$repo_root" log -1 --format=%ct 2>/dev/null || true)"
    fi
    source_date_epoch="${source_date_epoch:-0}"
    if [[ ! "$source_date_epoch" =~ ^[0-9]+$ ]]; then
        printf '[synveil-artifact] ERROR: SOURCE_DATE_EPOCH must be a non-negative integer for Qt resource generation, got %q\n' "$source_date_epoch" >&2
        return 1
    fi

    if [[ "$host_toolchain" == *windows* ]]; then
        wrapper_suffix=".exe"
    fi

    for query in \
        "QT_HOST_LIBEXECS/get" \
        "QT_HOST_LIBEXECS" \
        "QT_HOST_BINS/get" \
        "QT_HOST_BINS" \
        "QT_INSTALL_LIBEXECS/get" \
        "QT_INSTALL_LIBEXECS" \
        "QT_INSTALL_BINS/get" \
        "QT_INSTALL_BINS"; do
        query_dir="$("$real_qmake" -query "$query" 2>/dev/null | head -n 1 || true)"
        [[ -n "$query_dir" ]] || continue
        if command -v cygpath >/dev/null 2>&1; then
            query_dir="$(cygpath -u "$query_dir" 2>/dev/null || printf '%s' "$query_dir")"
        fi
        candidate="${query_dir%/}/rcc${wrapper_suffix}"
        if [[ -x "$candidate" ]]; then
            real_rcc="$candidate"
            break
        fi
    done
    # Let the actual desktop build report its normal missing-Qt diagnostic.
    [[ -n "$real_rcc" ]] || return 0

    qmake_identity="$(cksum < "$real_qmake" | awk '{print $1 ":" $2}')"
    rcc_identity="$(cksum < "$real_rcc" | awk '{print $1 ":" $2}')"
    wrapper_source_identity="$(cksum < "${repo_root}/scripts/reproducible-qt-wrapper.rs" | awk '{print $1 ":" $2}')"
    wrapper_key="$(printf '%s\n%s\n%s\n%s\n%s\n%s\n' \
        "$real_qmake" "$qmake_identity" "$real_rcc" "$rcc_identity" \
        "$wrapper_source_identity" "$source_date_epoch" |
        cksum | awk '{print $1}')"
    wrapper_dir="${cargo_target_dir}/synveil-reproducible-qt-tools/${wrapper_key}"
    mkdir -p "$wrapper_dir"
    wrapper_binary="${wrapper_dir}/qt-tool-wrapper${wrapper_suffix}"
    qmake_for_wrapper="$real_qmake"
    rcc_for_wrapper="$real_rcc"
    wrapper_dir_for_wrapper="$wrapper_dir"
    qmake_export_path="${wrapper_dir}/qmake${wrapper_suffix}"
    if command -v cygpath >/dev/null 2>&1; then
        qmake_for_wrapper="$(cygpath -m "$qmake_for_wrapper" 2>/dev/null || printf '%s' "$qmake_for_wrapper")"
        rcc_for_wrapper="$(cygpath -m "$rcc_for_wrapper" 2>/dev/null || printf '%s' "$rcc_for_wrapper")"
        wrapper_dir_for_wrapper="$(cygpath -m "$wrapper_dir_for_wrapper" 2>/dev/null || printf '%s' "$wrapper_dir_for_wrapper")"
        qmake_export_path="${wrapper_dir_for_wrapper}/qmake${wrapper_suffix}"
    fi
    SYNVEIL_REAL_QMAKE="$qmake_for_wrapper" \
        SYNVEIL_REAL_RCC="$rcc_for_wrapper" \
        SYNVEIL_QT_WRAPPER_DIR="$wrapper_dir_for_wrapper" \
        SYNVEIL_SOURCE_DATE_EPOCH="$source_date_epoch" \
        rustc --edition=2021 "${repo_root}/scripts/reproducible-qt-wrapper.rs" -o "$wrapper_binary"
    cp "$wrapper_binary" "${wrapper_dir}/qmake${wrapper_suffix}"
    cp "$wrapper_binary" "${wrapper_dir}/rcc${wrapper_suffix}"
    chmod +x "${wrapper_dir}/qmake${wrapper_suffix}" "${wrapper_dir}/rcc${wrapper_suffix}"
    export QMAKE="$qmake_export_path"
}

# Export the build inputs that affect release-byte reproducibility. The
# remapping target is deliberately stable and contains neither a username nor
# a checkout path. Existing caller flags are retained; the only added flags
# are path remaps and incremental compilation is disabled for release output.
synveil_prepare_reproducible_rust_build() {
    local repo_root="$1"
    local canonical_repo_root
    local cargo_target_dir
    local cargo_home
    local existing_flags="${RUSTFLAGS:-}"
    local flag
    local remap_flags=()
    local native_repo_root=""
    local native_target_dir=""

    canonical_repo_root="$(cd "$repo_root" && pwd -P)"
    cargo_target_dir="${CARGO_TARGET_DIR:-${canonical_repo_root}/target}"
    if [[ "$cargo_target_dir" =~ ^[A-Za-z]:[/\\] ]]; then
        if command -v cygpath >/dev/null 2>&1; then
            cargo_target_dir="$(cygpath -u "$cargo_target_dir")"
        fi
    fi
    if [[ "$cargo_target_dir" != /* ]]; then
        cargo_target_dir="${canonical_repo_root}/${cargo_target_dir}"
    fi
    cargo_target_dir="$(realpath -m -- "$cargo_target_dir")"

    # Rustc applies the last matching remap prefix. Add broad checkout/home
    # prefixes first, then nested target prefixes so clean builds under
    # different CARGO_TARGET_DIR roots resolve to the same stable target path.
    remap_flags+=("--remap-path-prefix=${canonical_repo_root}=/usr/src/synveil")
    if command -v cygpath >/dev/null 2>&1; then
        native_repo_root="$(cygpath -m "$canonical_repo_root" 2>/dev/null)"
        native_target_dir="$(cygpath -m "$cargo_target_dir" 2>/dev/null)"
        if [[ -n "$native_repo_root" && "$native_repo_root" != "$canonical_repo_root" ]]; then
            remap_flags+=("--remap-path-prefix=${native_repo_root}=/usr/src/synveil")
        fi
    fi

    cargo_home="$(synveil_cargo_home || true)"
    if [[ -n "$cargo_home" && -d "$cargo_home" ]]; then
        remap_flags+=("--remap-path-prefix=${cargo_home}=/usr/local/cargo")
    fi

    if [[ "${canonical_repo_root}/target" != "$cargo_target_dir" ]]; then
        remap_flags+=("--remap-path-prefix=${canonical_repo_root}/target=/usr/src/synveil-target")
    fi
    if [[ -n "$native_repo_root" && "$native_repo_root" != "$canonical_repo_root" && "${native_repo_root}/target" != "$native_target_dir" ]]; then
        remap_flags+=("--remap-path-prefix=${native_repo_root}/target=/usr/src/synveil-target")
    fi
    if [[ -n "$native_target_dir" && "$native_target_dir" != "$cargo_target_dir" ]]; then
        remap_flags+=("--remap-path-prefix=${native_target_dir}=/usr/src/synveil-target")
    fi
    remap_flags+=("--remap-path-prefix=${cargo_target_dir}=/usr/src/synveil-target")

    for flag in "${remap_flags[@]}"; do
        if [[ "$existing_flags" != *"$flag"* ]]; then
            if [[ -n "$existing_flags" ]]; then
                existing_flags+=" "
            fi
            existing_flags+="$flag"
        fi
    done

    export RUSTFLAGS="$existing_flags"
    export CARGO_INCREMENTAL=0
    # Qt's QML AOT compiler uses QHash-backed data structures. Its default
    # per-process hash seed can change generated C++ and release bytes across
    # clean builds, so pin it for build tools only (the app does not inherit it
    # at runtime).
    export QT_HASH_SEED=0

    # CXX-Qt compiles generated/native C++ into the desktop binary. Rust path
    # remaps do not affect C/C++ __FILE__ strings, so pass the equivalent
    # GCC/Clang prefix maps on non-MSVC toolchains. Native MSVC builds do not
    # understand these flags and must not receive them.
    local host_toolchain=""
    host_toolchain="$(rustc -vV 2>/dev/null | sed -n 's/^host: //p' | head -n 1 || true)"
    if [[ "$host_toolchain" != *"msvc"* ]]; then
        # GCC/Clang use the last matching prefix map for an overlapping path.
        # Keep repository roots first and nested target directories afterward
        # so <repo>/target never becomes /usr/src/synveil/target in __FILE__.
        local native_prefixes=("$canonical_repo_root")
        if [[ -n "$native_repo_root" && "$native_repo_root" != "$canonical_repo_root" ]]; then
            native_prefixes+=("$native_repo_root")
        fi
        if [[ "${canonical_repo_root}/target" != "$cargo_target_dir" ]]; then
            native_prefixes+=("${canonical_repo_root}/target")
        fi
        if [[ -n "$native_target_dir" && "$native_target_dir" != "$cargo_target_dir" ]]; then
            native_prefixes+=("$native_target_dir")
        fi
        if [[ -n "$native_repo_root" && "$native_repo_root" != "$canonical_repo_root" ]]; then
            native_prefixes+=("${native_repo_root}/target")
        fi
        native_prefixes+=("$cargo_target_dir")
        for flag_prefix in -ffile-prefix-map -fmacro-prefix-map -fdebug-prefix-map; do
            for prefix in "${native_prefixes[@]}"; do
                flag="${flag_prefix}=${prefix}="
                if [[ "$prefix" == "$canonical_repo_root" || "$prefix" == "$native_repo_root" ]]; then
                    flag+="/usr/src/synveil"
                else
                    flag+="/usr/src/synveil-target"
                fi
                synveil_append_unique_build_flag CFLAGS "$flag"
                synveil_append_unique_build_flag CXXFLAGS "$flag"
            done
            flag="${flag_prefix}=/usr/src/synveil/target=/usr/src/synveil-target"
            synveil_append_unique_build_flag CFLAGS "$flag"
            synveil_append_unique_build_flag CXXFLAGS "$flag"
        done
    fi
    # Avoid silently dropping an encoded caller flag. RUSTFLAGS is the
    # supported input for the builders; an encoded-only invocation is rejected
    # instead of producing an artifact with an unknown compiler configuration.
    if [[ -n "${CARGO_ENCODED_RUSTFLAGS:-}" ]]; then
        printf '[synveil-artifact] ERROR: CARGO_ENCODED_RUSTFLAGS is not supported by the reproducible release builder; use RUSTFLAGS instead\n' >&2
        return 1
    fi
    synveil_prepare_reproducible_qt_tools "$canonical_repo_root" "$cargo_target_dir" "$host_toolchain"
}

synveil_artifact_sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        printf '[synveil-artifact] ERROR: SHA-256 tool is required: %s\n' "$1" >&2
        return 1
    fi
}

synveil_artifact_size() {
    wc -c < "$1" | tr -d '[:space:]'
}

synveil_artifact_build_id() {
    if ! command -v readelf >/dev/null 2>&1; then
        printf '%s' '-'
        return 0
    fi
    readelf -n "$1" 2>/dev/null |
        sed -n 's/^[[:space:]]*Build ID: //p' |
        head -n 1
}

synveil_assert_no_private_paths() {
    local artifact="$1"
    local label="${2:-$artifact}"
    local pattern='(/home/|/Users/|/mnt/|\.cargo/registry|[A-Za-z]:[\\/][Uu]sers[\\/]|/tmp/synveil-[^[:space:]]*|/var/tmp/synveil-[^[:space:]]*|/usr/src/synveil/target/|target[\\/]x86_64-pc-windows-gnu[\\/]release[\\/]build)'
    if LC_ALL=C grep -aEq "$pattern" "$artifact"; then
        printf '[synveil-artifact] ERROR: private or temporary build path found in %s\n' "$label" >&2
        LC_ALL=C grep -aoE "$pattern" "$artifact" | sort -u | head -n 8 |
            sed 's/^/[synveil-artifact]   matched path marker: /' >&2 || true
        return 1
    fi
}

synveil_assert_no_secret_markers() {
    local artifact="$1"
    local label="${2:-$artifact}"
    local pattern='(svd1_[A-Za-z0-9_-]+|sve1_[A-Za-z0-9_-]+|SYNVEIL_DATABASE_URL=postgresql://[^[:space:]]*:[^[:space:]]*@|DATABASE_URL=postgresql://[^[:space:]]*:[^[:space:]]*@|postgresql://[^[:space:]]+:[^[:space:]@]+@[^[:space:]]+)'
    if LC_ALL=C grep -aEiq "$pattern" "$artifact"; then
        printf '[synveil-artifact] ERROR: secret-like marker found in %s\n' "$label" >&2
        return 1
    fi
}

synveil_assert_linux_release_binary() {
    local artifact="$1"
    local label="${2:-$artifact}"
    [[ -f "$artifact" ]] || {
        printf '[synveil-artifact] ERROR: release artifact does not exist: %s\n' "$artifact" >&2
        return 1
    }
    [[ -x "$artifact" ]] || {
        printf '[synveil-artifact] ERROR: release artifact is not executable: %s\n' "$artifact" >&2
        return 1
    }
    if command -v file >/dev/null 2>&1; then
        file -b "$artifact" | grep -q 'ELF' || {
            printf '[synveil-artifact] ERROR: expected an ELF release binary: %s\n' "$label" >&2
            return 1
        }
    fi
    if ! command -v readelf >/dev/null 2>&1; then
        printf '[synveil-artifact] ERROR: readelf is required for ELF release validation: %s\n' "$label" >&2
        return 1
    fi
    readelf -h "$artifact" >/dev/null || {
        printf '[synveil-artifact] ERROR: ELF header audit failed: %s\n' "$label" >&2
        return 1
    }
    local build_id
    build_id="$(synveil_artifact_build_id "$artifact")"
    [[ "$build_id" =~ ^[0-9a-fA-F]+$ ]] || {
        printf '[synveil-artifact] ERROR: ELF build ID is missing: %s\n' "$label" >&2
        return 1
    }
    if readelf -S "$artifact" 2>/dev/null | grep -Eq '[[:space:]]\.debug_[^[:space:]]*'; then
        printf '[synveil-artifact] ERROR: debug section leaked into release binary: %s\n' "$label" >&2
        return 1
    fi
    synveil_assert_no_private_paths "$artifact" "$label"
    synveil_assert_no_secret_markers "$artifact" "$label"
}

synveil_assert_portable_release_binary() {
    local artifact="$1"
    local label="${2:-$artifact}"
    [[ -f "$artifact" ]] || {
        printf '[synveil-artifact] ERROR: release artifact does not exist: %s\n' "$artifact" >&2
        return 1
    }
    synveil_assert_no_private_paths "$artifact" "$label"
    synveil_assert_no_secret_markers "$artifact" "$label"
}

synveil_source_revision() {
    local repo_root="$1"
    git -C "$repo_root" rev-parse HEAD 2>/dev/null || {
        printf '[synveil-artifact] ERROR: Git revision is unavailable; cannot record release provenance\n' >&2
        return 1
    }
}

# Hash the actual source files, including dirty tracked files and non-ignored
# source files. Ignored build output (target/) is excluded by Git's file list.
# This is the provenance key for a generated manifest; it is intentionally not
# a hash of HEAD alone, because a dirty worktree must not be compared with a
# clean-checkpoint artifact.
synveil_source_fingerprint() {
    local repo_root="$1"
    local path digest
    if ! command -v git >/dev/null 2>&1; then
        printf '[synveil-artifact] ERROR: Git is required to fingerprint release source\n' >&2
        return 1
    fi
    {
        git -C "$repo_root" ls-files -z --cached --others --exclude-standard -- . ':!target' |
            LC_ALL=C sort -z |
            while IFS= read -r -d '' path; do
                [[ -f "$repo_root/$path" ]] || continue
                digest="$(synveil_artifact_sha256 "$repo_root/$path")"
                printf '%s  %s\n' "$digest" "$path"
            done
    } | sha256sum | awk '{print $1}'
}

synveil_toolchain_value() {
    local command_name="$1"
    if command -v "$command_name" >/dev/null 2>&1; then
        "$command_name" --version 2>/dev/null | head -n 1 | tr ' ' '_'
    else
        printf '%s' 'unavailable'
    fi
}

synveil_write_linux_artifact_manifest() {
    local manifest_path="$1"
    local repo_root="$2"
    local source_date_epoch="$3"
    shift 3

    local source_revision source_fingerprint pair name artifact
    source_revision="$(synveil_source_revision "$repo_root")"
    source_fingerprint="$(synveil_source_fingerprint "$repo_root")"
    mkdir -p "$(dirname "$manifest_path")"
    {
        printf '%s\n' 'Synveil Linux release artifact manifest'
        printf '%s\n' 'format=1'
        printf 'source_revision=%s\n' "$source_revision"
        printf 'source_fingerprint=%s\n' "$source_fingerprint"
        printf 'source_date_epoch=%s\n' "$source_date_epoch"
        printf 'rustc=%s\n' "$(synveil_toolchain_value rustc)"
        printf 'cargo=%s\n' "$(synveil_toolchain_value cargo)"
        printf '%s\n' 'artifacts=sha256 size build_id path'
        for pair in "$@"; do
            name="${pair%%:*}"
            artifact="${pair#*:}"
            synveil_assert_repo_artifact_path "$repo_root" "$artifact" "$name"
            printf '%s %s %s %s\n' \
                "$(synveil_artifact_sha256 "$artifact")" \
                "$(synveil_artifact_size "$artifact")" \
                "$(synveil_artifact_build_id "$artifact")" \
                "$name"
        done
    } > "$manifest_path"
}

synveil_assert_repo_artifact_path() {
    local repo_root="$1"
    local artifact="$2"
    local relative_path="$3"
    local canonical_root expected actual
    canonical_root="$(cd "$repo_root" && pwd -P)"
    expected="$(realpath -e -- "${canonical_root}/${relative_path}")" || {
        printf '[synveil-artifact] ERROR: expected release artifact path is missing: %s\n' "$relative_path" >&2
        return 1
    }
    actual="$(realpath -e -- "$artifact")" || {
        printf '[synveil-artifact] ERROR: release artifact path is missing: %s\n' "$artifact" >&2
        return 1
    }
    if [[ "$actual" != "$expected" ]]; then
        printf '[synveil-artifact] ERROR: release manifest path does not identify the supplied binary\n' >&2
        printf '[synveil-artifact]   manifest path: %s\n' "$relative_path" >&2
        printf '[synveil-artifact]   expected file: %s\n' "$expected" >&2
        printf '[synveil-artifact]   supplied file: %s\n' "$actual" >&2
        return 1
    fi
}

synveil_report_artifact_mismatch() {
    local expected="$1"
    local actual="$2"
    local label="${3:-artifact}"
    local expected_sha actual_sha expected_size actual_size first_difference
    expected_sha="$(synveil_artifact_sha256 "$expected")"
    actual_sha="$(synveil_artifact_sha256 "$actual")"
    expected_size="$(synveil_artifact_size "$expected")"
    actual_size="$(synveil_artifact_size "$actual")"
    printf '[synveil-artifact] ERROR: %s differs\n' "$label" >&2
    printf '[synveil-artifact]   expected: path=%s size=%s sha256=%s build_id=%s\n' \
        "$expected" "$expected_size" "$expected_sha" "$(synveil_artifact_build_id "$expected")" >&2
    printf '[synveil-artifact]   actual:   path=%s size=%s sha256=%s build_id=%s\n' \
        "$actual" "$actual_size" "$actual_sha" "$(synveil_artifact_build_id "$actual")" >&2
    first_difference="$(cmp -l "$expected" "$actual" 2>/dev/null | head -n 1 | awk '{print $1}' || true)"
    if [[ -n "$first_difference" ]]; then
        printf '[synveil-artifact]   first differing byte: %s\n' "$first_difference" >&2
    else
        printf '[synveil-artifact]   first differing byte: outside the shared length\n' >&2
    fi
}

synveil_require_identical_artifacts() {
    local expected="$1"
    local actual="$2"
    local label="${3:-artifacts}"
    if cmp -s "$expected" "$actual"; then
        printf '[synveil-artifact] %s byte-identical: sha256=%s size=%s\n' \
            "$label" "$(synveil_artifact_sha256 "$expected")" "$(synveil_artifact_size "$expected")" >&2
        return 0
    fi
    synveil_report_artifact_mismatch "$expected" "$actual" "$label"
    return 1
}
