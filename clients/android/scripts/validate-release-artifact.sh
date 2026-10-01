#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat >&2 <<'USAGE'
Usage: validate-release-artifact.sh --apk=PATH [--aab=PATH] [--version-name=NAME] [--version-code=CODE]
USAGE
}

apk=''
aab=''
expected_version_name='0.1.0'
expected_version_code='1'
while [[ $# -gt 0 ]]; do
    case "$1" in
        --apk=*) apk="${1#--apk=}"; shift ;;
        --aab=*) aab="${1#--aab=}"; shift ;;
        --version-name=*) expected_version_name="${1#--version-name=}"; shift ;;
        --version-code=*) expected_version_code="${1#--version-code=}"; shift ;;
        --help|-h) usage; exit 0 ;;
        *) usage; exit 2 ;;
    esac
done

[[ -n "$apk" && -f "$apk" ]] || {
    printf 'release artifact is missing: %s\n' "$apk" >&2
    exit 1
}

sdk_root="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Android/Sdk}}"
apkanalyzer="${APKANALYZER:-$sdk_root/cmdline-tools/latest/bin/apkanalyzer}"
if ! command -v "$apkanalyzer" >/dev/null 2>&1 && [[ ! -x "$apkanalyzer" ]]; then
    printf 'apkanalyzer is required; set APKANALYZER or install Android SDK command-line tools\n' >&2
    exit 1
fi

manifest_value() {
    "$apkanalyzer" manifest "$1" "$apk"
}

package_id="$(manifest_value application-id)"
version_name="$(manifest_value version-name)"
version_code="$(manifest_value version-code)"
target_sdk="$(manifest_value target-sdk)"
debuggable="$(manifest_value debuggable)"
permissions="$(manifest_value permissions | sort)"
expected_permissions=$'android.permission.ACCESS_NETWORK_STATE\nandroid.permission.FOREGROUND_SERVICE\nandroid.permission.INTERNET\nandroid.permission.RECEIVE_BOOT_COMPLETED\nandroid.permission.WAKE_LOCK\ncom.synveil.android.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION'

[[ "$package_id" == 'com.synveil.android' ]] || { printf 'unexpected package: %s\n' "$package_id" >&2; exit 1; }
[[ "$version_name" == "$expected_version_name" ]] || { printf 'unexpected version name: %s\n' "$version_name" >&2; exit 1; }
[[ "$version_code" == "$expected_version_code" ]] || { printf 'unexpected version code: %s\n' "$version_code" >&2; exit 1; }
[[ "$target_sdk" == '36' ]] || { printf 'unexpected target SDK: %s\n' "$target_sdk" >&2; exit 1; }
[[ "$debuggable" == 'false' ]] || { printf 'release artifact is debuggable: %s\n' "$debuggable" >&2; exit 1; }
[[ "$permissions" == "$expected_permissions" ]] || {
    printf 'unexpected release permissions:\n%s\n' "$permissions" >&2
    exit 1
}

entries="$(unzip -Z1 "$apk")"
if grep -Eiq '(^|/)(google-services\.json|local\.properties|.*\.(pem|p12|jks|keystore))$' <<< "$entries"; then
    printf 'release artifact contains a credential or signing file\n' >&2
    exit 1
fi
if unzip -p "$apk" classes.dex 2>/dev/null | strings | grep -Eq 'svd1_[A-Za-z0-9_]{16,}|sve1_[A-Za-z0-9_]{16,}|BEGIN (RSA|EC|OPENSSH|PRIVATE) KEY'; then
    printf 'release artifact contains plaintext credential material\n' >&2
    exit 1
fi

if [[ -n "$aab" ]]; then
    [[ -f "$aab" ]] || { printf 'app bundle is missing: %s\n' "$aab" >&2; exit 1; }
    bundle_entries="$(unzip -Z1 "$aab")"
    for required_entry in BundleConfig.pb base/manifest/AndroidManifest.xml base/dex/classes.dex base/resources.pb; do
        grep -Fxq "$required_entry" <<< "$bundle_entries" || {
            printf 'app bundle is missing required entry: %s\n' "$required_entry" >&2
            exit 1
        }
    done
    if grep -Eiq '(^|/)(google-services\.json|local\.properties|.*\.(pem|p12|jks|keystore))$' <<< "$bundle_entries"; then
        printf 'app bundle contains a credential or signing file\n' >&2
        exit 1
    fi
    if unzip -p "$aab" base/dex/classes.dex 2>/dev/null | strings | grep -Eq 'svd1_[A-Za-z0-9_]{16,}|sve1_[A-Za-z0-9_]{16,}|BEGIN (RSA|EC|OPENSSH|PRIVATE) KEY'; then
        printf 'app bundle contains plaintext credential material\n' >&2
        exit 1
    fi
    printf 'app bundle shape valid: %s\n' "$aab"
fi

printf 'release artifact valid: package=%s version=%s (%s) targetSdk=%s debuggable=%s\n' \
    "$package_id" "$version_name" "$version_code" "$target_sdk" "$debuggable"
printf 'permissions=%s\n' "${permissions//$'\n'/,}"
