<!-- {"key":"cover"} -->
# Teach the highlighter your language

This TOML block only highlights because a grammar sits next to the deck — the build would fail without it.

```toml
# deploy.toml — highlighted by syntaxes/toml.sublime-syntax
[service.web]
image = "registry.example.com/web:1.5.0"
replicas = 3
public = true
deployed_at = 2026-07-12T09:30:00Z
```

---
<!-- {"key":"error"} -->
# An unknown tag stops the build

Peitho highlights at build time with syntect. A language tag it cannot resolve is a parse error with a line number — never silently plain text.

```
× slide 1 ('cover'), line 6: unknown code language 'toml'
  = help: use a language name syntect recognizes (e.g. rust, js,
    py, sh, json, yaml, html, css, md, go, c, cpp, java, rb)
    or remove the tag
```

---
<!-- {"key":"convention"} -->
# Drop a grammar next to the deck

- Peitho auto-detects `syntaxes/` beside `deck.md` and reads every `*.sublime-syntax`
- Custom grammars augment the built-in set instead of replacing it
- An explicit `syntaxes:` key in frontmatter can point anywhere else

<!-- The built-in set is syntect's default Sublime package set — TOML genuinely is not in it, which is why it makes a good demo language. -->

---
<!-- {"key":"grammar"} -->
# A grammar is a page of YAML

Scopes map to `hl-*` classes on spans; the theme CSS owns the colors.

```yaml
contexts:
  main:
    - match: '#.*$'
      scope: comment.line.number-sign.toml
    - match: '\b(true|false)\b'
      scope: constant.language.boolean.toml
    - match: '"'
      scope: punctuation.definition.string.begin.toml
      push: string
```
