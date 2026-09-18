# Rehearsal audio and per-slide timeline

Date: 2026-09-18
Branch: `rehearsal-audio`

<!-- derived-from ../specs/2026-09-18-rehearsal-audio-design.md -->
<!-- constrained-by ./2026-07-19-rehearsal-mode.md -->

## Goal and PR boundary

Make rehearsals reviewable per slide without weakening the existing absolute-snapshot model.

- **M1 / PR 1 — timeline:** v2 snapshot and record types, v1 record compatibility, an always-on per-slide timeline under `--rehearsal`, server validation against the built deck, and a terminal per-slide table. M1 has no audio flag, microphone access, audio endpoint, or `present.json` change and must ship independently.
- **M2 / PR 2 — audio:** opt-in `--audio`, explicit presenter privacy intent, timer-slaved `MediaRecorder`, serialized retries, server-enforced sequence order and size, a same-stem WebM, visible status/errors, and terminal audio output.

Land M1 before starting M2. The [design](../specs/2026-09-18-rehearsal-audio-design.md) extends the [original rehearsal plan](./2026-07-19-rehearsal-mode.md).

## File map

| File | Planned responsibility |
| --- | --- |
| `crates/peitho-core/src/rehearsal.rs`, `src/lib.rs`, `bindings/Rehearsal*.ts` | Distinct v1/v2 Rust records, v2 snapshot, slide entries, M2 audio field, and generated TS contracts. |
| `crates/peitho/src/server.rs` | Deck-aware timeline validation; session reservation; snapshot persistence; M2 audio policy, ordered append, size cap, and route. |
| `crates/peitho/src/main.rs` | Pass final slide keys to the sink; render v1/v2 review; add `--audio`, `present.json` input, startup text, and contamination tests. |
| `crates/peitho-core/src/present_config.rs`, `bindings/PresentConfig.ts`, `crates/peitho-core/src/render.rs` | In M2, carry `rehearsalAudio` into the generated presenter entry and style its indicator. |
| `packages/peitho-present/src/slideTimeline.ts` | New M1 absolute timeline accumulator next to, but separate from, `sectionActuals.ts`. |
| `packages/peitho-present/src/rehearsalReporter.ts`, `rehearsalBridge.ts` | Emit v2 full snapshots, including on start; retain the existing JSON event/POST boundary. |
| `packages/peitho-present/src/rehearsalAudio.ts` | New M2 recorder state machine and one serialized retry queue. |
| `packages/peitho-present/src/presenter.ts`, `src/index.ts` | Own one timeline accumulator; in M2 wire audio only when explicitly requested and render status. |
| `packages/peitho-present/src/sectionActuals.ts`, `timeTracker.ts` | Semantic references only: keep section measurement and auto-start ownership unchanged. |
| `packages/peitho-present/test/{slideTimeline,rehearsalReporter,rehearsalBridge,rehearsalAudio,presenter}.test.ts` | Client unit/integration coverage with listener cleanup. |
| `packages/peitho-present/dist/shell.js` | Regenerated embedded runtime code. Rehearsal JSON/WebM user data still never enters a deck's `dist/`. |
| `CLAUDE.md`, `README.md`, `site/content/guide/cli.md` | M1 timeline documentation, then M2 audio usage and measured playback guidance. |

## Load-bearing shape

Use separate payloads behind an enum:

```rust
pub enum RehearsalRecord {
    V1(RehearsalRecordV1),
    V2(RehearsalRecordV2),
}
```

Keep common accessors (`recorded_at_ms`, `elapsed_ms`, `sections`) on the enum, but no enum-wide `timeline()` or `audio()`: callers must match `V2` before v2-only data exists in their type. Preserve flat JSON with an untagged serialization and numeric versions by using private exact version markers plus custom enum deserialization that probes `version` once and dispatches to `deny_unknown_fields` payloads. Unknown versions remain named hard errors; ts-rs emits literal-version v1/v2 arms. `RehearsalSnapshot` is v2-only; compatibility applies to durable v1 files, not stale POST clients.

Snapshots stay absolute. The presenter reports regardless of mode; only `RehearsalSink` authorizes persistence. Build its ordered key vector from `artifacts.rendered.slides()`, whose post-draft indices/keys are the manifest authority. Reset publishes zero section actuals, `elapsedMs == 0`, and an empty timeline.

In M2, add `audio: Option<String>` only to v2 with `#[serde(default)]`, so M1 v2 files lacking the field read as `None`. The sink owns the current `(take, next_seq)`, conflict acknowledgements, and reset deletion. Replace the filename parser's raw tuple with a typed `{stamp, suffix}` identity: `parse_rehearsal_filename` stays the sole string parser and accepts only `.json`; the reserved session identity formats both `.json` and `.webm` from one stem.

## Milestone 1 — v2 per-slide timeline

### Task M1.1: Define v2 and preserve v1 at the type boundary

**Goal.** Add the v2 contract and generated bindings before changing producers.

**Files.**

- `crates/peitho-core/src/rehearsal.rs`
- `crates/peitho-core/src/lib.rs`
- `bindings/RehearsalRecord.ts`
- `bindings/RehearsalRecordV1.ts`
- `bindings/RehearsalRecordV2.ts`
- `bindings/RehearsalSlideEntry.ts`
- `bindings/RehearsalSnapshot.ts`

**Test.** Add golden serialization, strict decoding, validation, and binding tests. The core compatibility example is:

```rust
let v1: RehearsalRecord = serde_json::from_str(
    r#"{"version":1,"recordedAtMs":10,"elapsedMs":9000,
         "sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":9000}]}"#,
).unwrap();
assert!(matches!(v1, RehearsalRecord::V1(_)));

let v2: RehearsalRecord = serde_json::from_str(
    r#"{"version":2,"recordedAtMs":10,"elapsedMs":9000,
         "sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":9000}],
         "timeline":[{"key":"intro","index":0,"atMs":0}]}"#,
).unwrap();
let RehearsalRecord::V2(v2) = v2 else { panic!("expected v2") };
assert_eq!(v2.timeline()[0].key().as_str(), "intro");
```

Also prove: v1 pretty JSON is byte-identical to today's output; v1 snapshots fail; version 3 reports `unsupported rehearsal version 3`; unknown fields and empty sections fail; generated `RehearsalRecord` is a v1/v2 union and only its v2 arm carries `RehearsalSlideEntry`.

**Implementation.** Add `RehearsalSlideEntry { key: SlideKey, index: u32, at_ms: u64 }`, `RehearsalRecordV1`, `RehearsalRecordV2`, and the enum. Move legacy fixture construction to an explicit `RehearsalRecordV1::new`; make `RehearsalRecordV2::from_snapshot` the server write path, with no version-taking constructor. Give exact version markers TS overrides of `1`/`2`; serialize the enum untagged and implement version-dispatched deserialization. Add a deck-independent timeline-shape validator (non-decreasing and not after `elapsedMs`). The sink calls it so POST violations remain 422; record validation calls it so corrupt files fail review; `RehearsalSnapshot::validate()` must not turn those sink errors into 400. Export all generated dependencies through the existing ts-rs test; never edit bindings by hand.

**Verification.**

```sh
cargo test -p peitho-core rehearsal::tests
```

### Task M1.2: Accumulate and report one absolute timeline

**Goal.** Record slide entries once, include the complete list in every v2 report, and create the session immediately on timer start.

**Files.**

- `packages/peitho-present/src/slideTimeline.ts`
- `packages/peitho-present/src/rehearsalReporter.ts`
- `packages/peitho-present/src/presenter.ts`
- `packages/peitho-present/src/index.ts`
- `packages/peitho-present/test/slideTimeline.test.ts`
- `packages/peitho-present/test/rehearsalReporter.test.ts`
- `packages/peitho-present/test/rehearsalBridge.test.ts`
- `packages/peitho-present/test/presenter-agenda.test.ts`
- `packages/peitho-present/test/presenter.test.ts`

**Test.** Drive a fake shell through start, the same start slidechange, navigation, revisit, reveal, reset, and adopted reset:

```ts
expect(timeline.entries()).toEqual([
  { key: "intro", index: 0, atMs: 0 },
  { key: "details", index: 1, atMs: 7_000 },
  { key: "intro", index: 0, atMs: 12_000 }
]);

bus.dispatchEvent(new CustomEvent("peitho:timercontrol",
  { detail: { action: "start" } }));
expect(reports[0]).toEqual({
  version: 2,
  elapsedMs: 0,
  sections: [{ name: "Setup", plannedDurationMs: 60_000, actualMs: 0 }],
  timeline: [{ key: "intro", index: 0, atMs: 0 }]
});

adoptedBus.dispatchEvent(new CustomEvent("peitho:timeradopt", {
  detail: { running: true, previousElapsedMs: 0, elapsedMs: 7_000 }
}));
expect(adoptedTimeline.entries()).toEqual([
  { key: "details", index: 1, atMs: 7_000 }
]);
```

Assert reveal steps add nothing; exact consecutive `(key,index,atMs)` duplicates coalesce; a genuine revisit appends; a positive `timeradopt` into an empty run seeds the current slide at the adopted elapsed time and reports immediately; adopted pause/resume adds no entry; reset reports an empty absolute timeline; `entries()` returns a copy; destroy removes listeners. Retain cadence, slidechange, pause, adopted reset, close, integer rounding, flush-before-report, sectionless no-op, bridge `keepalive`, bridge error logging, and presenter teardown tests.

**Implementation.** Add `installSlideTimeline({shell,bus,log}) -> {entries,destroy}`. On `peitho:presentationstart`, append the current manifest slide at `0`; a valid positive `timeradopt` seeds an empty list at rounded `detail.elapsedMs`; while started, append validated slidechanges at rounded non-negative `elapsedMs`; ignore stepchange; clear on local/adopted reset. `packages/peitho-present/src/sectionActuals.ts` adds only `previousElapsedMs - lastElapsedMs` to the current section before moving its baseline to `elapsedMs`, so it does not attribute the unknown adopted gap and the timeline likewise starts at the adopted `elapsedMs`. Coalesce only an exact last-entry duplicate because `timeTracker.ts` dispatches nested start during the same forward slidechange. In the reporter, set version 2, copy `timeline.entries()`, and call `report()` on local start or the first positive adoption (ordinary resume remains unchanged). In the presenter install `sectionActuals`, then `slideTimeline`, then display consumers/reporter; destroy consumers before both accumulators. `rehearsalBridge.ts` remains the JSON transport and persistence stays out of client state.

**Verification.**

```sh
cd packages/peitho-present && npm test -- test/slideTimeline.test.ts test/rehearsalReporter.test.ts test/rehearsalBridge.test.ts test/presenter-agenda.test.ts test/presenter.test.ts
```

### Task M1.3: Validate timelines in the server sink

**Goal.** Reject malformed or stale-window timelines before any session write.

**Files.**

- `crates/peitho/src/server.rs`
- `crates/peitho/src/main.rs`

**Test.** Create a sink for keys `intro, details` and post:

```rust
let valid = r#"{"version":2,"elapsedMs":2000,
  "sections":[{"name":"Setup","plannedDurationMs":60000,"actualMs":2000}],
  "timeline":[{"key":"intro","index":0,"atMs":0},
              {"key":"details","index":1,"atMs":1000}]}"#;
assert_eq!(rehearsal_post_outcome(Some(&sink), valid).status, 200);
```

Add exact 422 cases by appending `intro@500` (decreasing), changing `atMs` to `2001`, changing index to `2`, and changing index 1's key to `intro`. Every rejection must leave bytes unchanged. Also test empty reset timeline, v1 POST -> 400, v2 without a sink -> `{"recorded":false}`, stable `recordedAtMs`, `/sync`, and HTTP method behavior.

**Implementation.** Extend `RehearsalSink::new` with ordered `Vec<SlideKey>`. In one sink-owned pass require non-decreasing `atMs`, `atMs <= elapsedMs`, lossless `u32 -> usize`, in-range index, and exact key at that index; map violations to specific 422 text, schema errors to 400, and I/O to 500. Serialize validation/write under the session mutex. Add `expected_rehearsal_slide_keys(&BuildArtifacts)` from `rendered.slides()` and pass it only to rehearsal sinks.

**Verification.**

```sh
cargo test -p peitho rehearsal_server
cargo test -p peitho rehearsal_route
```

### Task M1.4: Print v2 slide totals while v1 stays byte-identical

**Goal.** Add terminal slide review without loading the current deck.

**Files.**

- `crates/peitho/src/main.rs`

**Test.** Keep the current v1 golden unchanged. For `intro@0`, `details@10000`, `intro@25000`, `elapsedMs=30000`, assert:

```text
  slide   first   total
  #1       0:00    0:15
  #2       0:10    0:15
```

Also test same-timestamp zero duration, a reset record's `  (no slide entries)`, hard errors with paths for decreasing/after-elapsed files, mixed v1/v2 `--all` order, numeric filename suffix order, and `.webm` exclusion.

**Implementation.** Return `RehearsalRecord` from the reader and validate its selected payload. Print the existing section table through common accessors. Only for `V2`, pair each entry with the next `atMs` (the last with `elapsedMs`), aggregate by recorded `(index,key)` in first-entry order, and show 1-based index, first entry, and accumulated duration. Return a diagnostic rather than saturating/panicking if validated subtraction fails. Do not print mutable deck titles or re-read the deck.

**Verification.**

```sh
cargo test -p peitho rehearsal_command
cargo test -p peitho latest_rehearsal_record
```

### Task M1.5: Lock distribution boundaries, document M1, and clear PR 1 gates

**Goal.** Make timeline-only M1 shippable with current generated artifacts and documentation.

**Files.**

- `crates/peitho/src/main.rs` (test module)
- `packages/peitho-present/dist/shell.js`
- `CLAUDE.md`
- `README.md`
- `site/content/guide/cli.md`

**Test.** Add a build-distribution regression using a recursive test helper built with `std::fs::read_dir`:

```rust
emit_distribution(&out, &artifacts).unwrap();
assert!(!out.join(".peitho").exists());
assert!(!out.join("present.json").exists());
let names = recursively_list_file_names(&out);
assert!(!names.iter().any(|name|
    name.starts_with("rehearsal-") || name.ends_with(".webm")));
```

**Implementation.** Regenerate the committed shell. Update the CLAUDE rehearsal bullet, README commands, and the existing site CLI guide for v2 absolute timelines, revisits, the slide table, and v1 readability. Do not mention unavailable audio behavior in M1; leave the historical plan unchanged.

**Verification.** Run every CLAUDE gate, including three test passes, and the edited site build:

```sh
cargo test --workspace
cargo test --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
(cd packages/peitho-present && npm run build && npm test && npm run typecheck)
git diff --exit-code packages/peitho-present/dist/shell.js
git diff --exit-code packages/peitho-present/dist/preview.js
git diff --exit-code packages/peitho-present/dist/remote.js
make demo-site
git diff --check
```

Before PR 1 merges, the implementer must use real Chrome with a fixed port to start, advance, revisit, pause/resume, and reset; compare the v2 JSON/CLI totals with observed timer positions; review a v1 fixture; and confirm plain present/build emits no rehearsal artifact. This is not part of this plan-writing run.

## Milestone 2 — opt-in timer-aligned audio

### Task M2.1: Carry explicit audio intent to presenter configuration

**Goal.** Make microphone intent explicit, Rust-owned, and invalid without a rehearsal presenter.

**Files.**

- `crates/peitho/src/main.rs`
- `crates/peitho-core/src/present_config.rs`
- `crates/peitho-core/src/render.rs`
- `bindings/PresentConfig.ts`
- `packages/peitho-present/src/presenter.ts`
- `packages/peitho-present/test/generated.test.ts`

**Test.** Add clap/config/cache/generated-entry tests:

```rust
let err = Cli::try_parse_from(["peitho", "present", "deck.md", "--audio"])
    .unwrap_err();
assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
assert!(err.to_string().contains("--rehearsal"));

assert!(present_config_json(&PresentConfig::new(true, true)).unwrap()
    .contains("\"rehearsalAudio\": true"));
```

Also prove `--audio --no-presenter` is a clap conflict; plain/rehearsal-only caches write false; audio writes true; presenter HTML fetches `present.json`, passes the boolean, and shows a stale-shell error when audio is requested but `installRehearsalAudio` is absent.

**Implementation.** Add `audio: bool` with `#[arg(long, requires = "rehearsal", conflicts_with = "no_presenter")]`. Keep `PresentConfig` version 1, add required `rehearsalAudio`, and pass `options.audio` independently of `presenterOpen` so `--no-open` users may open the presenter manually. Fetch config in the presenter entry, treat a missing field as false, pass a required presenter option, and feature-detect the audio export before microphone access. Regenerate `PresentConfig.ts`. Main will pass an explicit enabled/disabled audio policy to the sink in M2.2; client config never authorizes persistence.

**Verification.**

```sh
cargo test -p peitho-core present_config
cargo test -p peitho present_command
cargo test -p peitho emit_present_cache
cargo test -p peitho-core presenter_index
```

### Task M2.2: Persist bounded chunks in server-enforced order

**Goal.** Append only authorized, contiguous chunks to the WebM paired with the existing JSON session.

**Files.**

- `crates/peitho-core/src/rehearsal.rs`
- `bindings/RehearsalRecordV2.ts`
- `bindings/RehearsalRecord.ts`
- `crates/peitho/src/server.rs`
- `crates/peitho/src/main.rs`

**Test.** First parse an M1 v2 fixture without `audio` and assert `None`. Then create a snapshot session and assert:

```rust
assert_eq!(post_audio(&server, "take-a", 0, b"head-a").status, 200);
assert_eq!(post_audio(&server, "take-a", 1, b"-one").status, 200);
let lost_response_retry = post_audio(&server, "take-a", 1, b"-one");
assert_eq!(lost_response_retry.status, 409);
assert_eq!(lost_response_retry.json(), json!({"take":"take-a","nextSeq":2}));

assert_eq!(post_audio(&server, "take-b", 0, b"head-b").status, 200);
let losing_window = post_audio(&server, "take-a", 2, b"-lost");
assert_eq!(losing_window.status, 409);
assert_eq!(losing_window.json(), json!({"take":"take-b","nextSeq":1}));
assert_eq!(fs::read(&audio_path).unwrap(), b"head-b");
assert!(read_v2_record(&json_path).audio().is_some());

post_snapshot(&server, reset_snapshot()).unwrap();
assert!(!audio_path.exists());
assert_eq!(read_v2_record(&json_path).audio(), None);
let stale = post_audio(&server, "take-b", 1, b"-stale");
assert_eq!(stale.status, 409);
assert_eq!(stale.json(), json!({"take":null,"nextSeq":0}));
assert_eq!(post_audio(&server, "take-c", 0, b"head-c").status, 200);
assert_eq!(fs::read(&audio_path).unwrap(), b"head-c");
assert!(read_v2_record(&json_path).audio().is_some());
```

Cover: no session and every ordering conflict return JSON 409 state; missing/repeated/invalid `take` or `seq` -> 400; wrong/missing content type and empty body -> 400; declared or streamed body above 5 MiB -> 413; no sink or disabled policy -> 404; concurrent requests cannot interleave takes; failed append does not advance seq; partial append rolls back; reset removes the WebM and audio field under the lock; same-stem identity and `.webm` record-discovery exclusion remain enforced.

**Implementation.** Add defaulted `audio` only to v2. Introduce `AudioRecording::{Disabled,Enabled}` and pass `Enabled` only for `--audio`. Make reservation return the same typed identity produced by `parse_rehearsal_filename`; its `json_name()` and `webm_name()` methods are the only formatters. Under the existing sink mutex, store that identity, latest v2 snapshot, audio filename, `current_take: Option<(TakeId, next_seq)>`, and a latched audio-write error. The route never reserves a session; every 409 uses one JSON serializer for `{take, nextSeq}` (`take: null`, `nextSeq: 0` when none is current).

A take different from the current one with seq 0 replaces the prior take, truncates the typed sibling WebM, and becomes current; only the current take at exactly `next_seq` appends, and every other chunk gets 409. A retry after a lost response therefore sees the current take and advanced sequence, while a losing window can never interleave. Hold the mutex through write, flush, JSON update, and sequence advancement; roll back partial appends and latch rollback failure. The first accepted chunk sets `audio`, and ordinary snapshot rewrites retain it.

Treat `elapsedMs == 0 && timeline.is_empty()` with a current take as reset: atomically rewrite the record with `audio: None`, delete the WebM, and clear the take within the same mutex hold; a later old-take nonzero chunk gets 409, and only a fresh take's seq 0 recreates the file. Route only `POST /rehearsal/audio?take=<id>&seq=N`, accept `audio/webm` parameters, reject oversized `Content-Length` early, and also read through a `5 * 1024 * 1024 + 1` limiter. `parse_rehearsal_filename` remains the only filename parser.

**Verification.**

```sh
cargo test -p peitho-core rehearsal::tests
cargo test -p peitho rehearsal_audio
```

### Task M2.3: Record, serialize retries, and render every failure

**Goal.** Make recorder state a pure consequence of timer events and ensure chunks are never skipped or reordered by the client.

**Files.**

- `packages/peitho-present/src/rehearsalAudio.ts`
- `packages/peitho-present/src/presenter.ts`
- `packages/peitho-present/src/index.ts`
- `packages/peitho-present/test/rehearsalAudio.test.ts`
- `packages/peitho-present/test/presenter.test.ts`
- `crates/peitho-core/src/render.rs`

**Test.** Use fake media APIs, deferred fetches, and fake timers. Prove recovery after an accepted response is lost:

```ts
queue.beginTake("take-a");
queue.enqueue(new Blob(["A"]), false); // seq 0
first.resolve(ok());
await Promise.resolve();
queue.enqueue(new Blob(["B"]), false); // seq 1
queue.enqueue(new Blob(["C"]), false); // seq 2 waits
second.reject(new TypeError("response lost"));
await vi.advanceTimersByTimeAsync(1_000);
retry.resolve(conflict({ take: "take-a", nextSeq: 2 }));
await Promise.resolve();
expect(lastRequest()).toMatchObject({ take: "take-a", seq: 2, body: "C" });

otherQueue.beginTake("take-a");
otherQueue.enqueue(new Blob(["A"]), false);
otherFirst.resolve(ok());
await Promise.resolve();
otherQueue.enqueue(new Blob(["B"]), false);
other.resolve(conflict({ take: "take-b", nextSeq: 1 }));
await vi.advanceTimersByTimeAsync(1_000);
expect(lastRequest(otherQueue)).toMatchObject({ take: "take-a", seq: 1, body: "B" });
expect(indicator.textContent).toContain("retrying");
```

Also assert one active fetch; only same-take `nextSeq == head.seq + 1` drops a 409 head; different take, other sequence, or malformed 409 keeps it visibly failed; 200 clears upload error; only final/close uses keepalive. Reset drops queued old-take chunks, never retries an active old chunk after it settles, and the next start mints a distinct take at seq 0. For the recorder assert immediate `getUserMedia({audio:true})`, construction with `{mimeType:"audio/webm"}`, `start(5000)`, local/adopted start/pause/resume/reset, start while permission is pending, idempotent close/pagehide, final chunk keepalive, zero-size suppression, track cleanup, MediaRecorder error, permission denial, retry recovery, and no media call/indicator when disabled. Confirm timeline reports continue during microphone failure.

**Implementation.** In one module, separate:

- A FIFO queue keyed by an explicit take ID, with per-take seq, one active fetch, unchanged Blob retries, and a fixed 1,000 ms retry. POST to `/rehearsal/audio?take=<encoded-id>&seq=N`. A valid 409 is proof of persistence only when its take equals the head and `nextSeq == head.seq + 1`; then treat it as success, drop the head, and clear the error, otherwise retain it and publish `audio upload failed: <reason> (retrying)`. Reset advances the client generation, drops queued old-take items, marks an active old item non-retryable, and waits for it to settle before sending the next take. Cleanup cancels retry timers and rejects new enqueue without aborting an active fetch.
- A recorder controller with an injected take-id factory (`crypto.randomUUID` in production) and desired timer state independent of permission readiness. Acquire at install, construct `MediaRecorder` for `audio/webm`, and surface construction failure; reconcile both `timercontrol` and valid `timeradopt` states. Each stopped-to-started transition mints a take at seq 0, pause/resume retains it, and reset stops/discards it so the next start mints a different ID. Wait for `stop` before restarting; map close/pagehide once to a final keepalive chunk; stop all tracks on destroy.

Wire reporter/bridge before audio so the start snapshot is dispatched first. Show one presenter-only element by `textContent`: `… REC` pending, `○ REC` ready, `● REC` recording, `❙❙ REC` paused, `mic unavailable: <reason>` for media failure, or the queue retry message. Put it in the clock row and add compact state/error CSS in `render.rs`. Export `installRehearsalAudio` as the stale-bundle feature marker.

**Verification.**

```sh
cd packages/peitho-present && npm test -- test/rehearsalAudio.test.ts test/presenter.test.ts && npm run typecheck
cargo test -p peitho-core presenter_index
```

### Task M2.4: Print the paired audio path and mode

**Goal.** Make requested/accepted audio visible in terminal workflows.

**Files.**

- `crates/peitho/src/main.rs`

**Test.** Extend the v2 golden with:

```text
  audio   .peitho/rehearsals/rehearsal-20260918-120000.webm
```

Assert v1 and M1-v2-without-audio have no audio line; audio mode prints exactly `recording rehearsal (with audio) to .peitho/rehearsals/`; rehearsal-only retains `recording rehearsal to .peitho/rehearsals/`; output resolves the record's relative audio field beside that record without opening either deck or media. A basename that differs from the selected JSON identity, including a parent path, must be a path-named corrupt-record error.

**Implementation.** Only in the `V2` branch, parse the selected JSON basename with `parse_rehearsal_filename`, require a present audio field to equal that identity's `webm_name()`, then join it to the record's parent; never infer audio presence from a sibling file. Select the startup line from `options.audio`. Keep `run_rehearsal` read-only and every v1 byte unchanged.

**Verification.**

```sh
cargo test -p peitho rehearsal_command
cargo test -p peitho rehearsal_startup
```

### Task M2.5: Document audio, clear PR 2 gates, and measure real Chrome

**Goal.** Finish the contract and resolve the two media properties that jsdom cannot establish.

**Files.**

- `crates/peitho/src/main.rs` (test module)
- `packages/peitho-present/dist/shell.js`
- `CLAUDE.md`
- `README.md`
- `site/content/guide/cli.md`

**Test.** Retain M1's distribution test. Extend CLI tests for flag help/requirements, both startup lines, v2 `audio: null`, same-stem path, and sibling WebM exclusion:

```rust
fs::write(rehearsals.join("rehearsal-20260918-120000.webm"), b"webm").unwrap();
assert_eq!(rehearsal_record_paths_by_name(&rehearsals).unwrap(),
           vec![rehearsals.join("rehearsal-20260918-120000.json")]);
assert!(!review_output.contains("audio")); // the v2 fixture has audio: null
```

The real-Chrome checklist below is a mandatory acceptance test.

**Implementation.** Regenerate the shell. Extend the CLAUDE rehearsal bullet with v1/v2 separation, absolute timeline, explicit microphone intent, sink-only persistence, take/seq conflict proof, reset deletion, 5 MiB cap, visible errors, same stem, and `dist/` exclusion. Update README and the existing site CLI guide with `--rehearsal --audio`, indicator states, paired files, slide/audio output, and external playback. Add `ffmpeg -i in.webm -c copy out.webm` only if measurement proves remuxing necessary.

**Verification.** Run every gate again:

```sh
cargo test --workspace
cargo test --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
(cd packages/peitho-present && npm run build && npm test && npm run typecheck)
git diff --exit-code packages/peitho-present/dist/shell.js
git diff --exit-code packages/peitho-present/dist/preview.js
git diff --exit-code packages/peitho-present/dist/remote.js
make demo-site
git diff --check
```

Do not run Chrome while authoring this plan. Before M2 merges, the implementer must use real Chrome and a real microphone:

1. Run:

   ```sh
   cargo build -p peitho
   ./target/debug/peitho present examples/lightning-talk/deck.md --rehearsal --audio --presenter-windowed --port 6198
   ```

   Grant permission; verify pending/ready/recording/paused states and immediate start JSON. After one chunk is accepted, block the next audio POST in DevTools, reset, and verify the WebM is deleted and JSON has `audio: null`; unblock the old-take retry and verify 409. Start again, verify a new take ID at seq 0 recreates the WebM, then verify Esc final flush.

2. Speak a recognizable phrase after each slide entry, pause for 10 wall-clock seconds, resume, and close at a noted timer value. Locate the newest accepted pair:

   ```sh
   record=$(ls -t .peitho/rehearsals/rehearsal-*.json | head -n 1)
   audio="$(dirname "$record")/$(jq -r '.audio' "$record")"
   jq '{elapsedMs,timeline,audio}' "$record"
   ./target/debug/peitho rehearsal
   ffprobe -v error -select_streams a:0 -show_entries packet=pts_time,duration_time -of csv=p=0 "$audio" | awk -F, 'NF == 2 { duration = $1 + $2 } END { print duration }'
   jq -r '.timeline[].atMs / 1000' "$record" | while read -r seek; do
     ffplay -ss "$seek" -t 2 -autoexit "$audio"
   done
   ```

   The pause must add no audio duration; decoded duration must be within 0.5 seconds of `elapsedMs / 1000`; seeking to each recorded `atMs` must reach its phrase. If not, do not merge and do not use stop/start: add measured pause spans as required by the design.

3. Prove raw concatenation and remux behavior:

   ```sh
   ffmpeg -v error -i "$audio" -f null -
   ffmpeg -y -i "$audio" -c copy "${audio%.webm}-remux.webm"
   ffprobe -v error -show_entries format=duration "${audio%.webm}-remux.webm"
   ```

   If raw decode/seek fails but remux works, document remuxing and repeat seeks. If ffmpeg cannot consume the concatenation, stop M2.

4. Start a fresh run, block `/rehearsal/audio` in DevTools before starting the timer, wait for two chunks, verify one visible error and no parallel/reordered POSTs, unblock, and verify one take's seq 0 then 1 plus ffmpeg decode. Stop it, revoke microphone permission, then start another fresh run; verify visible `mic unavailable`, continuing timeline JSON, no same-stem WebM, and `audio: null`. Finally run `e2e_dist=$(mktemp -d)` followed by `./target/debug/peitho build examples/lightning-talk/deck.md --out "$e2e_dist"`; confirm that directory contains no rehearsal JSON/WebM.

## Summary

<!-- derived-from #milestone-1--v2-per-slide-timeline -->
<!-- derived-from #milestone-2--opt-in-timer-aligned-audio -->

PR 1 ships a v1-compatible, server-validated absolute slide timeline. PR 2 adds explicit microphone intent and visible, strictly ordered audio persistence. Both keep review terminal-only and rehearsal data outside distribution output.

## Open questions

- Chrome pause/resume timing and raw WebM seekability remain empirical merge gates; Task M2.5 decides whether pause spans or documented remuxing are required.
