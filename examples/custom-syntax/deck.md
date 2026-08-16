<!-- {"key":"cover"} -->
# Teach the highlighter your language

This .crn block only highlights because a grammar sits next to the deck.

```crn
# app.crn — highlighted by syntaxes/crn.sublime-syntax
provider "aws" {
  region = "ap-northeast-1"
}

resource "service" "web" {
  image    = "registry.example.com/web:1.5.0"
  replicas = 3
  public   = true
}
```

---
<!-- {"key":"error"} -->
# An unknown tag stops the build

Peitho highlights at build time with syntect. A language tag it cannot resolve is a parse error with a line number — never silently plain text.

```
error: slide 1 ('cover'), line 6: unknown code language 'crn'
 help: use a language name syntect recognizes (e.g. rust, js, ts, py, sh, toml, json, yaml, html, css, md, go, c, cpp, java, rb) or remove the tag
```

---
<!-- {"key":"convention"} -->
# Drop a grammar next to the deck

- Peitho auto-detects `syntaxes/` beside `deck.md` and reads every `*.sublime-syntax`
- Custom grammars augment the bundled set instead of replacing it
- An explicit `syntaxes:` key in frontmatter can point anywhere else

<!-- Carina's .crn language is absent from the bundled syntax set, which is why it makes a good demo language. -->

---
<!-- {"key":"grammar"} -->
# A grammar is a page of YAML

Scopes map to `hl-*` classes on spans; the theme CSS owns the colors.

```yaml
contexts:
  main:
    - match: '#.*$'
      scope: comment.line.number-sign.crn
    - match: '\b(resource|provider|module)\b'
      scope: keyword.declaration.crn
    - match: '"'
      scope: punctuation.definition.string.begin.crn
      push: string
```
