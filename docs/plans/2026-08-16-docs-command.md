# peitho docs: embedded guide for offline/AI-agent reference (Issue #429)

Date: 2026-08-16
Issue: #429

## Problem

The deep documentation (frontmatter keys, layout/slot contract, explicit slot
syntax, CLI reference) lives in `site/content/guide/*.md` and is only
reachable online. An agent (or offline human) working from the installed
binary has only the README that Homebrew happens to ship. Author request
2026-08-16: make the complete reference discoverable from the binary itself.

## Design

New CLI subcommand `peitho docs`, single-sourced from the same guide pages
the docs site builds from (no second wiring site to drift):

- `peitho docs` — list topics (slug + the page's `description` from its Zola
  frontmatter), one per line, plus a pointer to `peitho docs <topic>` /
  `--all`.
- `peitho docs <topic>` — print that page as plain Markdown on stdout.
- `peitho docs --all` — print every page in guide weight order, separated by
  a `# <title>` heading per page.
- Unknown topic → error listing valid topics (line-numbered convention does
  not apply; this is a CLI-argument error).

Embedding: `include_str!` per guide page in a new `crates/peitho/src/docs.rs`
module with a static topic table `(slug, title, description, body)`. The Zola
TOML frontmatter (`+++ ... +++`) is stripped at compile-time-adjacent runtime
(small parser in docs.rs); `title`/`description`/`weight` come from it, so the
table is derived, not hand-maintained.

Output stays plain Markdown: no paging, no ANSI — what an agent ingests.

Drift guard: a test enumerates `../../site/content/guide/*.md` (excluding
`_index.md`) from the repo at test time and asserts the embedded topic set
matches, so adding a guide page without wiring it into docs.rs fails CI.
(`include_str!` already guarantees embedded bytes match the files.)

Discoverability: `--help` long about text mentions `peitho docs`; README gets
a short section.

## Tests (TDD order)

1. Topic table: every embedded page parses (frontmatter stripped, title and
   description non-empty).
2. Drift test: embedded slugs == `site/content/guide/*.md` slugs.
3. `docs_list` output contains each slug and description.
4. `docs_topic` prints body without `+++` frontmatter.
5. `docs_all` contains every page in weight order.
6. Unknown topic errors and names valid topics.

## Non-goals

- No HTML rendering, no pager, no search.
- No README embedding (README stays the human quick reference).
- No llms.txt on the site (separate concern; can follow later if wanted).
