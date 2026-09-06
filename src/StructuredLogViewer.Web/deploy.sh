#!/usr/bin/env bash
# Publishes dist/ to Cloudflare Pages as a direct upload.
#
#   ./deploy.sh                 build everything, then deploy
#   ./deploy.sh --skip-build    deploy whatever is in dist/ already
#   ./deploy.sh --ui-only       skip the .NET publish, then deploy
#
# Direct upload rather than Pages' git integration: the build needs Rust 1.97.1,
# trunk and the .NET wasm-tools workload, none of which are in the Pages build
# image. So the build happens here and only dist/ goes up.
#
# CF_PAGES_PROJECT overrides the project name. `npx wrangler login` first.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT="${CF_PAGES_PROJECT:-structured-log-viewer-wip}"

BUILD_ARGS=(--no-serve)
SKIP_BUILD=0
for arg in "$@"; do
  case "$arg" in
    --skip-build) SKIP_BUILD=1 ;;
    *) BUILD_ARGS+=("$arg") ;;
  esac
done

if [ "$SKIP_BUILD" = 0 ]; then
  "$HERE/build.sh" "${BUILD_ARGS[@]}"
fi

# dotnet.native.wasm is the one to watch: Pages rejects any file over 25 MiB,
# and the Mono AOT build sits just under it.
BIG="$(find "$HERE/dist" -type f -exec ls -l {} + | awk '$5 > 24 * 1048576 { printf "%.1f MiB  %s\n", $5 / 1048576, $9 }')"
if [ -n "$BIG" ]; then
  echo "WARNING: files close to or over the 25 MiB Pages limit:" >&2
  echo "$BIG" >&2
fi

# --branch main so this lands as a production deploy on <project>.pages.dev,
# whatever branch the working tree happens to be on.
npx --yes wrangler@latest pages deploy "$HERE/dist" \
  --project-name "$PROJECT" \
  --branch main \
  --commit-dirty=true
