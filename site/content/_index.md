+++
title = "peitho"
template = "index.html"

[extra.hero]
lead = "An HTML-native presentation tool that treats Markdown as the source of truth."
tagline = "Layouts as types. Broken decks fail at build, not on stage."

[[extra.sections]]
title = "Pillars"

[[extra.sections.rows]]
title = "Separation of content and design"
meta = "Markdown owns the content; HTML and CSS own the layout and theme. Neither leaks into the other."

[[extra.sections.rows]]
title = "Git-manageable HTML/CSS layouts"
meta = "Layouts and CSS are normal files that diff and review cleanly. The layout itself is the schema (<slot name accepts arity>)."

[[extra.sections.rows]]
title = "Type-checked slot contracts"
meta = "Slot excess and deficiency, type mismatches, broken references, and unassigned content are all build errors with line numbers and help."

[[extra.sections]]
title = "Install"
code = "brew install mizzy/tap/peitho"

[[extra.sections]]
title = "Examples"
gallery_intro = "Same tool, same Markdown conventions — different decks."

[[extra.sections.gallery]]
path = "@/examples/peitho-tour.md"
title = "Peitho Tour"
image = "/deck-shots/peitho-tour.png"

[[extra.sections.gallery]]
path = "@/examples/minimal.md"
title = "Minimal"
image = "/deck-shots/minimal.png"

[[extra.sections.gallery]]
path = "@/examples/lightning-talk.md"
title = "Lightning Talk"
image = "/deck-shots/lightning-talk.png"

[[extra.sections.gallery]]
path = "@/examples/keynote.md"
title = "Keynote"
image = "/deck-shots/keynote.png"

[[extra.sections.gallery]]
path = "@/examples/code-walkthrough.md"
title = "Code Walkthrough"
image = "/deck-shots/code-walkthrough.png"

[[extra.sections.gallery]]
path = "@/examples/two-column.md"
title = "Two Column"
image = "/deck-shots/two-column.png"

[[extra.sections.gallery]]
path = "@/examples/image-showcase.md"
title = "Image Showcase"
image = "/deck-shots/image-showcase.png"

[[extra.sections.gallery]]
path = "@/examples/aspect-ratio-4-3.md"
title = "Aspect Ratio 4:3"
image = "/deck-shots/aspect-ratio-4-3.png"

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
