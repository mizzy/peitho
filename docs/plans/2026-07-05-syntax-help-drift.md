# 2026-07-05 — syntax help drift (Issue #105)

## Problem

`Highlighter::validate_language` in `crates/peitho-core/src/highlight.rs`
returns a hard-coded example list on unknown language tags:

```
use a language name syntect recognizes (e.g. rust, js, ts, py, sh, toml, json) or remove the tag
```

`ts` is in the list, but `SyntaxSet::load_defaults_newlines()` does not
carry a TypeScript syntax. A user who follows the help's own suggestion
gets the same error again. The invariant "help text leads somewhere
valid" is broken.

## Root cause

The help text is authored independently of the `SyntaxSet`. Two
independent sources of truth — the hard-coded string and the actual
loaded set — are kept in sync by a human convention that failed. A
future syntect upgrade (adding or removing a language) would silently
re-introduce the same class of bug.

## Three-lens verdict

Fixing symptom-level (drop `ts` from the string) leaves the drift
condition intact. The correct fix is to **derive the example list from
the loaded `SyntaxSet`** so that any language mentioned in the help is
guaranteed to be recognized.

- **Long-term**: syntect version bumps auto-propagate; no manual sync.
- **Type/data-safety**: single source of truth for "which tokens are
  recognized" — the `SyntaxSet` itself. The `example_tokens()` function
  cannot return a token the set rejects (it filters through
  `find_syntax_by_token`).
- **Root-cause**: the drift condition (help ⇄ set independently
  authored) is removed, not filtered around.

## Design

Replace the hard-coded string with a small helper that filters a
curated preference list against the loaded `SyntaxSet`:

```rust
impl Highlighter {
    pub fn validate_language(&self, token: &str, line: usize) -> Result<()> {
        if self.syntax_set.find_syntax_by_token(token).is_some() {
            return Ok(());
        }
        Err(BuildError::new(
            ErrorKind::Parse,
            Some(line),
            format!("unknown code language '{token}'"),
            format!(
                "use a language name syntect recognizes (e.g. {}) or remove the tag",
                self.example_tokens().join(", ")
            ),
        ))
    }

    fn example_tokens(&self) -> Vec<&'static str> {
        const PREFERRED: &[&str] = &[
            "rust", "js", "py", "sh", "toml", "json", "yaml", "html", "css",
            "md", "go", "c", "cpp", "java", "rb",
        ];
        PREFERRED
            .iter()
            .copied()
            .filter(|t| self.syntax_set.find_syntax_by_token(t).is_some())
            .collect()
    }
}
```

### Why a curated `PREFERRED` list and not "top N from the set"

`SyntaxSet` exposes ~70 syntaxes; dumping all of them (or all their
tokens) would produce an unhelpful wall of text. A curated 10-15 line
list of common, short language names keeps the help concise. The list
is a *display filter*, not an authority — the authority remains
`find_syntax_by_token`. Anything on `PREFERRED` that isn't loaded is
silently dropped, so a future syntect release that removes or renames
a language auto-corrects the help.

### Why `ts` is deliberately absent from `PREFERRED`

`ts` is not in syntect's default set, and Option 2 (bundling a TypeScript
syntax) is out of scope for this PR — that's a separate feature ask. If
the author later decides to bundle a TS syntax, adding `"ts"` to
`PREFERRED` becomes the entire diff and it starts appearing in help
automatically. Until then, users who need TS can drop a
`.sublime-syntax` under a deck's `syntaxes/` directory (already
supported by `with_user_dir`).

## TDD plan

**Red 1** — regression test that today's help contains `ts`:

```rust
#[test]
fn help_text_never_suggests_a_token_the_default_set_rejects() {
    let highlighter = Highlighter::defaults();
    let err = highlighter.validate_language("notalang", 1).unwrap_err();
    let msg = err.to_string();
    for token in ["rust", "js", "py", "sh", "toml", "json"] {
        if msg.contains(&format!(" {token}")) || msg.contains(&format!(" {token},")) {
            assert!(
                highlighter.validate_language(token, 1).is_ok(),
                "help text suggests '{token}' but the default set rejects it"
            );
        }
    }
}
```

More directly, cover the specific `ts` regression:

```rust
#[test]
fn help_text_does_not_suggest_ts_when_default_set_lacks_it() {
    let highlighter = Highlighter::defaults();
    assert!(highlighter.validate_language("ts", 1).is_err(),
        "if this fires, syntect defaults now include TypeScript; adjust PREFERRED");
    let err = highlighter.validate_language("notalang", 1).unwrap_err();
    let msg = err.to_string();
    // Every token the help suggests must itself validate.
    // Parse the parenthetical (e.g. rust, js, py, sh, ...) and probe each.
    let list_start = msg.find("e.g. ").expect("help preamble present") + 5;
    let list_end = msg[list_start..].find(')').unwrap() + list_start;
    for raw in msg[list_start..list_end].split(", ") {
        let token = raw.trim();
        assert!(
            highlighter.validate_language(token, 1).is_ok(),
            "help suggests '{token}' but the default set rejects it"
        );
    }
}
```

The second is the stronger invariant assertion and covers Issue #105
directly. This is the one to lock in.

**Green** — implement `example_tokens` + rewrite `validate_language`
help to use it.

**Refactor** — extract the parenthetical parsing pattern into a small
test helper if it grows.

## Files touched

- `crates/peitho-core/src/highlight.rs` — replace hard-coded help,
  add `example_tokens`, add regression test.
- `docs/plans/2026-07-05-syntax-help-drift.md` — this file.

No changes to `bindings/`, TS side, or any other crate. The public API
of `Highlighter::validate_language` is unchanged — only the error
message content shifts, and the shape (`ErrorKind::Parse`, `Some(line)`,
"unknown code language 'X'") is preserved.

## Non-goals

- Bundling a TypeScript syntax (Option 2). If the author wants that,
  it's a follow-up: `builder.add_from_folder` on a bundled directory,
  or drop the sublime-syntax under an included `syntaxes/` embed.
- Redesigning the error message shape.
- Changing `with_user_dir` / `with_user_files` — user syntaxes
  automatically appear in help iff their tokens are in `PREFERRED`,
  which is fine (user syntaxes are typically for one-deck use and the
  user already knows the token name).
