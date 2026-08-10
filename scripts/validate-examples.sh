#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cache_dir=$(mktemp -d "${TMPDIR:-/tmp}/zellij-workspaces-examples.XXXXXX")
trap 'rm -rf "$cache_dir"' EXIT HUP INT TERM

for template in development services; do
  layout=$(
    ZELLIJ_WORKSPACES_TEMPLATES_DIR="$repo_root/examples" \
      ZELLIJ_WORKSPACES_CACHE_DIR="$cache_dir" \
      cargo run --quiet --package zellij-workspaces -- \
      --render "$template" example "$repo_root"
  )
  if command -v zellij >/dev/null 2>&1; then
    zellij --layout "$layout" setup --check >/dev/null
  fi
done

printf '%s\n' "workspace examples: ok"
