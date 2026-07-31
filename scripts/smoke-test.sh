#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${HYBRIDROUTE_URL:-http://localhost:8088}"

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
  local query="$4"

  local response
  response="$(curl --fail --silent "${BASE_URL}/route" \
    -H 'Content-Type: application/json' \
    -H "X-User-Roles: ${role}" \
    -H "X-Route-Domain: ${domain}" \
    -H 'X-Conversation-ID: smoke-test' \
    -d "{\"query\":\"${query}\"}")"

  python3 - "$expected" "$response" <<'PY'
import json
import sys
expected = sys.argv[1]
response = json.loads(sys.argv[2])
actual = response.get("service")
if actual != expected:
    raise SystemExit(f"expected service {expected!r}, got {actual!r}: {response}")
print(f"✓ {expected}: {response.get('semantic_route')} ({response.get('semantic_score')})")
PY
}

wait_for_health
assert_service "streetlight-api" "citizen" "city-services" "The street lamp outside my home is broken"
assert_service "parking-api" "citizen" "mobility" "Renew my residential parking permit"
assert_service "invoice-api" "finance-user" "finance" "Extract the total from this supplier invoice"
assert_service "general-api" "citizen" "city-services" "I have a question that does not match a known service"
