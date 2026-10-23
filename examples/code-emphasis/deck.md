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

# Interleaving Two Blocks

`@N` names an absolute slide step, so the pointer can return to an earlier
block instead of being locked to source order.

```rust {1@1|3@3} title="request.rs"
let request = read_request()?;
let config = load_config()?;
dispatch(request, config)
```

```rust {|1-3@2} title="config.rs"
let file = open_config()?;
let parsed = parse(file)?;
validate(parsed)
```

- Step 1 points at `request.rs`, step 2 at `config.rs`, and step 3 returns to
  `request.rs`.

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

Static emphasis takes no step, so `next` walks only the revealed bullets while
the code stays emphasized.

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

---

# Partial Types, Not Whole Lines

Quoted entries emphasize only the differing type, even across syntax spans.

```text {2:"aws.ec2.Vpc.Id"@1|3:"'dev' | 'stg' | 'prod'"@2} title="module.crn"
arguments {
  vpc_id: aws.ec2.Vpc.Id
  environment: 'dev' | 'stg' | 'prod'
}
```

```hcl {1:"string"@1|2:"set(string)"@2} title="variables.tf"
variable "vpc_id"      { type = string }
variable "environment" { type = set(string) }
```

- Step 1 compares VPC types; step 2 compares environment types.
