#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
GUARD="$HERE/agent-guard.sh"

fail() {
  printf 'worker-commit: FAILED: %s\n' "$*" >&2
  exit 1
}

[ "$#" -eq 2 ] ||
  fail 'usage: worker-commit.sh "<subject>" "<body>"'

subject="$1"
body="$2"

[ -n "$subject" ] || fail "empty subject"
[ -n "$body" ] || fail "empty body"
[[ "$subject" != *$'\n'* ]] || fail "subject must be one line"

branch="$(git symbolic-ref --short -q HEAD)" ||
  fail "detached HEAD"

[[ "$branch" =~ ^feature/issue-([1-9][0-9]*)-[a-z0-9]+(-[a-z0-9]+)*$ ]] ||
  fail "'$branch' is not a worker issue branch"

issue="${BASH_REMATCH[1]}"

"$GUARD" identity
"$GUARD" branch "$issue"

git diff --cached --quiet && fail "nothing staged"
git diff --cached --check

msg="$(mktemp)"
trap 'rm -f "$msg"' EXIT

printf '%s\n\n%s\n' "$subject" "$body" > "$msg"
"$GUARD" message "$msg"

git commit -F "$msg"
