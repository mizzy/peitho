# Phone remote control via a `/remote` route (Issue #289)

Date: 2026-07-16
Issue: #289

## Goal

Serve a phone-friendly `/remote` page from the `peitho present` server with
prev/next buttons and a slide counter, driving navigation through the existing
`/sync` long-polling channel. Add an opt-in `--host` flag so the server is
reachable from a phone (the recommended pattern is binding the Tailscale
address; see the issue comment).

## Decisions taken from the issue + author comment

- **Route naming**: `/remote` — extensionless and dot-free, consistent with
  `/presenter` / `/present-swapped` (Chrome app-name dot pitfall).
- **Reachability is the opt-in, not the route**: `remote.html` is always
  emitted and `/remote` is always served; without `--host` the server binds
  loopback only, so nothing new is exposed. `--host <addr>` is the explicit
  opt-in that changes exposure.
- **Binding a specific address (e.g. Tailscale `100.x.y.z`) is strictly better
  than `0.0.0.0`** when available — the flag accepts any IP address, so no
  Tailscale-specific code is needed for binding.
- **Skipped-slide semantics** on the remote's prev/next must match present-mode
  `next`/`prev`: resolve to the next non-skipped slide or no-op
  (`nextNonSkippedIndex` in `packages/peitho-present/src/skipnav.ts`).
- **Printed URL candidates**: prefer the default-route interface's IPv4,
  exclude loopback and link-local, keep CGNAT-range (`100.64.0.0/10`)
  addresses labeled as Tailscale, and print **all** candidates so the user can
  pick manually when the heuristic guesses wrong (better than Slidev, which
  has no priority logic).

## Out of scope (v1)

- **QR code** — the author comment classifies it as a first-run convenience
  once MagicDNS + a fixed `--port` make the URL bookmarkable. Follow-up.
- **Speaker notes on the remote** — the open question is explicitly
  undecided; the remote shows no notes and fetches only `manifest.json`.
- **Swap / close controls on the remote** — a phone remote is a clicker;
  accidentally ending or swapping the talk from a pocket is worse than not
  having the buttons. The remote reacts to an inbound `{close:true}` by
  showing an "ended" state, but never emits one.

## Design

### 1. Dual-bind server (`crates/peitho/src/server.rs`)

Binding *only* the `--host` address would break the local slides/presenter
windows: they open `http://localhost:<port>/...` (the dot-free app-name
convention, which cannot change), and a server bound only to `100.x.y.z` does
not listen on loopback. The invariant is: **local behavior is unchanged;
`--host` only adds exposure.**

- `PresentServer::bind` stays loopback-only and unchanged (preview keeps using
  it as-is).
- A pure decision function `bind_plan(host: Option<IpAddr>) -> BindPlan`
  (unit-tested, no I/O) selects the listener topology:
  - `None` → `LoopbackOnly` (unchanged default),
  - unspecified host (`0.0.0.0` / `::`) → `WildcardOnly`: the wildcard is the
    **single** listener — it already covers loopback, so dual-binding would
    only create a same-port overlap that needs `SO_REUSEPORT`-style socket
    flags, and those flags would let a second `peitho present --port N`
    process bind the same active port silently. No reuse flags, ever.
  - specific host → `LoopbackPlusExtra`: loopback primary (resolves the
    port), then an extra listener at `(host, resolved_port)` sharing the same
    `root` and `SyncHub` — two disjoint specific addresses on one port need
    no socket flags on any platform.
- `add_listener` rejects unspecified addresses (help: bind the wildcard as
  the primary listener) so the overlap cannot be reintroduced; specific
  loopback addresses stay accepted at the server API level (the loopback ban
  is CLI policy, and tests use `::1` as the extra listener).
- Known tradeoff: `--host ::` relies on the platform dual-stack default
  (IPV6_V6ONLY off, dual-stack true on macOS and stock Linux) so local
  windows' `127.0.0.1` URLs keep working; v6only platforms are out of scope.
- `serve_forever` runs all accept loops (extra listener on a thread).
- **Shutdown must unblock every listener**: `ShutdownHandle` used to hold the
  one `Arc<Server>` that received the `{close:true}`. With two listeners, a
  close arriving on either must unblock both, or the process never exits —
  the shutdown path owns the shared listener list.
- `--host` with a loopback address is a line-of-help error (it adds nothing);
  unspecified addresses (`0.0.0.0` / `::`) are allowed.

### 2. CLI flag (`crates/peitho/src/main.rs`)

- `peitho present --host <IP>` — clap arg parsed as `IpAddr`, new
  `PresentOptions::host: Option<IpAddr>`.
- `--host` combined with `--no-serve` is an error (no server, nothing to
  bind — no silent ignoring).
- After binding, print the remote URL(s) next to the existing
  `serving presentation at {url}` line:
  - specific `--host` → one line: `remote control: http://<host>:<port>/remote`
    (IPv6 bracketed).
  - unspecified (`0.0.0.0`/`::`) → enumerate candidates (below), one line
    each, default-route candidate first, Tailscale candidates labeled.

### 3. Candidate URL enumeration (new, `crates/peitho/src/`)

- Add the `if-addrs` crate to enumerate interface addresses.
- Default-route IPv4 detection via the std-only UDP-connect trick
  (`UdpSocket::connect("8.8.8.8:80")` + `local_addr()` — no packet is sent);
  failure degrades to unordered candidates, never an error.
- Pure function (unit-tested, no I/O):
  `remote_url_candidates(addrs, default_route, port) -> Vec<RemoteUrlCandidate>`
  - excludes loopback and link-local (`169.254.0.0/16`, `fe80::/10`),
  - keeps and labels `100.64.0.0/10` as Tailscale,
  - orders: default-route address first, then Tailscale, then the rest,
  - formats `http://<ip>:<port>/remote`.

### 4. Remote page (TS + template + embed)

Follows the existing two-bundle pattern exactly, as a third bundle:

- `packages/peitho-present/src/remote.ts` → `dist/remote.js` (new esbuild
  entry in `esbuild.config.mjs`).
- `BUILTIN_REMOTE_JS = include_str!(...)` next to `BUILTIN_SHELL_JS`
  (`main.rs`), written into the present cache by `emit_present_cache` as
  `remote.js`, with a drift test alongside the existing shell/preview drift
  tests and the `git diff --exit-code` gate.
- `render_remote_index()` template in `crates/peitho-core/src/render.rs`
  (exported via `lib.rs`), written as `remote.html` into the present cache.
  Namespace import + feature detection (`import * as peitho from
  './remote.js'`), phone viewport meta, large touch targets.
- Server: `/remote` alias → `remote.html` in `resolve_request_path`
  (`server.rs`), next to the `/presenter` aliases.
- Preview cache does **not** get remote assets (preview has no phone story).

### 5. Remote page behavior (`src/remote.ts`)

§16 holds: buttons only dispatch request events; the remote shell executes.

- Buttons dispatch `peitho:navigate` `{to:"next"|"prev"}` (reuses the existing
  event vocabulary).
- A remote controller (`mountRemoteView` + `installRemoteSyncBridge`-style
  seam, mirroring `shell.ts`/`sync.ts` structure):
  - fetches `manifest.json` for slide count + `skip` flags; fetch failure is a
    visible error state, never a blank page;
  - joins `/sync` via the existing `serverSyncChannelFactory` (handshake +
    poll); replayed `index` updates the local current index and the counter;
  - `next`/`prev` resolve to an absolute index with `nextNonSkippedIndex`
    from the current index (a `null` index — nobody has navigated yet —
    resolves from the first non-skipped slide, matching present's initial
    slide), then `POST /sync {"index":N}`; no-op at the ends;
  - out-of-range replayed indexes are clamped, never crash;
  - counter renders `current+1 / total` (indexes include skipped slides, same
    as everywhere else);
  - inbound `{close:true}` disables the buttons and shows an "ended" state;
    `swapped` and `generation` are ignored (no reload story for present).
- Exports added to `src/index.ts`? **No** — `remote.ts` is its own entry;
  `shell.js` must not grow remote code.

### 6. Docs

- `site/content/guide/cli.md` `peitho present` section: document `--host`,
  the `/remote` route, and the Tailscale recommendation (short — link the
  thinking, don't duplicate the issue).

## TDD task list

Each step is red → green → refactor; production code only after a failing test.

1. **`/remote` alias** — `server.rs` unit test: `resolve_request_path` maps
   `/remote` → `remote.html`.
2. **CLI parsing** — `main.rs` tests: `present --host 100.64.0.5` parses;
   invalid IP fails; `--host` + `--no-serve` errors; loopback `--host` errors.
3. **Dual bind + shutdown** — `crates/peitho/tests/present.rs` integration:
   server with extra listener serves `manifest.json` on both addresses
   (use `127.0.0.1` + a second loopback-reachable bind for CI safety — bind
   extra on `127.0.0.1` is rejected, so exercise via `0.0.0.0` guarded or via
   unit-level listener plumbing); `{close:true}` posted to either listener
   unblocks both accept loops.
4. **Candidate enumeration** — unit tests for `remote_url_candidates`:
   loopback/link-local excluded, CGNAT labeled Tailscale, default-route
   ordering, IPv6 bracket formatting.
5. **Cache emit + embed** — `tests/present.rs`: present cache contains
   `remote.html` + `remote.js`; drift test for `BUILTIN_REMOTE_JS`; preview
   cache does not contain them.
6. **Template** — render test: `render_remote_index` output uses namespace
   import + feature detection, fetches `manifest.json`, has viewport meta.
7. **Remote TS** — vitest `test/remote.test.ts`: buttons dispatch
   `peitho:navigate`; controller resolves next/prev across skip flags;
   posts `{index}`; null-index initial resolution; end no-ops; replay updates
   counter; clamping; close → ended state; manifest fetch failure → error
   state. Add to `generated.test.ts` export-presence checks if applicable.
8. **URL printing** — present integration or unit seam: specific host prints
   one URL; unspecified prints ordered labeled candidates.
9. **Docs** — `cli.md` update (no test; reviewed by reading).

## Gates

All of `CLAUDE.md`'s gates, including the two dist drift checks plus the new
`git diff --exit-code packages/peitho-present/dist/remote.js` once the bundle
is committed, and `cargo test` × 3.
