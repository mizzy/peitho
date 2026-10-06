# Unify No-Slot Errors (Issue #686)

## Problem

Missing conventional slots and missing image-accepting slots construct equivalent errors in different modules, with different messages and error kinds. This allows their user-facing diagnostics to diverge.

## Design

Add one private `no_slot_accepts` builder in `check.rs` that owns the `ResidualContent` kind, source line, and shared message. Expose two crate-visible entry points: `unassigned_fragment_error` consumes the existing slot-and-fragment pair, while `no_image_slot_error` supplies the fixed image remedy.

Pass the layout into the unassigned-fragment check and derive its help from the paired expected slot. Pass the full image fragment to image-slot selection and use the image-specific entry point when no image slot exists. Keep multiple image slots as a layout error and keep image rejection at mapping time.

## Tests

Cover an unassigned code block and an image without an image slot with exact assertions for the message, help, line, and kind. Update dispatch traces, diagnostics snapshots, CLI output, and guide examples, then run the full workspace test, lint, format, and bindings gates.
