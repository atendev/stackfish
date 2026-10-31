#!/usr/bin/env bash
# Copies tools/hooks/pre-commit into this clone's hooks dir.
# Leaves core.hooksPath unset so the Git LFS hooks keep running.
set -euo pipefail

src="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/hooks/pre-commit"
cd "$(git rev-parse --show-toplevel)"

if hooks_path="$(git config --get core.hooksPath)"; then
    echo "core.hooksPath is set to '$hooks_path', so a hook in .git/hooks would be ignored." >&2
    echo "Unset it, or copy tools/hooks/pre-commit into '$hooks_path' yourself." >&2
    exit 1
fi

hooks_dir="$(git rev-parse --git-path hooks)"
mkdir -p "$hooks_dir"
dest="$hooks_dir/pre-commit"

if [ -e "$dest" ] && ! cmp -s "$src" "$dest"; then
    backup="$dest.bak"
    n=1
    while [ -e "$backup" ]; do
        backup="$dest.bak.$n"
        n=$((n + 1))
    done
    mv "$dest" "$backup"
    echo "moved the existing pre-commit hook to $backup"
fi

cp "$src" "$dest"
chmod +x "$dest"
echo "installed $dest"
