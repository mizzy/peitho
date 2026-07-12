# breaks: true demo in the keynote example (Issue #278)

Date: 2026-07-13

## Goal

The `breaks` frontmatter key (single newlines render as hard line breaks when
`true`) appeared in no example deck.

## Approach

`breaks` is deck-level, so it must go in a deck whose existing paragraphs tolerate
it. The keynote deck qualifies: every existing paragraph is a single source line
(audited), so enabling `breaks: true` changes nothing about the current slides. Its
centered serif aesthetic also suits line-break-significant content.

Changes: add `breaks: true` frontmatter, insert one verse slide ("Line breaks are
content" — one paragraph of three short lines, dispatching structurally to the
statement layout), and mention the behavior on the keynote example page. No Makefile
or landing wiring needed (existing deck; landing derives per #274).

A dedicated example was rejected: a one-key deck-level flag does not carry a whole
gallery entry, and retrofitting is safe here.
