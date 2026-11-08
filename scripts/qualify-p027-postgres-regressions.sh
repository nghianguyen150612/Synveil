#!/usr/bin/env bash
# Fresh databases in an explicitly disposable PostgreSQL instance only.
set -euo pipefail
: "${SYNVEIL_TEST_DATABASE_URL:?disposable PostgreSQL administrator URL required}"
[[ "${SYNVEIL_P027_DISPOSABLE:-}" == 1 ]] || { echo 'Disposable opt-in required' >&2; exit 1; }
admin_url=$SYNVEIL_TEST_DATABASE_URL
evidence_dir=${SYNVEIL_P027_EVIDENCE_DIR:-target/p027-evidence}
mkdir -p "$evidence_dir"
while read -r package target test_name; do
  database="p027_regression_$(python3 -c 'import uuid; print(uuid.uuid4().hex)')"
  psql "$admin_url" -v ON_ERROR_STOP=1 -c "CREATE DATABASE $database"
  SYNVEIL_TEST_DATABASE_URL=$(python3 - "$admin_url" "$database" <<'PY'
import sys
from urllib.parse import urlsplit, urlunsplit
url = urlsplit(sys.argv[1])
print(urlunsplit(url._replace(path='/' + sys.argv[2])))
PY
  ) cargo test -p "$package" --test "$target" --locked "$test_name" -- --exact --ignored --test-threads=1 --nocapture 2>&1 | tee "$evidence_dir/p027-regression-$test_name.txt"
  rg 'test result: ok\. 1 passed; 0 failed; 0 ignored;' "$evidence_dir/p027-regression-$test_name.txt"
done <<'CASES'
synveil-auth postgres postgres_authentication_bootstrap_is_atomic_and_race_safe
synveil-auth postgres postgres_browser_sessions_are_opaque_expiring_and_revocable
synveil-auth bootstrap_precision_postgres postgres_first_admin_accepts_nanosecond_server_timestamp
synveil-auth device_credentials_postgres postgres_device_credentials_enroll_once_and_revoke_immediately
synveil-auth device_credentials_postgres postgres_device_enrollment_concurrent_exchange_has_one_winner
synveil-auth device_credentials_postgres postgres_enrollment_expiry_is_rechecked_after_waiting_for_device_lock
synveil-api authorization_postgres p005_owner_and_device_authorization_probe_reveals_mutation_binding_gap
synveil-api p015_postgres_objectstore_authorization p015_postgres_objectstore_owner_to_byte
synveil-metadata postgres postgres_version_history_is_owner_scoped_keyset_paged_and_pointer_based
CASES
