import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(__file__))
from comment_diff import LexError, compare, lex


def texts(src):
    return [t for t, _ in lex(src)[0]]


class StripTests(unittest.TestCase):
    def test_line_and_doc_comments_dropped(self):
        self.assertEqual(texts("/// doc\n//! inner\n// plain\nfn a() {}"), texts("fn a() {}"))

    def test_nested_block_comment(self):
        self.assertEqual(texts("a /* x /* y */ z */ b"), ["a", "b"])

    def test_comment_markers_inside_strings_are_kept(self):
        self.assertEqual(texts('let s = "// not /* a comment */";')[3], '"// not /* a comment */"')

    def test_raw_string_with_quote_and_hashes(self):
        src = 'let s = r##"has "# and // inside"##; // real'
        self.assertEqual(texts(src)[3], 'r##"has "# and // inside"##')
        self.assertEqual(texts(src)[-1], ";")

    def test_byte_and_c_strings(self):
        self.assertEqual(texts('b"//" c"//" br#"//"#')[0:3], ['b"//"', 'c"//"', 'br#"//"#'])

    def test_char_literal_quote(self):
        self.assertEqual(texts("let c = '\"'; // x"), ["let", "c", "=", "'\"'", ";"])

    def test_escaped_char_literal(self):
        self.assertEqual(texts(r"let c = '\''; // x")[3], r"'\''")

    def test_lifetime_is_not_a_char(self):
        self.assertEqual(texts("fn f<'a>(x: &'a str) {} // c"), texts("fn f<'a>(x: &'a str) {}"))
        self.assertEqual(texts("<'a>")[:3], ["<", "'", "a"])

    def test_whitespace_is_ignored(self):
        self.assertEqual(texts("fn   a ( )\n{\n}"), texts("fn a(){}"))

    def test_unterminated_block_comment_raises(self):
        with self.assertRaises(LexError):
            lex("/* never closed")

    def test_comment_lines_and_todos(self):
        _, lines, todos = lex("a\n// TODO: one\nb /* TODO two\nmore */\nc")
        self.assertEqual(lines, {2, 3, 4})
        self.assertEqual(todos, 2)


class CompareTests(unittest.TestCase):
    def test_comment_only_change_passes(self):
        problems, *_ = compare("// long\n// long\nfn a() {}", "fn a() {}")
        self.assertEqual(problems, [])

    def test_code_change_fails(self):
        problems, *_ = compare("fn a() {}", "fn b() {}")
        self.assertEqual(len(problems), 1)
        self.assertIn("code differs", problems[0])

    def test_code_change_inside_string_fails(self):
        problems, *_ = compare('let s = "a";', 'let s = "b";')
        self.assertEqual(len(problems), 1)

    def test_todo_drop_fails(self):
        problems, *_ = compare("// TODO: x\nfn a() {}", "fn a() {}")
        self.assertEqual(problems, ["TODO count dropped from 1 to 0"])

    def test_added_and_removed_files_fail(self):
        self.assertEqual(compare(None, "fn a() {}")[0], ["file added"])
        self.assertEqual(compare("fn a() {}", None)[0], ["file removed"])


if __name__ == "__main__":
    unittest.main()
