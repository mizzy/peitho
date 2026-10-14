# Draft, skip, and present-only example

A deck mixing normal, skipped, draft, and present-only slides. This example is
not part of the demo-site gallery on purpose: draft and present-only slides are
dropped at build and skip only shows up in presenter navigation, so the behavior
has to be observed locally.

## Build: the draft and present-only slides disappear

```sh
cargo run -p peitho -- build examples/draft-skip/deck.md --out /tmp/peitho-draft-skip
```

The deck has five source slides but the output says `built 3 slide(s)` — the
`{"draft":true}` slide is dropped at parse end and appears in no output at all,
and the `{"present_only":true}` slide is dropped from every shareable output
(build, publish, PDF export, lint).
The `{"skip":true}` slide is a real slide: it is in `slides/`, and
`manifest.json` carries `"skip": true` for it.

## Present or preview: navigation steps over the skipped slide

```sh
cargo run -p peitho -- present examples/draft-skip/deck.md
```

`next`/`prev` (arrow keys, space) resolve to the next non-skipped slide, and
present opens on the first non-skipped slide. Direct navigation — number keys,
Home/End, the preview grid — can still land on the skipped slide, and it keeps
its speaker notes when you get there. PDF export and publish include it.

The present-only slide is a normal slide here: present and preview show it,
count it, and keep its speaker notes.

## Invalid combinations are build errors

`draft` + `skip` on one slide, `draft` + a `section` marker, `present_only` +
`draft` or a `section` marker, and marking every slide draft (or, for build,
every non-draft slide present-only) are all line-numbered build errors. For example:

```
× slide 1 ('slide-1'), line 1: slide cannot be both draft and skipped
  = help: remove "skip":true because draft slides are excluded from the build
```
