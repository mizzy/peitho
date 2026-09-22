# Name every preview editor's keys (#612)

<!-- derived-from ./2026-09-22-preview-edit-affordances.md -->

Date: 2026-09-22. This follows the source-editor hint from #605, the editor
affordances from #610, and the restore priority from #604. The shared slot was
still incomplete: only the whole-slide source editor named its keys.

The inline editor showed nothing while it was open. Focusing speaker notes
left the idle affordance in place, so the row invited the user to edit “notes
below” while they were already editing them. This change extends the same
single derived slot rather than adding editor-specific chrome.

## Why the keys matter

Preview's three editors deliberately interpret Enter and Escape differently:

- The whole-slide source editor inserts a newline on Enter, saves on
  Cmd/Ctrl+Enter or blur, and cancels on Escape.
- The inline editor saves on plain Enter or blur, inserts a newline on
  Shift+Enter, and cancels on Escape.
- The notes textarea inserts a newline on Enter and has no cancel operation.
  Escape leaves the textarea, and that blur saves just like clicking away.

Silence is unsafe when the same key changes meaning between editors. In
particular, clicking away from an inline edit commits it to the Markdown
source; the hint must name that path alongside Enter so a user who means to
discard the change knows to press Escape instead.

## Where it goes

The notes panel's position row continues to own one neutral guidance element,
`[data-peitho-preview="source-hint"]`. `renderPanelStatus` computes its text
through one total priority chain:

1. a destroyed shell → empty
2. any save failure → empty, leaving only the alerting status
3. a restorable discarded draft while focus is outside notes → the restore offer
4. an open source or inline edit → “Saving…” during an in-flight commit;
   otherwise that editor's keys
5. the notes textarea has focus → the notes keys
6. otherwise → the edit affordance for the current slide

Locked slide editors swallow their own keys while saving, so the slot replaces
those promises with the in-flight state. The three write routes share one
deck-writer mutex and their fetches have no timeout, so contention or a large
deck reparse can make that window visibly longer than a sub-second request.

The keyboard installer's editable-target guard makes `u` unreachable in the
notes textarea, so naming the restore offer there would advertise a key the
user cannot act on. Focusing notes does not clear the offer: its text yields to
the notes hint, then blur re-renders the restore hint. The `combined !== ""`
rung silences the editor and affordance rungs whenever a save failure exists.
Separately, `restorableDraft()` returns null while any panel status exists, so
the restore check above it in source order cannot preempt the failure. The
explicit comment above that editor boundary keeps this ordering visible during
later edits.

## Copy

The six guidance strings are exact. The source editor keeps the existing
copy:

    Cmd/Ctrl+Enter or click away saves · Enter inserts a newline · Esc cancels

The inline editor names both of its save paths:

    Enter or click away saves · Shift+Enter inserts a newline · Esc cancels

The notes editor makes clear that Escape saves rather than cancels:

    Esc or click away saves · Enter inserts a newline

While either slide editor has an in-flight commit, unavailable keys give way
to its saving state:

    Saving…

With no editor open, an editable source uses the full affordance:

    Click text to edit · e for Markdown · Enter for notes

When whole-slide Markdown editing is unavailable, only that entry point is
removed:

    Click text to edit · Enter for notes

Every available entry point is named by its key or action, keeping the
keyboard-only loop discoverable: Enter reaches notes, `e` reaches the Markdown
editor, and a click reaches an inline block.

## State and exhaustiveness

Notes focus is read live as
`document.activeElement === this.notesTextarea` inside the derivation. It does
not widen `ActiveEdit`, which is deliberately the union of slide edits, and it
does not add a mirrored boolean that could disagree with the DOM. Focus and
blur both re-render the slot; teardown removes the new focus listener.

`activeEditHint` switches exhaustively over the closed `ActiveEdit` union with
an explicit `string` return type and no default branch. A temporary third
variant proved the guard: TypeScript emitted TS2366, “Function lacks ending
return statement and return type does not include 'undefined'.” A future slide
editor must therefore add its own hint at compile time instead of silently
blanking the slot.

## Browser check

Measured in a real browser: the position row is 1196px wide with the position
text occupying 36px. The updated inline hint measures 448px; the pre-existing
source hint remains the longest at 476px, so the worst case does not move.
Nothing wraps, and the row height remains 20px, so no layout change or shorter
copy is needed.

## Tasks

1. Add exact inline and notes key hints, including inline click-away saving.
2. Derive notes focus from the live DOM, re-render on focus and blur, and
   remove the focus listener during teardown.
3. Replace the nested hint expression with a total priority method, a separate
   affordance helper, and an exhaustive active-editor switch.
4. Pin every state and priority with exact Vitest assertions, including save
   failure styling, restore precedence, source availability, and teardown.
5. Rebuild the committed preview bundle and update the CLI guide.

## Gates

Build, Vitest, TypeScript, the Rust workspace, bundle drift, and a Zola parse
of the updated guide.
