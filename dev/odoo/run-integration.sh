#!/usr/bin/env bash
# Starts Odoo 15/17/19 in Docker, creates the test databases and runs the ignored
# appex-odoo integration tests against them.
#
#   dev/odoo/run-integration.sh            # run and tear everything down
#   KEEP=1 dev/odoo/run-integration.sh     # keep containers running afterwards
#   IT_COMMAND='cargo test ...' dev/odoo/run-integration.sh  # run another suite instead
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
compose=(docker compose -f "$here/compose.yml")
master_password="it-master-password"

cleanup() {
  if [[ "${KEEP:-0}" != "1" ]]; then
    "${compose[@]}" down -v --remove-orphans >/dev/null
  fi
}
trap cleanup EXIT

echo "==> Starting PostgreSQL"
"${compose[@]}" up -d --wait db15 db17 db19

echo "==> Creating databases (base module, no demo data)"
"${compose[@]}" run --rm -T odoo15 odoo -d it15 -i base --without-demo=all --stop-after-init >/dev/null 2>&1 &
p15=$!
"${compose[@]}" run --rm -T odoo17 odoo -d it17 -i base --without-demo=all --stop-after-init >/dev/null 2>&1 &
p17=$!
"${compose[@]}" run --rm -T odoo19 odoo -d it19 -i base --stop-after-init >/dev/null 2>&1 &
p19=$!
wait "$p15" "$p17" "$p19"

echo "==> Generating an API key for admin on Odoo 19"
api_key="$("${compose[@]}" run --rm -T odoo19 odoo shell -d it19 --log-level=error 2>/dev/null <<'PY' | sed -n 's/^APPEX_API_KEY=//p' | tail -n1
user = env.ref("base.user_admin")
key = env["res.users.apikeys"].with_user(user)._generate(None, "appex-integration", None)
env.cr.commit()
print("APPEX_API_KEY=" + key)
PY
)"
if [[ -z "$api_key" ]]; then
  echo "could not generate the Odoo 19 API key" >&2
  exit 1
fi

echo "==> Starting Odoo servers"
"${compose[@]}" up -d --wait odoo15 odoo15-nolist odoo17 odoo19 odoo19-nolist

export APPEX_IT_MASTER_PASSWORD="$master_password"
export APPEX_IT_ADMIN_LOGIN="admin"
export APPEX_IT_ADMIN_PASSWORD="admin"
export APPEX_IT_ODOO15_URL="http://it15.localhost:18015"
export APPEX_IT_ODOO17_URL="http://it17.localhost:18017"
export APPEX_IT_ODOO19_URL="http://it19.localhost:18019"
export APPEX_IT_ODOO15_NOLIST_URL="http://it15.localhost:18115"
export APPEX_IT_ODOO19_NOLIST_URL="http://it19.localhost:18119"
export APPEX_IT_ODOO19_API_KEY="$api_key"

# Exported for other suites (e.g. the app IPC tests) when the stack is kept.
mkdir -p "$here/.data"
env | grep '^APPEX_IT_' > "$here/.data/it.env"
chmod 600 "$here/.data/it.env"

if [[ -n "${IT_COMMAND:-}" ]]; then
  echo "==> Running: $IT_COMMAND"
  cd "$repo"
  bash -c "$IT_COMMAND"
  exit 0
fi

echo "==> Running integration tests"
cd "$repo"
cargo test -p appex-odoo --test integration_odoo -- --ignored --nocapture --test-threads=1 "$@"
