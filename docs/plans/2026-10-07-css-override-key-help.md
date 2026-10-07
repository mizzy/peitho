# Issue 694: Make unknown CSS override keys actionable

## Problem

Unknown `data-slide-key` selectors currently suggest every surviving slide key,
including unstable derived keys, and always mention drafts. The help also fails
to identify the stylesheet rule that should be removed or distinguish selectors
that can never be valid slide keys.

## Design

Partition parsed slides once into drafts and survivors. Record drafted keys and
the explicitly authored keys of surviving slides privately on `DeckSettings`,
then carry those typed sets into a public `OverrideKeys` value alongside slot
classes. Its standalone constructor is named `without_deck` to make the absence
of those hints visible.

For an unknown selector, explain invalid key syntax first, otherwise identify a
matching draft or suggest adding a `"key"` property to the slide's existing page
settings comment. Every fix names the originating stylesheet, and only sorted,
explicitly authored keys may appear as examples.

## Tests

Start with failing parser and theme tests covering drafted-key retention,
surviving explicit-key retention, draft-only help, invalid key syntax, exclusion
of derived keys, sorted explicit keys, page-settings wording, and per-stylesheet
names. Then update callers and run the Rust test, lint, formatting, and
bindings-drift gates.
