#!/usr/bin/env bash
#
# Guards the size and shape of this fork's delta against upstream
# (KirillOsenkov/MSBuildStructuredLog). Two rules, both from
# docs/fork/MAINTENANCE.md:
#
#   1. Every upstream file we modify or delete is listed in
#      docs/fork/upstream-delta.allow, with a reason.
#   2. Every file we add lives under a fork-owned root.
#
# Usage: scripts/check-upstream-delta.sh [upstream-ref]
#
# The ref defaults to $UPSTREAM_REF, else the first of upstream/main,
# origin/main that exists. CI fetches upstream explicitly.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

allow_file="docs/fork/upstream-delta.allow"

# Directories this fork owns outright. Upstream has no files here, so nothing
# we add inside them can ever conflict. Keep in sync with MAINTENANCE.md.
fork_owned=(
  "src/StructuredLogViewer.Gpui/"
  "src/StructuredLogViewer.NativeBridge/"
  "src/StructuredLogViewer.Semantics/"
  "src/StructuredLogViewer.Web/"
  "docs/fork/"
  "scripts/"
  ".github/"
)

upstream_ref="${1:-${UPSTREAM_REF:-}}"
if [ -z "$upstream_ref" ]; then
  for candidate in upstream/main origin/main; do
    if git rev-parse --verify --quiet "$candidate" >/dev/null; then
      upstream_ref="$candidate"
      break
    fi
  done
fi

if [ -z "$upstream_ref" ]; then
  echo "error: no upstream ref found. Pass one, or set UPSTREAM_REF." >&2
  exit 2
fi

base="$(git merge-base HEAD "$upstream_ref")"
echo "Comparing HEAD against $upstream_ref (merge base ${base:0:12})"
echo

allowed="$(sed 's/#.*//' "$allow_file" | sed 's/[[:space:]]*$//' | grep -v '^$' || true)"

modified="$(git diff --name-only --diff-filter=MD "$base"...HEAD || true)"
added="$(git diff --name-only --diff-filter=A "$base"...HEAD || true)"

status=0

# Rule 1 -- modified upstream files must be on the allowlist.
unlisted=""
while IFS= read -r path; do
  [ -n "$path" ] || continue
  if ! printf '%s\n' "$allowed" | grep -Fxq -- "$path"; then
    unlisted="${unlisted}${path}"$'\n'
  fi
done <<< "$modified"

if [ -n "$unlisted" ]; then
  status=1
  echo "FAIL: upstream files modified without an entry in $allow_file:"
  printf '%s' "$unlisted" | sed 's/^/  /'
  echo
  echo "  Can the change live in a fork-owned directory instead? If not, add the"
  echo "  path to $allow_file with a one-line reason."
  echo
fi

# Rule 2 -- added files must be in a directory we own.
intruders=""
while IFS= read -r path; do
  [ -n "$path" ] || continue
  owned=0
  for root in "${fork_owned[@]}"; do
    case "$path" in "$root"*) owned=1; break;; esac
  done
  [ "$owned" -eq 1 ] || intruders="${intruders}${path}"$'\n'
done <<< "$added"

if [ -n "$intruders" ]; then
  status=1
  echo "FAIL: files added outside a fork-owned directory:"
  printf '%s' "$intruders" | sed 's/^/  /'
  echo
  echo "  A file under an upstream project directory is glob-compiled into"
  echo "  upstream's assembly and collides on every merge. Move it under one of:"
  printf '    %s\n' "${fork_owned[@]}"
  echo
fi

# Not a failure: an allowlist entry we no longer need, e.g. because the change
# landed upstream.
stale=""
while IFS= read -r path; do
  [ -n "$path" ] || continue
  if ! printf '%s\n' "$modified" | grep -Fxq -- "$path"; then
    stale="${stale}${path}"$'\n'
  fi
done <<< "$allowed"

if [ -n "$stale" ]; then
  echo "note: $allow_file lists paths this fork no longer modifies:"
  printf '%s' "$stale" | sed 's/^/  /'
  echo "  Landed upstream? Delete the entry."
  echo
fi

if [ "$status" -eq 0 ]; then
  ins_del="$(git diff --shortstat --diff-filter=MD "$base"...HEAD || true)"
  echo "OK: $(printf '%s\n' "$modified" | grep -c . || true) upstream files touched, all accounted for."
  [ -n "$ins_del" ] && echo "   $(echo "$ins_del" | sed 's/^ *//')"
fi

exit "$status"
