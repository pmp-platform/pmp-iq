#!/usr/bin/env bash
# Bring up a docker compose topology. Optional first arg:
#   single       -> one app container on SQLite, no dependencies
#   distributed  -> two app instances + nginx + Postgres + Redis
#   <profile>    -> the default compose file with that profile (e.g. migrate)
#   (empty)      -> the default compose file (Postgres + dbmate)
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET="${1:-}"
bash bin/down.sh "$TARGET"

case "$TARGET" in
  single)
    docker compose -f docker-compose.single.yml up --build ;;
  distributed)
    docker compose -f docker-compose.distributed.yml up --build ;;
  "")
    docker compose up ;;
  *)
    docker compose --profile "$TARGET" up ;;
esac
