# Make preview's three editors discoverable (#610)

Date: 2026-09-22. Reported by the author while using the feature: nothing on
screen says that `e` opens the Markdown editor, that clicking text edits it,
or that the notes panel is editable.

Preview has three editors and advertises none of them. The key hint added in
#605 appears only *after* the whole-slide editor opens, so it helps you leave,
not arrive.

## Where it goes

The notes panel's position row already owns neutral guidance: one element,
`[data-peitho-preview="source-hint"]`, whose text is **derived** inside
`renderPanelStatus` (added #605, extended #604). After #604, the derivation
first asks `restorableDraft()` whether an offer can stand, then evaluates this
priority chain:

1. a discarded draft is restorable → the restore offer
2. a source edit is open and there is no save failure → the save-key hint
3. otherwise → empty

This task extends that same expression with a lowest-priority affordance when
there is no active editor and no save failure. A save failure or an open inline
editor therefore still produces an empty hint, while the alerting status span
or inline editor owns the row. The text still has one element and one
derivation method; `renderNotes` also calls `renderPanelStatus` so that method
re-evaluates the current slide's source availability after navigation. This is
not a per-call-site `showAffordance()` decision: every call renders the whole
priority chain from state.

`destroy` sets a `destroyed` field before it closes the active editor. The
field's top-priority empty branch prevents the cleanup's status renders from
painting the affordance back onto a torn-down shell. The resulting total chain
is:

1. the shell is destroyed → empty
2. a discarded draft is restorable → the restore offer
3. a source edit is open and there is no save failure → the save-key hint
4. no editor is open and there is no save failure → the edit affordance
5. otherwise → empty

## Behaviour

- In single mode, with no editor open, no failure showing, and no restore
  offer standing, the row names how to edit. Everything else already
  supersedes it by sitting higher in the chain.
- Grid mode hides the notes panel entirely, so nothing is needed there.
- A slide whose body is in `SlideSources.unavailable` (e.g. a lone-CR deck)
  must NOT advertise `e` — pressing it would only produce a refusal. Fall back
  to the affordances that still work on that slide: `Click text to edit · notes
  below`.

## Copy

The final normal-slide copy names all three entry points and stays on one line
beside `N / total`:

    Click text to edit · e for Markdown · notes below

The unavailable-slide form drops only the entry point that cannot work:

    Click text to edit · notes below

## Author decision

The affordance stays permanently. Fading after an editor has been used would
need per-viewer memory with nowhere honest to live: source and inline edit text
deliberately never enter `sessionStorage`. Permanent also matches how the
position line behaves.

## Tasks

1. Extend the derived chain in `renderPanelStatus` with the lowest-priority
   affordance, without adding another element or derivation method.
2. Suppress the `e` clause for the current slide when its `sourceKey` is in
   `unavailable`, and call `renderPanelStatus` from `renderNotes` so navigation
   re-derives that slide-specific copy.
3. Add the `destroyed` field and top-priority empty branch before teardown can
   close an editor and render status into the torn-down shell.
4. Tests: the affordance shows in single mode with nothing else active; it is
   replaced (not joined) by the restore offer, the save-key hint, and a save
   failure according to the derived chain; grid mode's panel remains hidden; a
   middle slide in `unavailable` changes the exact copy across navigation; the
   alerting status span and panel styling stay neutral for the affordance.
5. Author-run real-Chrome check at full width and ~1000px: the row does not
   wrap and does not collide with the position.

## Gates

The CLAUDE.md gate list, including the three bundle drift checks.
