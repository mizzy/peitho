# Peitho v1.23.0 tour showcase

Add two slides immediately after “A slide is just Markdown” in `examples/peitho-tour/deck.md`. Keep the existing type-driven dispatch and give each slide a page-settings comment with only its unique `key`—never an explicit `layout`.

- **“Tables, quotes, and edits render natively”** (`key: "gfm-body"`): render a compact GFM table, a real blockquote, and a sentence containing real `~~strikethrough~~`; none of these examples should be hidden inside a code fence. Use concise explanatory copy and, if useful, a speaker note calling out the resulting `<table>`, `<blockquote>`, and `<del>` elements. Title plus body content dispatches to `topic`.
- **“Code blocks stay highlighted in quotes”** (`key: "container-code"`): render a small `toml` fence inside a real blockquote in the body, then pair it with a short top-level Markdown source fence in the code panel. The quote visibly exercises the container-code pipeline while the source fence makes the authoring pattern explicit. Title, body, and top-level code dispatch to `code`.

Include the extended highlighting set (#434) and container-code pipeline (#443) together on the second slide: one quoted TOML example demonstrates both without adding a third slide.

Increase the Write section budget from `6m` to `8m` and the frontmatter total from `23m` to `25m`; the section sum remains exact: `3 + 1 + 8 + 5 + 7 + 1 = 25m`.

Verify with `cargo run -q -p peitho -- build examples/peitho-tour/deck.md`; it must succeed without timing, slot, or dispatch errors. Then inspect the generated `dist/` output and confirm the table, `<del>`, quoted TOML highlighting, and both expected layouts.
