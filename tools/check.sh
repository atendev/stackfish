#!/usr/bin/env bash
# Repo checks. No argument or --fast runs fmt and clippy (the pre-commit hook).
# --full adds the workspace tests and the tools unittests (CI and audits).
set -euo pipefail

usage() {
    echo "usage: tools/check.sh [--fast | --full]" >&2
}

mode=fast
case "$#:${1-}" in
    0: | 1:--fast) ;;
    1:--full) mode=full ;;
    *) usage; exit 2 ;;
esac

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

run() {
    local name=$1
    shift
    echo "==> $name"
    if ! "$@"; then
        echo "check failed: $name" >&2
        exit 1
    fi
}

# Mirrors the skip condition in crates/engine/tests/db_scripts_smoke.rs.
db_missing() {
    local db="${STACKFISH_DB_DIR:-$root/../stackfish-db}/stackfish.db"
    [ -r "$db" ] || return 0
    ! head -c 16 "$db" | cmp -s - <(printf 'SQLite format 3\0')
}

run fmt cargo fmt --all -- --check
run clippy cargo clippy --workspace --all-targets -- -D warnings

if [ "$mode" = full ]; then
    run test cargo test --workspace
    run tools-unittest python3 -m unittest discover -s tools

    # TODO: fail instead of skipping when a strict missing-db mode is set
    if db_missing; then
        msg="stackfish-db is not checked out, so db_scripts_smoke.rs skipped"
        if [ "${GITHUB_ACTIONS-}" = true ]; then
            echo "::notice::$msg"
        else
            echo "note: $msg"
        fi
    fi
fi

echo "all $mode checks passed"
