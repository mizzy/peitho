+++
title = "peitho"
template = "index.html"

[extra.hero]
lead = "An HTML-native presentation tool that treats Markdown as the source of truth."
tagline = "The layout is the schema. Broken decks fail at build, not on stage."

[[extra.sections]]
title = "Write with AI, fix from diagnostics"

[[extra.sections.rows]]
title = "Contracts to write against"
meta = "Every layout declares what each slot accepts and how many items it takes. Give an AI the layouts and it knows what a slide may contain before it writes one."

[[extra.sections.rows]]
title = "Diagnostics to fix from"
meta = "peitho build and peitho lint name the slide, the line, and what is wrong, with help. The AI reads the error, fixes the deck, and builds again."

[[extra.sections.rows]]
title = "No HTML escape hatch"
meta = "Markdown cannot carry HTML; comments for page settings and speaker notes are the only exception. A fix cannot hide in the content as a one-off div or inline style: it goes into the content or into a layout, and the AI can write the layout too."

[[extra.sections]]
title = "From contract to slide"

[[extra.sections.panels]]
label = "layouts/title-body-code.html"
markdown = '''
```html
<h1><slot name="title" accepts="inline" arity="1"></slot></h1>
<slot name="body" accepts="blocks" arity="0..*"></slot>
<slot name="code" accepts="code" arity="0..1"></slot>
```'''

[[extra.sections.panels]]
label = "deck.md"
markdown = '''
````markdown
# Peitho Architecture

Markdown is the source of truth, while HTML and CSS own layout.

```rust
enum Phase { Parsed, Mapped, Checked, Rendered }
```

```rust
fn render(deck: Deck<Checked>) -> Deck<Rendered>
```
````'''

[[extra.sections.panels]]
label = "peitho build"
markdown = '''
```
error: slide 1 ('peitho-architecture'), line 5: slot 'code' got 2 item(s), but layout 'title-body-code' allows 0..1
 help: use a layout with more code capacity or remove one code block
```'''

[[extra.sections.panels]]
label = "After removing one code block"
deck_shot = "minimal"
alt = "The built slide: the title Peitho Architecture, one line of body text, and one Rust code block"

[[extra.sections]]
title = "What each check catches"

[[extra.sections.rows]]
title = "peitho build: structure"
meta = "Slot excess and deficiency, type mismatches, zero or ambiguous layout matches, broken references, unassigned content, and HTML in Markdown. A deck with any of these does not build."

[[extra.sections.rows]]
title = "peitho lint: rendering"
meta = "Content that overflows the slide, and text below 24pt unless the layout CSS lowers the floor for it."

[[extra.sections.rows]]
title = "peitho preview: your eyes"
meta = "Whether the slides say the right thing. Edits made in preview are written back to the Markdown."

[[extra.sections]]
title = "Pillars"

[[extra.sections.rows]]
title = "Separation of content and design"
meta = "Markdown owns the content; HTML and CSS own the layout and theme. Neither leaks into the other."

[[extra.sections.rows]]
title = "Version-controllable HTML/CSS layouts"
meta = "Layouts and CSS are normal files that diff and review cleanly. The layout itself is the schema (<slot name accepts arity>)."

[[extra.sections.rows]]
title = "Type-checked slot contracts"
meta = "Slot excess and deficiency, type mismatches, broken references, and unassigned content are all build errors with line numbers and help."

[[extra.sections]]
title = "Install"
code = "brew install mizzy/tap/peitho"

[[extra.sections]]
title = "Workflow"
shots_intro = "Preview while you edit, a presenter view when you speak, and a phone remote in your pocket."

[[extra.sections.shots]]
shot = "preview-overview"
title = "Preview overview"
meta = "peitho preview rebuilds on every save; press o for a tile overview of the whole deck."
alt = "The overview view: every slide as a tile in a scrollable grid"
path = "@/guide/getting-started.md"
anchor = "preview-while-editing"

[[extra.sections.shots]]
shot = "presenter-view"
title = "Presenter view"
meta = "Current and next slide, speaker notes, a timer with slide progress, and a per-section agenda."
alt = "Presenter view with current and next slides, notes, a timer, and a per-section agenda"
path = "@/guide/getting-started.md"
anchor = "present"

[[extra.sections.shots]]
shot = "remote-landscape"
title = "Phone remote"
meta = "peitho present --host serves a remote with notes and navigation; scan the QR code once and add it to your Home Screen."
alt = "Peitho remote in landscape: preview on the left, notes in the center, Previous and Next on the right edge rail"
path = "@/guide/getting-started.md"
anchor = "drive-the-deck-from-your-phone"

[[extra.sections]]
title = "Examples"
gallery_intro = "Same tool, same Markdown conventions — different decks."
gallery_from = "examples"

[[extra.sections]]
title = "Start"

[[extra.sections.rows]]
title = "Read the guide"
path = "@/guide/_index.md"

[[extra.sections.rows]]
title = "Browse examples"
path = "@/examples/_index.md"

[[extra.sections.rows]]
title = "View source"
url = "https://github.com/mizzy/peitho"
+++
