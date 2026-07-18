+++
title = "peitho"
template = "index.html"

[extra.hero]
lead = "An HTML-native presentation tool that treats Markdown as the source of truth."
tagline = "The layout is the schema. Broken decks fail at build, not on stage."

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
shot = "presenter-view"
title = "Presenter view"
meta = "Current and next slide, speaker notes, a timer with slide progress, and a per-section agenda."
alt = "Presenter view with current and next slides, notes, a timer, and a per-section agenda"
path = "@/guide/getting-started.md"
anchor = "present"

[[extra.sections.shots]]
shot = "preview-overview"
title = "Preview overview"
meta = "peitho preview rebuilds on every save; press o for a tile overview of the whole deck."
alt = "The overview view: every slide as a tile in a scrollable grid"
path = "@/guide/getting-started.md"
anchor = "preview-while-editing"

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
