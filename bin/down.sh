#!/usr/bin/env bash
# Tear down the docker compose stack. Optional first arg: a compose profile.
set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE="${1:-}"
if [ -n "$PROFILE" ]; then
  docker compose --profile "$PROFILE" rm -f --all
else
  docker compose rm -f --all
fi
