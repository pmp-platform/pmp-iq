#!/usr/bin/env bash
# Tear down a docker compose topology. Optional first arg mirrors up.sh:
#   single | distributed | <profile> | (empty)
# Named volumes are kept so data persists across up/down cycles.
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET="${1:-}"

case "$TARGET" in
  single)
    docker compose -f docker-compose.single.yml rm -f --all ;;
  distributed)
    docker compose -f docker-compose.distributed.yml rm -f --all ;;
  "")
    docker compose rm -f --all ;;
  *)
    docker compose --profile "$TARGET" rm -f --all ;;
esac
