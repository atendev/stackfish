#!/usr/bin/env bash
# Builds the adapter and runs the Godot smoke test headless, one process per mode so a crash in one
# mode shows up as its exit code. Set GODOT to the Godot 4.2+ binary.
#
# SAFEGUARDS_OFF=1 builds in release with gdext's own thread check compiled out, so the adapter's
# guard is the one that answers a wrong-thread call. The smoke project is copied under target/ for
# that run, because Godot's editor binary reads the .debug library entries.
set -u
here="$(cd "$(dirname "$0")" && pwd)"
repo="$here/../../.."
godot="${GODOT:-godot}"
project="$here"
# The wrong-thread result that `play` must see: null with gdext's safeguards on, an empty array with them off.
expect=null

if [ -n "${SAFEGUARDS_OFF:-}" ]; then
	expect=empty
	cargo build --release -p stackfish-godot --manifest-path "$repo/Cargo.toml" \
		--features godot/safeguards-release-disengaged || exit 1
	project="$repo/target/smoke-release"
	rm -rf "$project" && mkdir -p "$project"
	cp "$here/project.godot" "$here/smoke.gd" "$project/"
	sed 's#\.\./\.\./\.\./target/debug#../../target/release#' "$here/stackfish.gdextension" > "$project/stackfish.gdextension"
else
	cargo build -p stackfish-godot --manifest-path "$repo/Cargo.toml" || exit 1
fi

# The first import after a fresh checkout can abort while it registers the extension, so run it twice.
"$godot" --headless --path "$project" --import >/dev/null 2>&1
"$godot" --headless --path "$project" --import >/dev/null 2>&1
status=0
# thread_owned is recorded only. With gdext's safeguards on it aborts Godot, and that is the finding.
for mode in play drop thread_owned; do
	echo "== $mode"
	"$godot" --headless --path "$project" --script smoke.gd -- "$mode" "$expect"
	code=$?
	echo "exit code $code"
	[ "$code" -eq 0 ] || [ "$mode" = thread_owned ] || status=1
done
exit $status
