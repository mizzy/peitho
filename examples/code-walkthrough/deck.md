<!-- {"key":"markers"} -->
# Phases live in the type

```rust
pub struct Deck<Phase> {
    slides: Vec<Slide>,
    _phase: PhantomData<Phase>,
}

pub enum Parsed {}
pub enum Checked {}
pub enum Rendered {}
```

- A deck carries its pipeline phase as a type parameter.
- The phase markers are empty enums: they exist only at compile time.

---
<!-- {"key":"constructors"} -->
# Constructors stay private

```rust
impl Deck<Checked> {
    pub(crate) fn checked(
        slides: Vec<Slide>,
    ) -> Self {
        Self {
            slides,
            _phase: PhantomData,
        }
    }
}
```

- Only the checker can mint a `Deck<Checked>`.
- Outside the crate there is no way to fake a checked deck.

---
<!-- {"key":"signature"} -->
# The signature is the gate

```rust
pub fn render_deck(
    deck: Deck<Checked>,
    template: &Template,
) -> Result<Deck<Rendered>> {
    // an unchecked deck cannot reach here
}
```

- The renderer does not re-validate anything.
- The precondition is spelled in the parameter type.

---
<!-- {"key":"payoff"} -->
# The compiler reviews every caller

```rust
let parsed = parse_markdown(input)?;
render_deck(parsed, &template);

// error[E0308]: mismatched types
//   expected `Deck<Checked>`
//      found `Deck<Parsed>`
```

- A new call site cannot forget the check.
- This slide is highlighted by a keyed override in `overrides.css`.
