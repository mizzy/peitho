# Presenter redesign follow-up fixes (2026-07-04)

Follow-up fixes requested by the user against the presenter redesign landed in PR #79, taken in over this session. More items are expected to be added, so this plan grows item by item.

## Fix 1: Space key triggers timer Start/Pause/Resume

**Instruction**: Start and Pause should fire not only on click but also on the Space key. The original design (Claude Design mock `Presenter.dc.html`) works this way, so bring the hint label into line.

At PR #79 the call was "keybindings unchanged (Space=next), hint labels track actual behavior," but that has been overridden by a user decision. Scope is the presenter window only; Space=next on the slides side (present index) does not change.

### Behavior

- The Space key in the presenter window dispatches `peitho:timercontrol` via the same code path as the Play button click (stopped → `start` / running → `pause` / paused → `resume`)
- `event.preventDefault()` prevents a double fire alongside the native click when the Play button has focus
- `event.repeat` (long press) does not toggle
- Navigation on ←/→/PageUp/PageDown/Home/End is unchanged
- §16 event contract is unchanged: the keyboard layer only dispatches request events; only the shell performs state transitions

### Changes

- `packages/peitho-present/src/keyboard.ts` — factor out the base map of navigation keys and add `installPresenterKeyboard(win, bus, onPlaypause)`. Existing `installKeyboardNavigation` keeps its API and behavior (embedded entry points call it, so backward compatibility is required)
- `packages/peitho-present/src/presenter.ts` — swap keyboard installation to `installPresenterKeyboard`. Change the kbdbar label to "`Space` start / pause" and add a `<span class="k">Space</span>` hint on the Play button (per the mock)
- `crates/peitho-core/src/render.rs` — add the three rules that the mock had but were dropped when hints were absent: `.btn.primary .k` / `.btn.primary:active .k` / the paused-state Play button `.k`
- Tests: flip the "Space hint absent" assertion in presenter.test.ts and add tests for Space → timercontrol on each transition, repeat suppression, and non-firing of navigate. On the render.rs side, assert on the presence of the added CSS

### Verification

In addition to the standard gates (cargo test x3 / clippy / fmt / npm build + test + typecheck / shell.js drift), verify the Start → Pause → Resume Space-key transitions and the hint label with real-browser screenshots.

**Result**: Merged as PR #80 (merge commit `f1692d2`, 2026-07-04).

## Fix 2: Stabilize the size of the speaker notes panel and align it with the slide width (`.stage` approach)

**Evolution of the instruction**:
1. "The notes panel is vertically narrower than the original design. Give it a fixed height even when empty" → first implemented with `min-height: 42vh; max-height: 42vh` fixed (initial PR #81)
2. Rejected as "too big." Reading the target screenshot (the original mock's preview): notes height is roughly 24% of screen height, and **the left/right edges of the notes, kbdbar, and header row are aligned with the left/right edges of the 16:9 slide** — this was added as a requirement

### Design

Wrap the left-column contents (colhead / slide-frame / kbdbar / notes) in a single vertical flex column `.stage`, and set **the stage width itself to "the 16:9 width back-solved from available height"** `max(280px, min(100%, calc((100cqh − colhead − kbdbar − notes baseline − gap×3) × 16 / 9)))`. The slide pane becomes `width: 100%` + `aspect-ratio: 16/9`, so the widths of every element line up structurally and the edges are always aligned (no JS layout syncing).

- notes are `flex: 1 0 24vh; max-height: 42vh`. Under height constraint (wide) they are exactly 24vh; under width constraint (tall) they stretch to the bottom edge like the mock and cap at 42vh. **Content quantity is irrelevant** (same height when empty, overflow scrolls inside the body)
- colhead/kbdbar are single-line bars, so `--colhead-h: 18px` / `--kbdbar-h: 22px` fixed heights. They use the same variables as the stage-width calc so they cannot drift
- The container query width on `.slide-pane` (`min(100cqw, calc(100cqh * 16/9))`) is retired; the container moves to `.left`

### Verification

In addition to the standard Rust/TS gates, verify with JS measurements and screenshots in a real browser that (1) the left/right edges of the slide pane and notes/kbdbar align within ±1px, (2) height is identical whether notes are empty or long, and (3) notes stretch to the bottom edge in a narrow window (width-constrained path).
