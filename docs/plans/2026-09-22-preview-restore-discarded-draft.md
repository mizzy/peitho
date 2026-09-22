# Restore an Escape-discarded preview draft once (#604)

Date: 2026-09-22. Author decision (option B of three): Escape keeps discarding
immediately; the discarded text becomes restorable with one key instead of
being guarded by a confirmation.

Rejected in the issue and not to be revisited here: a confirmation step on a
dirty Escape. It breaks the keyboard-only loop, has no good answer for "what
does Escape do inside the confirmation", has no precedent in this shell (every
failure is text in the status line), and `CLAUDE.md` forbids `alert`/`confirm`
outright.

## Scope

Both preview editors, one mechanism:

- the whole-slide source editor (`e`, `previewSourceEdit.ts`)
- the inline block editor (click, `ActiveEdit::inline` in `preview.ts`)

## Behaviour

- Escape closes the editor immediately, exactly as today. Nothing about the
  cancel path changes.
- When the discarded text differed from what the server last had, the shell
  keeps it in memory and shows an offer in the notes panel. The offer names
  the key and is not an error: it rides the same neutral element the source
  key hint uses (`[data-peitho-preview="source-hint"]`, added in #605), never
  `setPanelStatus`, which styles every message as a failure.
- Pressing `u` while the offer stands reopens that editor with the discarded
  text and clears the offer.
- The offer is dropped, silently, by anything else that happens: opening any
  editor, a slide change or mode change, a successful save of the edit the
  draft came from (which necessarily opens/closes an editor), a generation
  reload, or shell teardown. An unrelated notes save deliberately leaves the
  offer standing, because it tells you nothing about the discarded draft.
- An unchanged (clean) Escape offers nothing — there is nothing to restore.

## Constraints

- **Never persisted.** The retained draft stays in memory for the current
  page. Source and inline edit text deliberately never enter `sessionStorage`
  (`slide_edits_are_never_written_to_session_storage` pins this), and a
  restorable draft must not create a back door into it. A generation reload
  drops the offer with everything else.
- **One offer at a time**, like the one-edit-at-a-time rule it mirrors. A
  second discard replaces the first.
- **`u` must not steal a keystroke.** It is only special while an offer stands
  AND no editor is open AND the target is not editable (the keyboard installer
  already treats textarea/input/select/contenteditable via `composedPath()[0]`
  as editable, so typing `u` in the notes textarea is unaffected). Chord
  modifiers are ignored via the shared `hasChordModifier` guard, like every
  other shortcut.
- §16: the keyboard component only emits a request event; the shell performs
  the restore. No transition logic moves into the key handler.
- Restoring an inline edit must land on the same block. If the slide changed
  underneath (a generation reload happened), there is no offer left to accept,
  so this cannot resurrect a stale range.

## Tasks

1. Retain the discarded text at the cancel seam. Source: the active edit needs
   a snapshot of the server key and body taken when the editor opens, rather
   than reading the view's key later, because a successful save can change that
   key. Inline: `cancelSlideEdit` has `edit.old` and the editor's current text.
   Dirty is derived by comparison, never stored as a flag, matching the
   existing rule.
2. Add the offer element's copy and a `peitho:restorerequest` event; the
   preview keyboard installer emits it for an unmodified `u` on a
   non-editable target. Shell handles it by reopening the right editor kind
   with the retained text.
3. Clear the offer in the one place that already knows about every relevant
   transition rather than at each call site, mirroring how #605 derives the
   key hint inside `setPanelStatus` — a per-site `clearOffer()` is the
   "callers must remember to" shape `CLAUDE.md` warns against.
4. Tests: offer appears only for a dirty Escape (both editors); `u` restores
   the text and clears the offer; `u` with no offer does nothing; `u` while
   typing in the notes textarea inserts a `u`; Cmd+U is ignored; opening an
   editor / changing slides / saving the edit the draft came from / generation
   reload each drop the offer; an unrelated notes save leaves it standing; the
   retained draft never reaches `sessionStorage`.
5. Document the shared one-shot restore behaviour and the still-distinct notes
   Escape behaviour in the preview-editing guide.
6. Real-Chrome check: discard a real edit, see the offer, press `u`, confirm
   the text comes back and saves. jsdom cannot show the panel layout.

## Gates

The CLAUDE.md gate list, including the three bundle drift checks.
