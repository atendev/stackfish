#!/usr/bin/env python3
"""Prove a comment pass changed no code.

Compares each .rs file in the working tree with its version at a git ref.
Comments are dropped and whitespace is ignored, so only a code difference
fails. Also fails if the number of TODOs in comments drops.

    python3 tools/comment_diff.py [--ref REF] [paths...]

REF defaults to HEAD and paths default to `crates`. Exit codes: 0 clean,
1 code differs or a TODO was lost, 2 usage or git error.
"""

import argparse
import os
import re
import subprocess
import sys

IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
NUMBER = re.compile(r"[0-9][A-Za-z0-9_]*")
RAW_STRING = re.compile(r'(?:br|cr|r)(#*)"')
PLAIN_STRING = re.compile(r'(?:b|c)"')
TODO = re.compile(r"\bTODO\b")


class LexError(Exception):
    pass


def lex(src):
    """Return (tokens, comment_lines, todo_count).

    tokens is a list of (text, line). comment_lines is the set of 1-based
    line numbers that contain comment text.
    """
    tokens = []
    comment_lines = set()
    todos = 0
    i = 0
    line = 1
    n = len(src)

    def string_end(start):
        j = start
        while j < n:
            c = src[j]
            if c == "\\":
                j += 2
            elif c == '"':
                return j + 1
            else:
                j += 1
        raise LexError(f"unterminated string at line {line}")

    while i < n:
        c = src[i]
        if c == "\n":
            line += 1
            i += 1
        elif c.isspace():
            i += 1
        elif src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j == -1 else j
            comment_lines.add(line)
            todos += len(TODO.findall(src, i, j))
            i = j
        elif src.startswith("/*", i):
            depth = 0
            j = i
            while True:
                if src.startswith("/*", j):
                    depth += 1
                    j += 2
                elif src.startswith("*/", j):
                    depth -= 1
                    j += 2
                    if depth == 0:
                        break
                elif j >= n:
                    raise LexError(f"unterminated block comment at line {line}")
                else:
                    j += 1
            span_lines = src.count("\n", i, j)
            comment_lines.update(range(line, line + span_lines + 1))
            todos += len(TODO.findall(src, i, j))
            line += span_lines
            i = j
        elif c == '"':
            j = string_end(i + 1)
            tokens.append((src[i:j], line))
            line += src.count("\n", i, j)
            i = j
        elif c == "'":
            if src.startswith("\\", i + 1):
                j = src.find("'", i + 3)
                if j == -1:
                    raise LexError(f"unterminated char literal at line {line}")
                tokens.append((src[i : j + 1], line))
                i = j + 1
            elif src.startswith("'", i + 2):
                tokens.append((src[i : i + 3], line))
                i += 3
            else:
                tokens.append(("'", line))
                i += 1
        elif c.isalpha() or c == "_":
            raw = RAW_STRING.match(src, i)
            plain = PLAIN_STRING.match(src, i)
            if raw:
                closer = '"' + raw.group(1)
                j = src.find(closer, raw.end())
                if j == -1:
                    raise LexError(f"unterminated raw string at line {line}")
                j += len(closer)
                tokens.append((src[i:j], line))
                line += src.count("\n", i, j)
                i = j
            elif plain:
                j = string_end(plain.end())
                tokens.append((src[i:j], line))
                line += src.count("\n", i, j)
                i = j
            elif src.startswith("b'", i):
                j = src.find("'", i + 3 if src.startswith("\\", i + 2) else i + 2)
                if j == -1:
                    raise LexError(f"unterminated byte literal at line {line}")
                tokens.append((src[i : j + 1], line))
                i = j + 1
            else:
                m = IDENT.match(src, i)
                tokens.append((m.group(), line))
                i = m.end()
        elif c.isdigit():
            m = NUMBER.match(src, i)
            tokens.append((m.group(), line))
            i = m.end()
        else:
            tokens.append((c, line))
            i += 1
    return tokens, comment_lines, todos


def git(root, *args):
    result = subprocess.run(
        ["git", *args], cwd=root, capture_output=True, text=True, encoding="utf-8"
    )
    if result.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)}: {result.stderr.strip()}")
    return result.stdout


def rust_files(root, ref, paths):
    old = git(root, "ls-tree", "-r", "--name-only", ref, "--", *paths).splitlines()
    new = git(
        root, "ls-files", "--cached", "--others", "--exclude-standard", "--", *paths
    ).splitlines()
    return sorted(f for f in set(old) | set(new) if f.endswith(".rs"))


def read_old(root, ref, path):
    result = subprocess.run(
        ["git", "show", f"{ref}:{path}"],
        cwd=root,
        capture_output=True,
        encoding="utf-8",
    )
    return result.stdout if result.returncode == 0 else None


def read_new(root, path):
    full = os.path.join(root, path)
    if not os.path.isfile(full):
        return None
    with open(full, encoding="utf-8") as f:
        return f.read()


def first_difference(old, new):
    for k in range(min(len(old), len(new))):
        if old[k][0] != new[k][0]:
            return k
    return min(len(old), len(new))


def describe(tokens, k):
    if k >= len(tokens):
        return "end of file"
    window = " ".join(t for t, _ in tokens[max(0, k - 3) : k + 4])
    return f"line {tokens[k][1]}: ... {window} ..."


def compare(old_src, new_src):
    """Return (problems, old_comment_lines, new_comment_lines, old_todos, new_todos)."""
    problems = []
    if old_src is None:
        return ["file added"], 0, 0, 0, 0
    if new_src is None:
        return ["file removed"], 0, 0, 0, 0
    old_tokens, old_cl, old_todos = lex(old_src)
    new_tokens, new_cl, new_todos = lex(new_src)
    if [t for t, _ in old_tokens] != [t for t, _ in new_tokens]:
        k = first_difference(old_tokens, new_tokens)
        problems.append(
            f"code differs. before: {describe(old_tokens, k)}; "
            f"after: {describe(new_tokens, k)}"
        )
    if new_todos < old_todos:
        problems.append(f"TODO count dropped from {old_todos} to {new_todos}")
    return problems, len(old_cl), len(new_cl), old_todos, new_todos


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--ref", default="HEAD", help="git ref to compare against")
    parser.add_argument("paths", nargs="*", default=["crates"])
    args = parser.parse_args()

    try:
        root = git(os.getcwd(), "rev-parse", "--show-toplevel").strip()
        paths = [os.path.relpath(os.path.abspath(p), root) for p in args.paths]
        files = rust_files(root, args.ref, paths)
    except RuntimeError as e:
        print(e, file=sys.stderr)
        return 2

    failures = 0
    totals = [0, 0, 0, 0]
    for path in files:
        old_src = read_old(root, args.ref, path)
        new_src = read_new(root, path)
        try:
            problems, ocl, ncl, otd, ntd = compare(old_src, new_src)
        except LexError as e:
            problems, ocl, ncl, otd, ntd = [f"cannot lex: {e}"], 0, 0, 0, 0
        for k, v in enumerate((ocl, ncl, otd, ntd)):
            totals[k] += v
        if problems:
            failures += 1
            print(f"FAIL {path}")
            for p in problems:
                print(f"     {p}")
        elif ocl != ncl:
            print(f"ok   {path}  comment lines {ocl} -> {ncl}  TODO {otd} -> {ntd}")

    print(
        f"{len(files)} files, comment lines {totals[0]} -> {totals[1]}, "
        f"TODO {totals[2]} -> {totals[3]}, {failures} failing"
    )
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
