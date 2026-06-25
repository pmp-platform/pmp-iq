#!/usr/bin/env bash
# Bring up the docker compose stack. Optional first arg: a compose profile.
set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE="${1:-}"
bash bin/down.sh "$PROFILE"

if [ -n "$PROFILE" ]; then
  docker compose --profile "$PROFILE" up
else
  docker compose up
fi
