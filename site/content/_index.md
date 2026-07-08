+++
title = "peitho"
template = "index.html"

[extra.hero]
lead = "An HTML-native presentation tool that treats Markdown as the source of truth."

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
