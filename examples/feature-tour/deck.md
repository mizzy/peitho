---
time: 8m
---

<!-- {"key":"tour-cover","section":"Basics","time":"2m"} -->
# The Peitho Feature Tour

<!-- Welcome! This deck is plain Markdown — every slide you see is generated at build time. -->

<!-- This cover matched the `cover` layout purely by its shape: one heading, nothing else. No layout name written anywhere. -->

---

<!-- {"key":"separation"} -->
# Content stays Markdown, design stays CSS

This deck uses **four layouts** and *one theme* — plain HTML and CSS living next to the deck. Swap the whole look by pointing at another directory; the gallery at [peitho.gosu.ke](https://peitho.gosu.ke/) does exactly that.

- Write slides without thinking about design
  - no font-size fiddling
  - no pixel nudging
- Review decks as diffs, like any other code

<!-- Emphasize: the same Markdown conventions drive every example on the demo site. -->

---

<!-- {"key":"checks","layout":"agenda","section":"Contracts","time":"3m"} -->
# Everything the build catches

- A slot with too much or too little content
- Content the layout has no slot for
- A code block in a deck whose layout forbids code
- A keyed CSS override pointing at a key that no longer exists
- An unknown syntax-highlighting language tag

<!-- This slide is list-only, so it structurally matches both `topic` and `agenda`. The {"layout":"agenda"} request resolves that ambiguity — leaving it out is a build error, never a silent pick. -->

---

<!-- {"key":"contract"} -->
# One contract, end to end

The Rust domain types are the single source of truth; this deck's own `manifest.json` is emitted from them.

```rust
pub struct ManifestSection {
    name: String,
    start_index: usize,
    end_index: usize,
    planned_duration_ms: u64,
}
```

```json
{ "name": "Basics", "startIndex": 0, "endIndex": 1, "plannedDurationMs": 120000 }
```

<!-- The code slot here takes 1..* blocks — two fences on one slide is fine, and each language is highlighted at build time by syntect. -->

---

<!-- {"key":"cli","section":"Presenting","time":"3m"} -->
# Build, present, publish

Three commands cover the whole life cycle. No runtime JS in the distributed slides.

```bash
peitho build deck.md --watch
peitho present deck.md
peitho publish -- aws s3 sync dist/ s3://your-bucket/
```

<!-- Mention that present places slides and the presenter view across two displays automatically. -->

---

<!-- {"key":"timing"} -->
# The presenter view keeps you honest

Planned time lives in the deck itself; sections mark milestones; actuals accumulate as you speak.

- `time: 8m` in this deck's frontmatter is the budget
- Section markers split it: Basics, Contracts, Presenting
- Speaker notes are plain HTML comments — this slide has one

<!-- Press Space to start the timer and watch the agenda track actual vs. planned per section. -->

---

<!-- {"key":"closing"} -->
# Now write your own
