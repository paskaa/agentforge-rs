#!/usr/bin/env bash
set -euo pipefail

BUG_ID="${1:?Usage: $0 <bug_id> [extra_args...]}"
EXTRA_ARGS=("${@:2}")
REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
LOG_DIR="${LOG_DIR:-$REPO_DIR/logs/e2e}"
mkdir -p "$LOG_DIR"
TIMESTAMP="$(date +%Y%m%d-%H%M%S)"
LOG_FILE="$LOG_DIR/analyze-bug-${BUG_ID}-${TIMESTAMP}.log"

{
  echo "=== analyze-bug E2E ==="
  echo "bug_id: $BUG_ID"
  echo "repo: $REPO_DIR"
  echo "time: $TIMESTAMP"
  echo "args: ${EXTRA_ARGS[*]:-}"
  echo "--- run ---"
  cd "$REPO_DIR"
  set +e
  cargo run -q -- analyze-bug --bug-id "$BUG_ID" "${EXTRA_ARGS[@]}"
  STATUS=$?
  set -e
  echo "--- exit: $STATUS ---"
  echo "log: $LOG_FILE"
  exit $STATUS
} 2>&1 | tee "$LOG_FILE"
