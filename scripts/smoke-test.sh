#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${HYBRIDROUTE_URL:-http://localhost:8080}"

wait_for_health() {
  for _ in $(seq 1 60); do
    if curl --fail --silent "${BASE_URL}/healthz" >/dev/null; then
      return 0
    fi
    sleep 2
  done
  echo "HybridRoute did not become healthy" >&2
  return 1
}

assert_service() {
  local expected="$1"
  local role="$2"
  local domain="$3"
  local payload="$4"

  local response
  response="$(curl --fail --silent "${BASE_URL}/route" \
    -H 'Content-Type: application/json' \
    -H "X-User-Roles: ${role}" \
    -H "X-Service-Domain: ${domain}" \
    -H 'X-Conversation-ID: smoke-test' \
    --data "$payload")"

  python3 - "$expected" "$response" <<'PY'
import json
import sys

expected = sys.argv[1]
response = json.loads(sys.argv[2])
actual = response.get("service")
if actual != expected:
    raise SystemExit(f"expected service {expected!r}, got {actual!r}: {response}")
print(f"✓ routed to {actual}")
PY
}

wait_for_health
assert_service \
  "streetlight-report" \
  "citizen" \
  "infrastructure" \
  '{"query":"The street lamp outside my home is broken"}'
assert_service \
  "parking-permit" \
  "citizen" \
  "mobility" \
  '{"query":"Renew my residential parking permit"}'
assert_service \
  "invoice-processing" \
  "finance-user" \
  "finance" \
  '{"query":"Extract the total from this supplier invoice","invoice_number":"INV-1001","amount":125.50}'
assert_service \
  "general-intake" \
  "citizen" \
  "general" \
  '{"query":"I have a question that does not match a known service"}'
