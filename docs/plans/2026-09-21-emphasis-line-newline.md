# Exclude line endings from emphasized code HTML

Date: 2026-09-21
Issue: #561

## Problem

Line-emphasized Markdown code can render a source newline inside a syntax
scope and therefore inside a `.code-line` wrapper. It creates a blank visual
line and makes the code panel taller than its source.

## Root cause

`Highlighter::highlight_lines` correctly parses each source line with its
ending, but also passes it to HTML generation and trims only the final HTML
byte. A scope covering the ending puts its closing tag after the newline, so
the trim cannot reach it. The bug was not Markdown-specific: any scope covering
a line ending, including Rust, Bash, Python, and YAML line comments, leaked the
newline in the same way.

## Fix

Keep `ParseState::parse_line` on the complete source line. Generate HTML from
the line text without its LF or CRLF ending, and clamp every syntax operation
offset to that length. Syntect 5.3.0 applies equal-offset operations in order,
removes empty push/pop spans, and updates the stack for every operation. Remove
the obsolete HTML trim while retaining the existing close count so multi-line
scopes remain balanced.

## Tests

Add a highlighter property-style regression over Markdown, HTML, Rust, and
plain text: no returned line contains a newline, and stripping tags then
decoding entities recovers the exact source text. Include Rust multi-line
strings and block comments to protect cross-line scope state.

Add a render regression for a `markdown {1|2}` fence containing `# 見出し`
and `本文`; require exactly one newline in the `<code>` body, the separator
between its two `.code-line` wrappers. Run both regressions red before the
implementation, then run the requested workspace tests, clippy, formatting,
and bindings-drift gates.
