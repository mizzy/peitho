---
public_reveal: steps
---

# Code Line Emphasis

Emphasis points at the line you are talking about — a separate layer from
syntax highlighting, which colors code by what it *is*.

```rust {3}
pub struct Deck<Phase> {
    slides: Vec<Slide>,
    _phase: PhantomData<Phase>,
}
```

- Static emphasis has no `|`, so it consumes no steps.
- It ships with the deck, published output included.

---

# Stepping Through a Function

A `|` makes one step per group, and the emphasis moves rather than accumulating.

```rust {2|4-6|9}
fn handle(req: Request) -> Response {
    let session = authenticate(&req)?;
    let body = match req.route() {
        Route::List => render_list(&session),
        Route::Detail(id) => render_detail(&session, id),
        Route::Unknown => return Response::not_found(),
    };
    Response::ok(body)
}
```

- The code stays visible; only the pointer moves.

---

# Ranges and No Language Tag

Within a group, `,` separates entries and `-` makes a range. The language tag
is optional, so emphasis works on plain text too.

```{1,4-5}
GET /api/decks        200  12ms
GET /api/decks/42     200   8ms
POST /api/decks       201  31ms
GET /api/decks/99     404   3ms
GET /api/decks/99     404   2ms
```

- `{1,4-5}` emphasizes line 1 and lines 4-5 together.

---

# Sharing Steps With Reveal

Emphasis steps are reveal steps, so `next` walks through both in source order.

::: {reveal}

- First the context arrives.
- Then the code, already emphasized.

:::

```rust {2}
pub(crate) fn checked(slides: Vec<Slide>) -> Self {
    Self { slides, _phase: PhantomData }
}
```

<!--
Steps 1-2 reveal the bullets; the code block carries static emphasis, so it
appears fully emphasized rather than consuming a step of its own.
-->

---

# Ordinary Code Is Untouched

A block with no emphasis spec renders exactly as it always has — no line
wrapping, no dimming, byte-identical output.

```rust
pub fn render_deck(deck: Deck<Checked>) -> Result<Deck<Rendered>> {
    // nothing here opts into emphasis
}
```

- Decks that do not use the notation are unaffected.
- Emphasis is opt-in, one code block at a time.

---

# Labels Stay With Their Blocks

`title="..."` puts a caption directly above its own block, so source and output
stay paired.

```hcl title="main.tf" {2}
resource "aws_vpc" "main" {
  tags = { Name = upper(aws_vpc.main.id) }
}
```

```text {2} title="terraform plan"
  + tags = {
      + "Name" = (known after apply)
    }
```

- Titles compose with `{…}` in either order.
