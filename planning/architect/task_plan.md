# Architect Task Plan — Overall Coordination

## Goal
Coordinate the FT-991A radio control application: a `radio` crate (command
table, `CatRadio` state machine, controller client), a `ui` crate (ratatui
TUI), an `emulator` crate (PTY-hosted `CatFramework<Ft991aRadio>`), and the
`app` wiring layer (`src/main.rs`) — all built on the shared `cat-framework`/
`cat-client`/`cat-transport-core`/`cat-transport-serial` crates consumed as
external git dependencies from `radio-cat-rs`.

## Status: UNBLOCKED — implementation dispatch beginning (2026-07-17)

All three preconditions recorded in
`../../docs/adr/0001-second-radio-on-shared-cat-framework.md` have cleared:

1. `radio-cat-rs` (https://github.com/kf0uwv/radio-cat-rs) has extracted and
   published `cat-framework`, `cat-client`, `cat-transport-core`,
   `cat-transport-serial` on `origin/main` (commit `0c13844`). `ts570d` has
   already migrated onto them — see `ts570d/Cargo.toml` for the exact
   dependency syntax mirrored below.
2. The official Yaesu FT-991A CAT Operation Reference Manual is checked in
   at `../../docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` (20 pages, read in
   full for this plan).
3. User go-ahead given ("let's implement the ft991a").

ADR 0001, `docs/adr/README.md`, and `CLAUDE.md` have been updated in place
to reflect this. This document now records the design and dispatch queue
for the first implementation wave.

---

## Sources read for this plan (session 2026-07-17)

- `docs/adr/0001-second-radio-on-shared-cat-framework.md` (full)
- `ts570d/docs/adr/0004-extraction-boundary.md` ("Adding a second radio")
- `ts570d/Cargo.toml`, `ts570d/radio/Cargo.toml`, `ts570d/src/main.rs`,
  `ts570d/radio/src/ts570d.rs`, `ts570d/radio/src/ts570d_radio.rs` (ground
  truth for the shape being mirrored, not the ADRs' paraphrase)
- `radio-cat-rs/cat-framework/src/cat.rs`,
  `radio-cat-rs/cat-client/src/client.rs`,
  `radio-cat-rs/cat-transport-serial/src/lib.rs` +
  `.../src/io_uring.rs` (`SerialConfig` section)
- `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` — all 20 pages, in full

---

## 1. Workspace / dependency design

Mirrors `ts570d`'s post-remap shape exactly:

```toml
[workspace]
members = ["radio", "ui", "emulator"]   # ui/emulator populated incrementally — see dispatch queue
resolver = "2"

[workspace.dependencies]
monoio, local-sync, async-trait, ratatui, crossterm, thiserror,
serde(+json), tracing(+subscriber), bytes, futures, libc, nix,
tempfile, mockall   # same versions ts570d pins unless an agent finds a reason to diverge

cat-framework        = { git = "https://github.com/kf0uwv/radio-cat-rs", branch = "main" }
cat-client            = { git = "https://github.com/kf0uwv/radio-cat-rs", branch = "main" }
cat-transport-core    = { git = "https://github.com/kf0uwv/radio-cat-rs", branch = "main" }
cat-transport-serial  = { git = "https://github.com/kf0uwv/radio-cat-rs", branch = "main" }

[package]
name = "ft991a"
# version/edition/authors/license from workspace, mirroring ts570d's package metadata

[[bin]]
name = "ft991a"
path = "src/main.rs"

[dependencies]
monoio, thiserror, tracing, tracing-subscriber, libc,
cat-transport-serial = { workspace = true }   # only concrete transport main.rs names directly
radio = { path = "radio" }
ui = { path = "ui" }

[dev-dependencies]
emulator = { path = "emulator" }
radio = { path = "radio" }
monoio = { workspace = true }
cat-transport-serial = { workspace = true }

[profile.release]
lto = true
codegen-units = 1
panic = "abort"
```

`radio/Cargo.toml`: depends on `cat-framework`, `cat-client`,
`cat-transport-core` (workspace), `monoio`, `async-trait`, `thiserror`;
dev-deps `tempfile`, `mockall`, and (test-only, exactly like `ts570d`)
`cat-transport-serial` for wire-framing-level unit tests only — never in
production code paths.

`ui/Cargo.toml` / `emulator/Cargo.toml`: per CLAUDE.md's dependency-model
diagram (`ui` → `radio` only + ratatui/crossterm; `emulator` → `cat-framework`
+ `radio`). Not read from a live example this session — the `ui`/`emulator`
agents should confirm shape against `ts570d/ui/Cargo.toml` and
`ts570d/emulator/Cargo.toml` directly when their waves are dispatched.

### Serial-crate decision: RESOLVED — no local `serial` crate

CLAUDE.md's "OPEN DECISION" is now closed and CLAUDE.md updated accordingly.
**This repo depends directly on `cat-transport-serial`; no local `serial`
crate is created.**

Reasoning, checked against the manual rather than assumed by TS-570D parity:

- Manual p.1 "Connection": FT-991A CAT port is RS-232C (built-in level
  converter) or USB (built-in USB-to-Dual-UART bridge, needs a driver) —
  standard serial framing either way, not a proprietary transport.
- Manual p.7, Menu items 029 "232C RATE" / 031 "CAT RATE": FT-991A serial
  baud rates are exactly **4800 / 9600 / 19200 / 38400 bps**.
  `cat-transport-serial/src/io_uring.rs`'s `baud_rate_from_u32` already maps
  all four (plus 1200/2400/57600/115200/230400) to
  `nix::sys::termios::BaudRate` — no transport code changes needed.
- Data bits / parity / stop bits aren't spelled out beyond "standard serial
  cable, not null-modem" in this manual, but `SerialConfig::default()`
  (`baud_rate: 9600, data_bits: 8, stop_bits: 2, parity: None,
  flow_control: None`) already matches the industry-known Yaesu default
  framing (8N2), and the struct stays fully configurable if a future agent
  finds real-hardware evidence otherwise.
- Manual p.8, Menu item 033 "CAT RTS" (0 DISABLE / 1 ENABLE): optional
  hardware RTS/CTS handshake. `SerialConfig.flow_control: FlowControl`
  already has a `Hardware` variant.
- Frame terminator is `;` for both radios — matches
  `cat_transport_core`/`cat-transport-serial`'s read-until-`;` framing
  (`SerialCatSession`) unchanged.
- The one FT-991A serial quirk found (`PS`'s "dummy data, then 1–2s delay"
  wake sequence — see §2) is an application-level *sequencing* concern, not
  a transport-*framing* concern: a caller-side `sleep` + two `send` calls
  over the existing session API, no new trait surface needed.

Reimplementing a transport locally would only recreate the exact
duplication `ts570d`'s extraction eliminated, for zero protocol benefit.

---

## 2. First-slice `Ft991aCommandId` / `FT991A_COMMAND_TABLE` scope

11 commands, covering VFO A/B frequency, mode, PTT, S-meter, power on/off,
and basic gain/level controls (AF/RF gain, squelch, TX power) plus radio
identification — sized from the FT-991A's actual manual, not assumed from
TS-570D parity. All widths/forms below are transcribed directly from
`docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf`; page numbers are the printed
page footers.

| Id | Code | Manual page | Query wire form | Set wire form | Notes |
|----|------|------|------|------|-------|
| Fa | `FA` | p.9 | `FA;` → `FA<9 digits>;` | `FA<9 digits>;` | Freq range 000030000–470000000 Hz. **9-digit field, not TS-570D's 11.** |
| Fb | `FB` | p.9 | `FB;` → `FB<9 digits>;` | `FB<9 digits>;` | Same shape as FA. |
| Md | `MD` | p.11 | `MD0;` → `MD0<mode>;` | `MD0<mode>;` | See "selector reads" below — read is **not** zero-width. `<mode>` is one hex char 1–E (1 LSB,2 USB,3 CW-U,4 FM,5 AM,6 RTTY-LSB,7 CW-L,8 DATA-LSB,9 RTTY-USB,A DATA-FM,B FM-N,C DATA-USB,D AM-N,E C4FM). |
| Tx | `TX` | p.17 | `TX;` → `TX<0/1/2>;` | `TX<0/1>;` | Zero-width query. Set 0 = CAT-TX off (release), 1 = CAT-TX on (key). Answer/read can report 2 = radio TX on via some other cause, CAT-TX off — **3-valued, not a simple bool.** |
| Sm | `SM` | p.17 | `SM0;` → `SM0<3 digits>;` | none (read-only) | Selector read (fixed `0`); level 000–255. |
| Ps | `PS` | p.14 | `PS;` → `PS<0/1>;` | `PS<0/1>;` | Zero-width query/set. **Quirk**: manual states "This command requires dummy data be initially sent. Then after one second and before two seconds the command is sent" — a wake-from-standby sequencing requirement, flagged for `yaesu`/`app`. |
| Ag | `AG` | p.4 | `AG0;` → `AG0<3 digits>;` | `AG0<3 digits>;` | Selector read/write (fixed `0`); level 000–255. |
| Rg | `RG` | p.15 | `RG0;` → `RG0<3 digits>;` | `RG0<3 digits>;` | Selector read/write (fixed `0`); level 000–255. |
| Sq | `SQ` | p.17 | `SQ0;` → `SQ0<3 digits>;` | `SQ0<3 digits>;` | Selector read/write (fixed `0`); level 000–100 (different range from AG/RG). |
| Pc | `PC` | p.14 | `PC;` → `PC<3 digits>;` | `PC<3 digits>;` | Zero-width query. Watts 005–100. |
| Id | `ID` | p.10 | `ID;` → `ID<4 hex digits>;` | none (read-only) | FT-991A's ID is `0670` (fixed). |

Cross-checked against the master Set/Read/Ans/AI table on manual p.3 — all
11 commands' O/X flags match the above (FA/FB/MD/AG/RG/SQ/PC all "O O O O";
TX "O O O O"; SM "X O O X"; PS "O O O X"; ID "X O O X").

### Non-obvious structural finding: "selector reads" — read this before implementing

`MD`, `SM`, `AG`, `RG`, `SQ` all take a **parameter even on read** (e.g.
`MD0;`, `SM0;`, `AG0;` — the `0` addresses "main receiver", since the
FT-991A has a sub-receiver architecture). `cat-framework`'s
`CommandTable::parse` (`cat.rs` lines 147–182) only ever classifies a
**zero-length** parameter as `CommandOperation::Query`; any non-empty
parameter is matched against `set_forms` instead, regardless of the
command's actual read/write semantics. This is not a gap in
`cat-framework` — `ts570d`'s own `SM`/`MR` commands hit the exact same
shape (`ts570d_radio.rs` line ~182 comment: *"SM/KY/MR: the wire-grammar
forms take a selector parameter, so their documented controller read/write
is stated explicitly (docs authoritative)"*). Reuse that established,
already-working pattern:

- Put **both** the selector-only width (for reads) and the real write width
  (if any) into `set_forms` as separate `CommandForm::fixed(Set, N)`
  entries.
- Set the `CommandDefinition`'s `readable`/`writable` flags **explicitly**
  (not derived) to the documented controller capability — e.g. `Sm`:
  `readable: true, writable: false` even though its only wire form is
  structurally a `Set`.
- `handle_command` distinguishes "this is actually a read" vs. "this is
  actually a write" by inspecting `request.parameters.raw().len()` (1 =
  selector-only read; 2 = selector+mode write for MD; 4 = selector+level
  write for AG/RG/SQ), not by `request.operation` alone.
- Client-side, `cat_client::CatClient::query_with_param(code, "0")` already
  exists for exactly this (used by `ts570d` for `SM0;`/`RM1;`) — no new
  client-side mechanism needed.

This is easy to get wrong by analogy to TS-570D (whose `MD`/`AG` reads
*are* zero-width) — call it out explicitly in the `yaesu` dispatch.

### Gap acknowledged, not silently assumed

The manual's 20 pages, read in full, do **not** state an explicit
protocol-error response format anywhere (unlike `ts570d`'s Kenwood manual,
which documents `?;` directly). Yaesu radios including the FT-991A are
widely known (outside this manual) to also respond `?;` to malformed/
unknown CAT commands, and using `"?;"` for `write_protocol_error` is a
reasonable default — but this is **not manual-cited**, and the `yaesu`
agent should state it as an explicit assumption in its own
`planning/yaesu/task_plan.md`/code comments, not silently inherit it from
`ts570d`.

### Explicitly out of scope for this wave (follow-on, not dropped)

- **`IF` (Information, p.10)** — composite ~27-byte status response
  (memory channel, VFO-A freq, clarifier direction/offset, RX/TX clarifier
  on/off, mode, VFO/memory select, CTCSS/DCS status, offset type).
  Deliberately excluded: the field-boundary transcription from the
  extracted manual text has one ambiguous spot (a possible P3/P4 column
  overlap around the clarifier fields), and this response is operationally
  important enough to deserve its own careful, page-10-only pass by the
  `yaesu` agent (re-verify column-by-column against the manual image
  directly) rather than being rushed into the first wave.
- **`RM`** (Read Meter, p.15) and **`RI`** (Radio Information, p.15) —
  additional status/meter reads.
- **`RA`/`PA`** (attenuator/preamp, p.15/p.14), **`NB`/`NR`/`NA`** (noise
  blanker/reduction/narrow, p.13), **`RT`/`RU`/`RD`/`XT`** (clarifier
  RIT/XIT, p.15/p.18), **`SC`** (scan, p.16), **`VX`/`VG`/`VD`** (VOX,
  p.18) — natural next-wave additions.
- **Memory channels**: `MC`/`MR`/`MW`/`MT`/`MA`/`BA`/`AM`/`CH`/`QI`/`QR`/
  `QS`/`SV` (p.5–15) — non-trivial fixed-width records, own wave.
- **`EX` (Menu, p.7–9)** — 153 numbered menu parameters, each with its own
  width/range; a large, separate body of work.
- Keyer (`KM`/`KP`/`KR`/`KS`/`KY`), antenna tuner (`AC`), band select
  (`BS`/`BU`/`BD`), date/time (`DT`), dimmer (`DA`), CTCSS/DCS (`CN`/`CT`),
  IF-shift (`IS`), speech processor (`PL`/`PR`), notch (`BC`/`BP`/`CO`),
  DVS record/playback (`LM`/`PB`), lock (`LK`), width (`SH`), break-in
  (`BI`/`SD`), fast step (`FS`), encoder (`ED`/`EU`/`EK`), auto-info
  (`AI`), unlock (`UL`), swap-VFO (`SV`), up/down (`UP`/`DN`), zero-in
  (`ZI`), opposite-band info (`OI`), repeater offset (`OS`).

---

## 3. `Ft991aState` / `Ft991aRadio` / `CatRadio` impl shape

Mirrors `ts570d_radio.rs` structurally:

- `Ft991aCommandId` enum: exactly the 11 first-slice variants (`Fa, Fb, Md,
  Tx, Sm, Ps, Ag, Rg, Sq, Pc, Id`). Unlike `ts570d`'s table (which included
  a documented-but-unemulated superset up front), this repo's table should
  **only** contain manual-cited, width-verified entries — do not pad with
  placeholder variants for commands not yet transcribed; grow the
  enum/table in each follow-on wave instead.
- `CommandForm` consts needed: `QUERY0` (Fa/Fb/Tx/Ps/Pc/Id read), `SET_9`
  (Fa/Fb set), a width-1 selector-read form (Md/Sm/Ag/Rg/Sq read), `SET_2`
  (Md real set: selector+mode), `SET_4` (Ag/Rg/Sq real set: selector+
  3-digit level), `SET_1` (Tx set, Ps set), `SET_3` (Pc set). `Md`/`Ag`/
  `Rg`/`Sq`'s `set_forms` slices each need **two** `CommandForm` entries
  (selector-read width + real-set width) per the "selector reads" finding;
  `Sm`'s `set_forms` needs just the one selector-read width with
  `writable: false` explicit.
- `Ft991aState`: `vfo_a_hz: u64`, `vfo_b_hz: u64`, `mode: u8` (raw
  hex-nibble value 1–14/0xE), `cat_tx: u8` (0/1, CAT-asserted PTT state —
  distinct from any front-panel-asserted TX the emulator might model
  later), `af_gain: u8`, `rf_gain: u8`, `squelch: u8`, `power_control: u8`
  (PC watts), `smeter: u8` (0–255), `power_on: bool`; `id` is a `const`,
  not stored state (`"0670"` fixed).
- `Ft991aEvent { field: &'static str, value: String }` — identical shape to
  `Ts570dEvent`.
- `Ft991aRadio { state: Ft991aState }`, `CatCommandCatalog` returning
  `&FT991A_COMMAND_TABLE`; `CatRadio::handle_command` dispatches on
  `(request.id, request.parameters.raw().len())` for the selector-read
  commands and on `request.id` alone for the rest; `write_protocol_error`
  writes `"?;"` (flagged as an assumption, not manual-cited — see §2).
- Unit tests: mirror `ts570d_radio.rs`'s `#[cfg(test)] mod tests` — table
  integrity (unique codes/ids, every definition has a legal operation),
  `CatFramework::process_frame` round-trips for at least `FA`/`MD`/`TX`,
  and one explicit test proving the `MD0;`/`AG0;`-style selector-read shape
  parses as intended (highest risk-of-being-wrong part of this design;
  deserves direct test coverage, not just review).

---

## 4. Controller client shape

`Ft991a<S: CatSession>`, wrapping `cat_client::CatClient<Ft991aCommandId,
SharedSession<S>>` — same two-field struct (`client`, `session`) and the
same `SharedSession<S>` `Rc<RefCell<Option<S>>>` adapter `ts570d.rs` defines
(lines 65–131), copied near-verbatim (it solves a generic
monoio-`!Send`-futures-vs-`RefCell`-borrow problem, not an FT-991A-specific
one). Needed because `Ft991a::flush_rx` and any wire-level test assertions
require direct session access `CatClient` doesn't expose.

Methods (first slice only):
- `get_vfo_a`/`set_vfo_a`, `get_vfo_b`/`set_vfo_b` — plain `query("FA")`/
  `set("FA", ...)`, `{:09}` zero-padded (not `{:011}` — 9-digit field).
- `get_mode`/`set_mode` — `query_with_param("MD", "0")` for read, `set("MD",
  "0" + mode_char)` for write. `Mode` domain enum encodes the FT-991A's
  hex-nibble scheme (1 LSB … E C4FM), not TS-570D's single-digit scheme.
- `transmit`/`receive` — `set("TX", "1")`/`set("TX", "0")`. Also expose a
  `get_tx_state() -> RadioResult<u8>` (or a small 3-value enum) for the
  `TX;` query's 0/1/2 answer, since `Radio::transmit`/`receive` alone can't
  represent "radio is transmitting via a non-CAT cause" — do not silently
  collapse this to a bool the way TS-570D's simpler `TX;`/`RX;` write-only
  actions could.
- `get_smeter` — `query_with_param("SM", "0")`.
- `get_power_on`/`set_power_on` — plain `query("PS")`/`set("PS", ...)`. The
  "dummy data, then wait 1–2s, then PS1;" wake sequence (p.14) is a
  **caller-side sequencing concern**, not something `CatClient`/`set`
  itself can express — recommend a dedicated `wake_and_power_on()` helper
  on `Ft991a` (send arbitrary bytes via the session, `monoio` timer sleep,
  then `set_power_on(true)`) rather than baking the delay into
  `set_power_on` itself (which should stay a faithful 1:1 wire mapping).
  First-slice nice-to-have, not a hard blocker.
- `get_af_gain`/`set_af_gain`, `get_rf_gain`/`set_rf_gain`,
  `get_squelch`/`set_squelch` — `query_with_param(code, "0")` / `set(code,
  "0" + "{:03}")`.
- `get_power`/`set_power` — plain `query("PC")`/`set("PC", "{:03}")`.
- `get_id` — plain `query("ID")`, parse 4 hex digits — confirm hex-vs-
  decimal parsing choice against the p.10 citation when implementing
  (`0670` isn't purely decimal-looking but is described as 4 arbitrary
  digits/chars in the P1 column).

`Radio` trait: implement **only** the subset of `radio`'s own `Radio` trait
that the first slice actually backs (vfo a/b, mode, ptt, smeter, power
on/off, af/rf gain, squelch, tx power, id). Do not stub the rest of the
trait surface with `unimplemented!()`/`todo!()` — grow the trait alongside
command coverage in each wave, since some of `ts570d`'s `Radio` trait shape
(bool-only RIT/XIT, single power-on bool) may not map 1:1 once FT-991A's
dual-receiver/hex-mode/3-valued-TX quirks are factored in. This is a
deliberate deviation from "mirror ts570d's Radio trait verbatim" — flagged
here, not silently assumed.

### Domain types

- `Frequency`: thin `u64` Hz wrapper like `ts570d`'s, but
  `to_protocol_string()` zero-pads to **9** digits (`{:09}`), and valid
  range is 30,000–470,000,000 Hz per FA/FB's p.9 citation — do not reuse
  TS-570D's range/width.
- `Mode`: new enum, hex-nibble-valued (`1` Lsb, `2` Usb, `3` CwU, `4` Fm,
  `5` Am, `6` RttyLsb, `7` CwL, `8` DataLsb, `9` RttyUsb, `A` DataFm, `B`
  FmN, `C` DataUsb, `D` AmN, `E` C4fm) — cited p.11. Not compatible with
  `ts570d::Mode`'s digit scheme; a fresh type.
- `RadioError`/`RadioResult`: thiserror-based, wrapping
  `cat_client::ClientError<TransportError>` plus FT-991A-specific variants
  (`InvalidMode`, `FrequencyOutOfRange`, `InvalidProtocolString`) — same
  shape `ts570d::RadioError` uses, adjusted for the two domain types above.
- `InformationResponse`/`MemoryChannelEntry` — deferred to the `IF`/memory
  follow-on waves, not designed this session.

---

## 5. Dispatch queue — Wave 1 (this dispatch)

Two tasks, independent enough to run in parallel (`yaesu` owns `radio/`,
`app` owns `src/` + root `Cargo.toml` + the workspace's own `Cargo.toml`;
neither touches the other's files). Each must write its own
`planning/{agent}/task_plan.md` and get review/approval before writing code,
per the Architect Review Workflow — this plan does not pre-approve their
code, only pre-approves dispatching them with this scope.

### Task 1 — `yaesu` agent: `radio` crate, first slice

Scope: `radio/Cargo.toml` + `radio/src/*`. Deliver:
- `Ft991aCommandId` enum + `FT991A_COMMAND_TABLE` for exactly the 11
  commands in §2, with the exact wire widths/forms cited there (re-verify
  each against the manual pages cited, not just this summary).
- `Ft991aState`/`Ft991aEvent`/`Ft991aRadio` per §3, implementing
  `CatCommandCatalog` + `CatRadio` from `cat-framework`.
- `Ft991a<S: CatSession>` controller client per §4, wrapping
  `cat_client::CatClient` + the `SharedSession<S>` adapter copied from
  `ts570d/radio/src/ts570d.rs`.
- `radio`-crate-local `Radio` trait (first-slice subset only, per §4) +
  `Frequency`/`Mode`/`RadioError`/`RadioResult` domain types per §4.
- Unit tests per §3's last bullet, run entirely in-process via
  `cat_framework::CatFramework` — no PTY/emulator needed for this task (see
  Wave 1 reasoning below).
- Explicitly state, in `planning/yaesu/task_plan.md`, the `"?;"`
  protocol-error-response assumption from §2 as an open item to verify
  against real hardware or a later official errata, since the manual does
  not state it.

### Task 2 — `app` agent: workspace scaffold + minimal wiring binary

Scope: root `Cargo.toml`, `src/main.rs`, plus a **minimal placeholder `ui`
crate** (an empty `pub async fn run<R: radio::Radio>(radio: R) -> Result<(),
...> { Ok(()) }` stub — not the ratatui TUI, which is Wave 2's job). Deliver:
- Root `Cargo.toml` per §1 (workspace members `["radio", "ui"]` only for
  now — `emulator` is not added as a workspace member until its own wave,
  see reasoning below).
- `src/main.rs` mirroring `ts570d/src/main.rs`'s shape: `--port`/`--baud`/
  `--stop-bits` CLI parsing (baud choices should include the FT-991A's
  4800/9600/19200/38400 per §1, not TS-570D's 1200/2400/4800/9600),
  `SerialPort::open`, `Ft991a::new(SerialCatSession::new(port))`, then
  `ui::run(radio).await` against the placeholder stub.
- Confirm `ui`/`emulator`'s real `Cargo.toml` shape by reading
  `ts570d/ui/Cargo.toml`/`ts570d/emulator/Cargo.toml` directly before
  finalizing the placeholder — this session did not read those files.

### Why no `ui`/`emulator` implementation work this wave

- **`ui`**: the ratatui TUI is a substantial, independently reviewable body
  of work (layout, widgets, live-updating fields) that depends on the
  `radio` crate's `Radio` trait actually existing first. Building it against
  a moving/unreviewed `radio` API in the same wave risks rework. Deferred
  to Wave 2, after Task 1 is reviewed and approved.
- **`emulator`**: NOT necessary to test the `yaesu` agent's Wave-1 work.
  `cat_framework::CatFramework<R: CatRadio>` can be driven entirely
  in-process via `process_frame(&str, &mut Vec<u8>)` — exactly how
  `ts570d_radio.rs`'s own test module validates `Ts570dRadio` today, with
  no PTY, no separate process, and no `emulator` crate involved. The
  `emulator` crate's actual job (PTY hosting, a real second process for
  external end-to-end testing) is orthogonal to unit-testing the command
  table/state machine, so it's deferred to its own wave rather than treated
  as a Wave-1 dependency. `app`'s `[dev-dependencies]` entry for `emulator`
  is therefore also deferred — added when the `emulator` crate exists.

## 6. Wave 2 — `ui` crate design

### Sources read for this section (session 2026-07-17, Wave 2)

- `radio/src/radio_trait.rs`, `radio/src/ft991a.rs`, `radio/src/lib.rs` (full)
  — the real, committed first-slice `Radio` trait/domain types (`e3698cf`),
  not the §4 sketch above.
- `ts570d/ui/src/layout.rs`, `ts570d/ui/src/control.rs` (full, structural
  reference), `ts570d/ui/src/lib.rs` (`RadioDisplay`, `run` re-export),
  `ts570d/ui/src/terminal.rs` (`run`/`poll_radio_state` signatures and poll
  cadence only — grepped, not read in full: 2496 lines, mostly event-loop
  plumbing that doesn't change shape-wise for a smaller command set).
- `ft991a/ui/src/lib.rs`, `ft991a/ui/Cargo.toml`, `ts570d/ui/Cargo.toml`
  (current placeholder vs. real target dependency shape).

### 6.1 Scope decision: single flat screen, not ts570d's menu tree

**Decision: right-sized down from ts570d's `Menu` → `GroupMenu` →
`{TextInput,ListSelect}` three-level hierarchy to a flat `Normal` →
`{TextInput,ListSelect}` two-level state machine, with no group layer.**

Why ts570d needed groups: its `control.rs` has **8 top-level command
groups** (`Frequency`, `Memory`, `ModeDsp`, `Receive`, `Transmit`, `Cw`,
`Tones`, `System`) holding 5-12 items apiece — `frequency_commands()` alone
has 12 entries — because it exposes roughly 60 distinct operations across
RIT/XIT, memory channels, CW keyer, tone squelch, AGC, noise
reduction/blanker, VOX, antenna tuner, etc. A single flat keymap at that
scale would not fit on screen and would not be learnable.

Why `ft991a` doesn't need that: the first slice is **9 write-capable
operations** (`set_vfo_a`, `set_vfo_b`, `set_mode`, `transmit`/`receive`,
`set_af_gain`, `set_rf_gain`, `set_squelch`, `set_power_on`, `set_power`)
plus 2 read-only values shown live without a key (`get_smeter`, `get_id`).
Nine keybindings fit on one screen with room to spare — introducing a group
layer for 9 items is the over-building this dispatch was explicitly warned
against; introducing *no* structured input handling at all (raw single-char
polling with inline parsing) would under-scope it, since frequency entry
and mode selection both genuinely need multi-character text input /
multi-option selection, not just a keypress. The flat two-level design is
the right size: real input validation and list selection where the domain
requires it (frequency, mode), direct one-key dispatch everywhere else.

**Reused from ts570d without change:** the `CommandKind::{Text, List,
Immediate}` descriptor pattern (`control.rs` lines 219-237) — a small
enum-of-closures-ish table that drives both the keymap and its rendering.
This is genuinely reusable at any command count and is *not* the part that
made ts570d's tree feel heavy (the two-level group nesting is). Keep the
descriptor-table idea, drop the group layer.

**Reused from ts570d without change:** `draw_disconnected`/connection-health
handling (`layout.rs` lines 191-241, `RadioDisplay.connected`/
`initializing`/`poll_errors` fields). This is infrastructure about session
liveness, not about how many commands exist — cutting it down would be a
false economy. Port it structurally unchanged.

**Not built this wave, and explicitly not a gap:** ts570d's `Diagnostic`
mode (`diag.rs`, `ControlState::Diagnostic`, the `[D]` key) — a
send-every-command-N-times self-test harness. It only makes sense once
there's a broader command surface to exercise; revisit when a later wave
grows the table past this first slice. No `diag.rs` file in `ft991a/ui`
this wave.

### 6.2 Display state: `Ft991aDisplay`

New struct (mirrors `ts570d::ui::RadioDisplay`'s role, not its field list —
that struct is ts570d-specific: RIT/XIT/split/memory/antenna/AGC/etc. don't
exist in this slice):

```rust
pub struct Ft991aDisplay {
    pub vfo_a_hz: u64,
    pub vfo_b_hz: u64,
    pub mode: Mode,                // radio::Mode; Default -> Mode::Usb
    pub tx_state: TxState,         // radio::TxState — 3-valued, see 6.5
    pub smeter: u8,                // 0-255, read-only
    pub power_on: bool,
    pub af_gain: u8,                // 0-255
    pub rf_gain: u8,                // 0-255
    pub squelch: u8,                // 0-100 (not 255 — different range, see radio/ft991a.rs)
    pub power_watts: u8,            // 5-100 (PC)
    pub id: String,                 // fetched once at startup, not polled per-tick
    pub poll_errors: Vec<String>,
    pub connected: bool,
    pub initializing: bool,
}
```

`Mode` has no `Default` impl in `radio_trait.rs` — `Ft991aDisplay::default()`
picks `Mode::Usb` explicitly (matches ts570d's own default-mode choice, and
is the FT-991A's most common general-coverage default), not derived.

### 6.3 Screen layout — reuses `split_areas`'s 4-band vertical split

Same band structure as `ts570d/ui/src/layout.rs::split_areas` (header /
status / errors / controls), sized down:

- **Header** (3 rows): title `" FT-991A RADIO CONTROL "`, reused verbatim
  pattern from `draw_header`.
- **Status** (5-6 rows, vs. ts570d's 5-row *inner* layout across gains/
  receiver-features/flags/status-bar — this slice collapses to 2 rows since
  there's no receiver-features/flags row content to show yet):
  - Row 1 (dominant): `VFO A  <freq>  <mode>  S <bar> <label>  <TX/RX
    indicator>` — same visual weight/position as ts570d's row 1 (bold
    white frequency, first line, left-anchored), not the emulator's
    separate block-glyph LCD font (`emulator/tui.rs`'s `big_digit` — a
    different binary's aesthetic, not `ui`'s convention; noted as a
    possible Wave 3+ polish, not required now).
  - Row 2: `VFO B  <freq>` plus `AF:[bar] RF:[bar] SQL:<val> PWR:<watts>W
    PS:<ON/OFF>` mini-bar row, same `mini_bar`/bracket-style rendering as
    ts570d's row 2/gains row, collapsed into one row since there's no
    mic-gain/AGC/receiver-feature content this slice.
  - `ID: <value>` shown small, right-aligned on the header or status row
    (fetched once, never changes) — no dedicated row needed.
- **Errors** (3-5 rows): reused verbatim from `draw_errors`/
  `draw_disconnected` — connection-health display is command-count-
  independent (see 6.1).
- **Controls** (remaining): flat single-column keybinding list (see 6.5) in
  `Normal` state; switches to the `TextInput`/`ListSelect`/`Feedback`
  overlay (ts570d's existing 3-line `lines[0..3]` layout, reused verbatim —
  that rendering is state-shape-driven, not group-count-driven) when active.

### 6.4 Flat `ControlState` state machine

```rust
pub enum ControlState {
    #[default]
    Normal,                                  // replaces Menu — no GroupMenu layer
    TextInput { prompt: String, buffer: String, error: Option<String>, action: InputAction },
    ListSelect { options: Vec<String>, cursor: usize, action: SelectAction },
    Feedback { message: String, is_error: bool },
}

enum InputAction { SetVfoA, SetVfoB, SetAfGain, SetRfGain, SetSquelch, SetPower }
enum SelectAction { SetMode }   // only one this slice — kept as an enum for symmetry/extensibility

enum ExecuteAction {
    SetVfoA(u64), SetVfoB(u64), SetMode(Mode), ToggleTx,
    SetAfGain(u8), SetRfGain(u8), SetSquelch(u8), SetPower(u8),
    TogglePowerOn,
}
```

`handle_key` in `Normal` dispatches directly off a single flat descriptor
list (9 `CommandKind` entries, see 6.1) — no `select_group_command`
indirection, no `Esc`-to-parent-menu (there is no parent menu; `Esc` from
`TextInput`/`ListSelect` returns straight to `Normal`, same as ts570d's
`Esc` → `Menu`).

### 6.5 Keybindings (flat, 1 key ≈ 1 command)

| Key | Action | State produced | Validates |
|-----|--------|-----------------|-----------|
| `F` | Set VFO A | `TextInput(SetVfoA)` | 0.030–470.000 MHz (Frequency::MIN_HZ/MAX_HZ — **not** ts570d's 0.5-60 MHz range) |
| `B` | Set VFO B | `TextInput(SetVfoB)` | same range as `F` |
| `M` | Set mode | `ListSelect(SetMode)`, 14 options | `Mode`'s hex-nibble set (LSB..C4FM, manual p.11) |
| `T` | Toggle CAT TX/RX | Immediate `ToggleTx` | see 3-valued-`TxState` handling below |
| `A` | Set AF gain | `TextInput(SetAfGain)` | 0-255 |
| `R` | Set RF gain | `TextInput(SetRfGain)` | 0-255 |
| `S` | Set squelch | `TextInput(SetSquelch)` | 0-100 (**not** 0-255 — SQ's own range, per `ft991a.rs` doc comment) |
| `P` | Set TX power | `TextInput(SetPower)` | 5-100 watts (PC) |
| `O` | Toggle power on/off | Immediate `TogglePowerOn` | boolean; see PS wake-quirk note below |
| `Q` | Quit | — | — |

No key for `SM` (S-meter) or `ID` — both read-only and always visible in
the status band (6.3), matching how ts570d never binds a key to values it
only displays (e.g. its S-meter).

**3-valued `TxState` handling** (the one place this trait genuinely departs
from ts570d's write-only TX/RX and needs bespoke UI logic, not a copy):
display renders `TxState::Off` as green `RX`, `TxState::CatKeyed` as red
`TX`, and `TxState::RadioKeyedNonCat` as **distinct** yellow `TX (ext)` —
this session didn't key it, some other cause did (front panel, VOX,
footswitch — manual p.17). The `T` key always sends `transmit()`/
`receive()` (`TX1;`/`TX0;`) based on whether the *last polled* state was
`CatKeyed` (→ send `receive()`) or anything else (`Off` or
`RadioKeyedNonCat` → send `transmit()`). Pressing `T` while
`RadioKeyedNonCat` does not clear the non-CAT key source (manual doesn't
document a command that would) — this is flagged as a known, inherent
limitation of the 3-valued protocol design itself, not a UI bug, and is
not blocking.

**`PS`/power-on wake-sequence note**: `radio::Ft991a::set_power_on` is
documented (`ft991a.rs` lines 298-309) as a faithful 1:1 `PS<0/1>;` mapping
that deliberately does *not* implement the manual's "dummy data, then
wait 1-2s, then `PS1;`" wake-from-standby sequence — that's an explicitly
deferred `wake_and_power_on()` helper, not yet on the `Radio` trait. The
`O` key in Wave 2 therefore just calls `radio.set_power_on(!power_on)`
directly; if the radio is in deep standby this may not wake it. This is an
already-flagged, inherited limitation from the `radio` crate (§4 above),
not something `ui` needs to solve — `ui`'s task should note it in a doc
comment, not silently paper over it or invent an undocumented delay.

### 6.6 Polling loop

Mirrors `ts570d::ui::terminal::run`'s shape (`terminal.rs` lines 130-160,
2101, 2143, 2209, 2254) at reduced call count:

- `run<R: Radio + 'static>(radio: R) -> UiResult<()>` — **same signature**
  as the current placeholder (`ui/src/lib.rs` line 49; takes the radio *by
  value*, not `&mut`), so the `app` follow-up task's `main.rs` call site
  (`ui::run(radio).await`) does not need to change shape.
- On entry: one `get_id()` call (with retry-until-success folded into the
  `initializing` flag, same pattern as ts570d's first-poll-cycle handling)
  — `ID` is fetched once, not on every tick, since it's a fixed constant
  (`"0670"`) per `ft991a.rs`'s doc comment.
- `poll_radio_state`: every 200ms (same cadence as ts570d), sequentially
  await `get_vfo_a`/`get_vfo_b`/`get_mode`/`get_tx_state`/`get_smeter`/
  `get_power_on`/`get_af_gain`/`get_rf_gain`/`get_squelch`/`get_power` (10
  calls vs. ts570d's larger poll set — proportional to the smaller trait
  surface), recording per-call errors into `poll_errors` and updating
  `connected`/`initializing` the same way ts570d does (3 consecutive failed
  cycles → `connected = false`).
- Key-event polling: same 10ms `event::poll` / 5ms idle-sleep cadence
  (`terminal.rs` lines 2209, 2254) — this loop-timing detail is unrelated
  to command-set size, reuse unchanged.

### 6.7 File layout (`ui/src/`)

- `lib.rs` — `UiError`/`UiResult` (already exist), `Ft991aDisplay` (new,
  replaces nothing — Wave 1 stub has no display struct), re-export `run`.
- `layout.rs` — render functions per 6.3 (`split_areas`, `draw_header`,
  `draw_errors`, `draw_disconnected` ported near-verbatim; `draw_status`
  replaces `draw_ui` with the collapsed 2-row body; `draw_control_panel`
  simplified to `Normal`/`TextInput`/`ListSelect`/`Feedback` only, no
  `GroupMenu`/`Diagnostic` arms).
- `control.rs` — `ControlState`/`InputAction`/`SelectAction`/
  `ExecuteAction`/`handle_key`/validation per 6.4-6.5. No
  `CommandGroup`/`group_commands`/`select_group_command`/
  `group_command_labels`/`initial_list_cursor`'s multi-group branching —
  one flat descriptor list instead of eight.
- `terminal.rs` — `run`/`poll_radio_state`/terminal setup-teardown per 6.6,
  structurally mirroring ts570d's (raw mode, alternate screen, panic-safe
  restore) — this part is genuinely command-count-independent.
- No `diag.rs` (see 6.1).

### 6.8 `ui/Cargo.toml` additions needed

Current placeholder (`ui/Cargo.toml`) depends on `radio`, `monoio`,
`thiserror`, dev-dep `async-trait`. The real TUI additionally needs
`ratatui`/`crossterm` (already pinned in the workspace root `Cargo.toml`'s
`[workspace.dependencies]`, just unused by `ui` until now) — add both as
real `[dependencies]`, matching `ts570d/ui/Cargo.toml`'s shape exactly
(`radio`, `monoio`, `thiserror`, `ratatui = { workspace = true }`,
`crossterm = { workspace = true }`). No transport crate, per CLAUDE.md rule
4 — unchanged from the placeholder's compliance.

---

## 7. Wave 2 — `emulator` crate design

### 7.1 Mirrors `ts570d/emulator` closely — this is infrastructure, not a
command-count-scaled design

Per this dispatch's brief: an emulator's job (host a fake radio behind a
PTY for out-of-process testing) is the same regardless of how many CAT
commands the radio-under-test supports. Read `ts570d/emulator/src/{emulator.rs,
main.rs,lib.rs,pty.rs,io.rs,port.rs,logger.rs,tui.rs}` and
`ts570d/emulator/Cargo.toml` in full for this section — confirmed each
file's actual genericity (not assumed):

| File | Radio-specific? | Action |
|------|------------------|--------|
| `Cargo.toml` | No (only `description` field mentions TS-570D) | Copy, retarget `description` to FT-991A, `radio = { path = "../radio" }` unchanged in shape |
| `lib.rs` | No — `pub mod` list + `EmulatorError{Pty,Io}` | Copy verbatim |
| `pty.rs` | No — `PtyPair` wraps `TTYPort::pair()`, no radio types referenced | Copy verbatim |
| `io.rs` | No — `CommandFramer`/`EmulatorIo` operate on raw `;`-terminated byte frames, no radio types referenced | Copy verbatim |
| `port.rs` | No — `PortMode`/`parse_port_arg`/`open_port`, no radio types referenced, **except** one hardcoded physical-mode baud (`serialport::new(path, 4800)`, line 65) | Copy structurally; **flag** the hardcoded `4800` as a value the `emulator` agent must reconsider — FT-991A's own default is 9600 (manual p.7/p.8, menu 029/031, matches `app`'s `main.rs` `--baud` default) unlike ts570d's apparent 4800 default; verify against the FT-991A manual, don't silently inherit ts570d's number |
| `logger.rs` | No — `StateChange{field: &'static str, value: String}` / `LogEvent::{Startup,Command,StateChange}` NDJSON shape is already fully generic; matches the already-designed `Ft991aEvent{field,value}` shape (§3 above, and confirmed identical in the landed `radio/src/ft991a_radio.rs`) | Copy verbatim |
| `main.rs` | No — imports only `Emulator`, `BackgroundLogger`, `port` (no radio-specific type reference at all); `--tui`/`--background`/`--log-file`/`--port` CLI parsing, `PTY_SLAVE=`/`Connected to` stdout lines, `ctrlc` handler | Copy verbatim |
| `emulator.rs` | **Yes** — `Ts570dRadio` → `Ft991aRadio`, `CatFramework<Ts570dRadio>` → `CatFramework<Ft991aRadio>` (2 type-name substitutions); `Emulator::new`/`from_port`/`run`/`run_background`/`run_with_tui`/`log_entry`/`tui_loop` method bodies are otherwise unchanged — they only touch `CatFramework<R>`/`EmulatorIo`/`PtyPair`/`BackgroundLogger`, all already radio-generic | Copy structure, swap the 2 type names |
| `tui.rs` | **Yes** — renders `Ts570dState` fields (`vfo_a_hz`, `active_vfo`, `tx`, `smeter`, `power_control`, `mode` 1-9, RIT/XIT/split/antenna/AGC/noise/etc. annunciators) | Rewrite against `Ft991aState`'s actual (smaller) field set — see 7.3 |

**Net delta from ts570d/emulator: 2 files need real rewriting
(`emulator.rs`'s 2 type substitutions, `tui.rs`'s field set), 7 files are
copy-verbatim modulo the one flagged baud-default check in `port.rs`.**
This confirms the brief's framing — emulator infrastructure genuinely does
not scale with command-table size.

### 7.2 `emulator/Cargo.toml`

Same dependency list as `ts570d/emulator/Cargo.toml`: `radio = { path =
"../radio" }`, `cat-framework = { workspace = true }`, `thiserror`,
`serde`+`serde_json`, `serialport = "4"` (a **non-workspace, direct**
pin — same as ts570d; note this is a *different* transport-adjacent crate
from this repo's `cat-transport-serial`, used only for its `TTYPort::pair()`
PTY-creation primitive, not as a `CatSession`/`Transport` impl — the
emulator hosts the *server* side of the PTY, `cat-transport-serial` is for
the *client*/`app` side connecting to a real or virtual port; no conflict
with CLAUDE.md rule 2/4, since `emulator` is the one crate CLAUDE.md's
architecture diagram explicitly permits transport-layer code in), `ctrlc =
"3.0"`, `ratatui`/`crossterm` (workspace). Dev-deps `tempfile`/`mockall`/
`libc` (workspace).

### 7.3 `tui.rs` rewrite against `Ft991aState`

Same three-column layout convention (meter column | LCD | command/log
panel) and same low-level helpers (`bargraph`, `tick_label_line`,
`big_digit`/`render_big_freq` block font, `on_style`) — these are
visual-language helpers, not TS-570D-specific. Redesigned per-field:

- Meter column: `draw_rx_smeter` unchanged in shape (`state.smeter` exists
  on `Ft991aState` too, same `u8` 0-255... **note range**: ts570d's
  `smeter` is documented 0-30 scale (`smeter_label` match arms cap at 30);
  FT-991A's `SM` is 0-255 (`ft991a.rs` line 274) — the label/tick-mapping
  table needs its own FT-991A-scaled thresholds, not a direct port of
  ts570d's `smeter_label`/tick constants. TX-side meter column
  (`draw_tx_meters`, PWR/SWR bars) has no FT-991A equivalent yet (no `RM`
  meter-read command in this slice, see §2's out-of-scope list) — simplest
  correct choice: when `state.cat_tx == 1`, show `PWR: <power_control>W`
  only (no SWR bar — SWR isn't in this slice's command table at all,
  unlike ts570d where `RM` backs it), not a fabricated SWR reading.
- LCD column: annunciator lines reduced to what `Ft991aState` actually has
  (`power_on`, `cat_tx`) — no RIT/XIT/split/antenna/AGC/noise-blanker/etc.
  annunciators (none of those fields exist on `Ft991aState`). Frequency
  block: FT-991A's 9-digit/470MHz range needs its own `format_freq_ascii`
  (ts570d's caps at 2-digit MHz, matching its 60MHz range — **not**
  reusable as-is for FT-991A's 3-digit MHz range up to 470). Mode row:
  FT-991A's hex-nibble `Mode` (`radio::Mode::name()`, 14 values) replaces
  ts570d's 1-9 `match`.
- Command/log panel: `lookup_description`'s command-code lookup needs
  retargeting to `FT991A_COMMAND_TABLE.find(code).map(|c| c.description)`
  (11 codes instead of ts570d's table), same `format_log_line`/
  `extract_command_code` pattern otherwise unchanged.

### 7.4 CLI shape — identical to `ts570d/emulator`

`--tui` / `--background` / `--log-file <path>` / `--port [virtual|<path>]`,
mutually-exclusive `--tui`/`--background` check, `PTY_SLAVE=<path>` (or
`Connected to <path>`) as the first stdout line — all copied verbatim via
`main.rs` (7.1 table). No FT-991A-specific CLI surface needed this wave.

---

## 8. Dispatch queue — Wave 2

Two independent implementation tasks plus one sequential follow-up.

### Task 3 — `ui` agent: real ratatui TUI

Scope: `ui/src/*.rs` + `ui/Cargo.toml`. Deliver the design in §6: flat
`Ft991aDisplay`/`ControlState`/`layout.rs`/`control.rs`/`terminal.rs`, the
9-keybinding table (§6.5), 3-valued-`TxState` rendering (§6.5), and the
`ratatui`/`crossterm` `Cargo.toml` additions (§6.8). Must write
`planning/ui/task_plan.md` before code, per the Architect Review Workflow.
Depends only on `radio` (already landed, `e3698cf`) — does not touch
`emulator/` or `src/`.

### Task 4 — `emulator` agent: PTY-hosted `CatFramework<Ft991aRadio>`

Scope: new `emulator/` crate (not yet a workspace member — this task adds
it to root `Cargo.toml`'s `members` list, since `emulator` owns its own
`Cargo.toml`, unlike `ui`/`radio` which `app` already scaffolded). Deliver
the design in §7: copy-verbatim the 7 generic files (flagging and
resolving the `port.rs` baud-default question against the FT-991A manual,
not silently inheriting ts570d's `4800`), retarget `emulator.rs`'s 2 type
names, and rewrite `tui.rs` against `Ft991aState`'s actual field set (§7.3).
Must write `planning/emulator/task_plan.md` before code. Depends only on
`radio` (already landed) and `cat-framework` — does not touch `ui/` or
`src/`.

### Parallel-vs-sequential call: **dispatch Task 3 and Task 4 in parallel**

Checked, not assumed: neither task's deliverable depends on the other's
output.
- `ui` depends on `radio::Radio`/domain types only (CLAUDE.md rule 4 — `ui`
  never imports a transport crate, and `emulator` is not a transport crate
  it would need anyway). `ui`'s `Cargo.toml` has no `emulator` dependency
  and no reason to gain one.
- `emulator` depends on `radio`/`cat-framework` only (CLAUDE.md's
  dependency diagram — `emulator` has no `ui` dependency).
- Both crates' Wave-1-stable dependency (`radio`) already landed and was
  reviewed in `e3698cf` — neither task is blocked on a moving API.
- File scopes don't overlap: `ui/` vs. `emulator/` (plus one shared edit
  each will need to make to root `Cargo.toml`'s `members` list — a
  git-mergeable one-line-add each, not a real collision; if both agents
  run concurrently the second to finish should re-check `members` before
  writing, same low-risk pattern as any two-branch merge).
- The one place they'd actually need to talk to each other — running the
  real `ui` against the real `emulator` end-to-end over a live PTY — is
  explicitly **not** part of either task's Wave 2 deliverable (unit/
  in-crate tests only, mirroring how Wave 1 tested `radio` without a PTY);
  that end-to-end wiring is Task 5's job, after both land.

Sequencing rule from the Architect Review Workflow still applies within
each task (plan → review → code, one task at a time *per subagent*), but
across the two subagents there is no ordering dependency — dispatch both,
review both independently.

### Task 5 — `app` agent (sequential follow-up, after Tasks 3 and 4 both land and are reviewed)

Scope: root `Cargo.toml` (add `emulator` to `members` if not already merged
cleanly by Task 4, add `emulator` as a `[dev-dependencies]` entry per the
architecture originally sketched in §1), `src/main.rs` (replace the
`ui::run(radio).await` call's target — the *call site* doesn't change
shape per §6.6's signature-stability note, but the `ui` crate behind it is
now real). This task is explicitly sequential, not parallel with 3/4: its
whole point is wiring the two Wave-2 deliverables together, so it cannot
start until both exist and have been reviewed.

---

## 9. ADR decision for Wave 2

**No new ADR.** Checked `ts570d/docs/adr/README.md` for precedent before
deciding, per this dispatch's instruction: ts570d's own 5 ADRs (0001
generic CAT framework, 0002 domain types in `radio`, 0003 single command
table, 0004 extraction boundary, 0005 network transport readiness) contain
**no ADR for ts570d's own UI design** — its menu-tree/control-state-machine
shape, LCD-style display convention, and keybinding scheme were never
ADR'd, only ever implemented directly. A UI-scope decision here is exactly
that same category of decision (implementation-shape, not an
architectural/cross-cutting boundary decision like "where do domain types
live" or "single vs. multiple command tables" — the things ts570d *did*
ADR). Recording the §6/§7 design in this file is sufficient, consistent
with precedent, and avoids ADR sprawl for a decision this repo's own
sibling never treated as ADR-worthy.

`docs/adr/README.md`'s "Repository status" paragraph is updated (see next
commit) to note Wave 1 landed and Wave 2 dispatched, without adding a table
row — the existing table only gained a row when a genuine cross-cutting
decision needed one (ADR 0001, the unblocking decision itself); Wave 2's
UI/emulator shape isn't that.

---

## Next steps (Wave 3+, not dispatched yet)

1. Review Wave 2 output (Tasks 3 and 4, independently) with the user.
2. Dispatch Task 5 (`app`, sequential) once both are approved.
3. End-to-end smoke test: real `ui` against the real `emulator` over a live
   PTY (`cargo run --bin emulator -- --port virtual`, then `cargo run --bin
   ft991a -- --port <printed PTY_SLAVE path>`) — first time this repo's `ui`
   and `emulator` actually talk to each other; do this after Task 5, not as
   part of Tasks 3/4.
4. `yaesu` agent, follow-on waves (unchanged from Wave 1's list): `IF`
   (careful page-10 re-verification), `RM`/`RI`, attenuator/preamp/noise/
   clarifier/scan/VOX, memory channels, the 153-entry `EX` menu table, and
   the remaining commands listed in §2's "explicitly out of scope" list —
   each its own reviewed task, not bundled. Growing the command table in a
   later wave will also grow `ui`'s keymap and `Ft991aDisplay`/
   `Ft991aState`/`tui.rs` — revisit §6.1's "flat vs. grouped" call at that
   point (explicitly not a one-time decision; the trigger for introducing
   a group layer is command count, not a fixed wave number).

---

## 10. Wave 3 — full CAT coverage + RTS/DTR PTT/CW-keying (2026-07-18)

### 10.0 Scope of this session

The user asked to keep going "until we have a fully functional emulator, a
control interface that manages all options, not JUST the CAT but the other
serial protocol as well." Two work items: (A) full CAT command-table
coverage (currently 11 of 91 top-level commands), and (B) a genuinely new
capability — PTT/CW keying via the RS-232/USB **hardware modem control
lines** (RTS/DTR), as an alternative to the `TX;`/`TX1;` CAT-command path.
This section is planning only (no code written, per the architect's
standing prohibition), and records design + a dispatch queue. It does not
dispatch anything itself — the coordinating session dispatches from this.

### 10.1 Sources read this session

- `radio/src/ft991a_radio.rs`, `radio/src/ft991a.rs`, `radio/src/
  radio_trait.rs`, `src/main.rs` (full, current landed Wave-1 code, not the
  Wave-1 sketch above — confirms `Ft991a<S: CatSession>` wraps
  `SharedSession<S>`, `main.rs` moves `SerialPort` into
  `SerialCatSession::new(port)` before `Ft991a::new(session)`).
- `radio-cat-rs/cat-transport-core/src/{transport.rs,session.rs,errors.rs,
  lib.rs}`, `radio-cat-rs/cat-transport-serial/src/{io_uring.rs,session.rs,
  lib.rs}`, `radio-cat-rs/cat-transport-core/Cargo.toml`,
  `radio-cat-rs/cat-transport-serial/Cargo.toml` (full) — confirmed neither
  `Transport` nor `SerialPort` exposes runtime RTS/DTR control today (only
  a one-time `TIOCMBIS` assert-both-high at `SerialPort::open`, comment
  explains it's there for TS-570D's "RTS = receive enable" convention);
  confirmed `SerialCatSession<T>.transport` is a **public** field; confirmed
  `cat-transport-core` has no `libc`/`nix` dependency today (trait
  signatures only, no ioctl bodies, keeps it that way).
- `radio-cat-rs/docs/adr/0001-scope-and-crate-boundaries.md` (full, +
  amendments) — crate boundary rules and the `cat-transport-serial` "also
  owns the concrete io_uring serial port implementation" amendment.
- `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` — re-read in full (all 20
  pages), this time transcribing every command in the p.3 master table and
  every per-command Set/Read/Answer/parameter box on p.4-18, plus the p.1
  RS-232C pinout table and the p.7-9 `EX` menu table (all 153 rows).

### 10.2 RTS/DTR PTT/CW-keying — corrections to the brief, from the manual

Two factual corrections to the brief's framing, found by reading the manual
directly rather than trusting the summary (as instructed):

1. **The RS-232C "CAT" 9-pin connector does NOT expose a DTR pin.** Manual
   p.1's pinout table (9-pin D-sub, "Figure 1"): pin 1 N/A, pin 2 SERIAL OUT
   (output), pin 3 SERIAL IN (input), pin 4 N/A, pin 5 GND, pin 6 N/A,
   **pin 7 RTS**, **pin 8 CTS**, pin 9 N/A. There is no DTR or DSR/DCD pin
   at all on this connector — only RTS (computer→radio) and CTS
   (radio→computer) are wired alongside the two data lines and ground.
   **Practical consequence**: RTS-based PTT/keying works over the existing
   single RS-232C CAT connection this app already opens; DTR-based
   PTT/keying is NOT reachable over that same physical connector at all —
   only over the USB Dual-UART bridge's virtual COM port(s), where DTR is a
   normal USB-CDC virtual signal regardless of the DB9's physical wiring.
   This is manual-cited (p.1); the "two virtual COM ports over USB"
   framing from the brief remains **not** manual-cited (confirmed — this
   20-page CAT-only manual never mentions USB port count or "Enhanced"/
   "Standard" naming anywhere), exactly as the brief warned.

2. **There is no single menu item "048 PTT SELECT."** The brief's citation
   of "048 PTT SELECT / 060 PC KEYING" is half right. Reading the full `EX`
   table (p.7-9): item **060 "PC KEYING"** exists exactly as described
   (`0:OFF 1:DAKY 2:RTS 3:DTR`, 1 digit) — this is the global "how does the
   radio interpret RTS/DTR toggling from the CAT computer as CW keying"
   setting, and it is **not** tied to a port-select sibling; nothing in the
   manual scopes it to a particular physical connector, and it sits on the
   same CAT connection the app already has open. But item **048** is
   actually **"AM PORT SELECT" (`0:DATA 1:USB`, 1 digit)** — part of a
   different family entirely: items **047 "AM PTT SELECT"**, **071 "DATA
   PTT SELECT"**, **076 "FM PKT PTT SELECT"**, **108 "SSB PTT SELECT"**
   (each `0:DAKY 1:RTS 2:DTR`, 1 digit) are four separate,
   *mode-specific* PTT-select items, each paired with its own port-select
   sibling (**048/072/077/109**, each `0:DATA 1:USB`) that chooses whether
   that mode's RTS/DTR line lives on the rear 6-pin **DATA** jack or on the
   **USB** audio/control virtual port — a *different physical connector*
   from the CAT RS-232C port, used for soundcard-driven digital-mode/voice
   PTT, not CAT control. This app has no other reason to open that
   connector (no soundcard/audio I/O anywhere in scope) — wiring up
   047/071/076/108 would require opening and controlling a port this
   application otherwise never touches, for zero benefit until a future
   soundcard-integration wave exists to use it.

   **Scope decision, following from this correction**: this wave's RTS/DTR
   feature is **CW keying via item 060 "PC KEYING," over the existing
   single CAT serial connection** — not the general "PTT SELECT" concept
   the brief's phrasing suggested. This is still exactly what was asked for
   ("PTT/CW keying via RTS/DTR... as an alternative to CAT-command-based
   TX/keying" — for CW mode specifically, asserting the PC-KEYING line does
   both key the CW element *and* assert transmit, functionally replacing
   `TX1;`/`TX0;` for that mode). The 047/071/076/108 family is recorded
   here as an explicit non-goal for this wave, not silently dropped — a
   natural fit for a future soundcard/digital-mode wave that would need to
   open the DATA/USB audio port anyway.

### 10.3 RTS/DTR capability — crate placement and API shape

**Placement, checked against `radio-cat-rs`'s own boundary rules
(`docs/adr/0001-scope-and-crate-boundaries.md`), not assumed**: this is
transport-level infrastructure (any serial-connected radio in either repo
could use it — `ts570d` already relies on RTS being driven high, per the
`SerialPort::open` comment), so it belongs in `radio-cat-rs`'s
`cat-transport-core` (trait signature) + `cat-transport-serial` (concrete
implementation) — **not** duplicated locally in this repo. **This means
Wave 3 has a cross-repo dependency: `radio-cat-rs` needs new code before
`ft991a`'s consumption task can build against it — dispatch that task
first, to `radio-cat-rs`'s own agents, not to this repo's `yaesu`/`app`
agents.** This repo's architect cannot dispatch into `radio-cat-rs` (out of
scope, different repo, different agent roster) — the coordinating session
needs to route this task there directly.

**API shape — a new trait, never added to the base `Transport`/`CatSession`
traits**:

```rust
// cat-transport-core/src/modem.rs (new file), re-exported from lib.rs.
// No libc/nix dependency needed here — signatures only, no ioctl bodies.

/// Optional capability for a serial-backed [`Transport`]/[`CatSession`]:
/// direct control of RS-232 modem control lines (RTS, DTR) and status
/// lines (CTS, DSR, DCD), independent of byte-level CAT framing.
///
/// Not every transport has physical modem control lines — TCP/UDP
/// sessions have none — so this is a separate, additively-implemented
/// trait, never folded into the base `Transport`/`CatSession` traits. A
/// radio crate that wants this bounds its own methods on
/// `S: CatSession + ModemControlLines` rather than requiring it
/// universally (see §10.4).
///
/// All methods are plain sync `fn`s, not `#[async_trait]` — these are
/// direct `ioctl(2)` calls with no I/O wait, matching the precedent
/// already set by `Transport::flush_rx`/`CatSession::flush_rx` (both
/// plain sync fns on otherwise-async traits, for the same reason).
pub trait ModemControlLines {
    fn set_rts(&self, asserted: bool) -> Result<(), TransportError>;
    fn set_dtr(&self, asserted: bool) -> Result<(), TransportError>;
    fn read_cts(&self) -> Result<bool, TransportError>;
    fn read_dsr(&self) -> Result<bool, TransportError>;
    fn read_dcd(&self) -> Result<bool, TransportError>;
}
```

Implementation, in `cat-transport-serial` (already depends on `libc`/`nix`
— no new dependency):

- `impl ModemControlLines for SerialPort` in `io_uring.rs`, using
  `TIOCMBIS`/`TIOCMBIC` (set/clear a bit) and `TIOCMGET` (read the status
  register) — the exact same `libc::ioctl` mechanism `SerialPort::open`
  already prototypes for its one-time RTS+DTR-high assert (lines 269-286
  today), generalized to runtime `&self` calls instead of a
  constructor-only side effect. `TIOCM_RTS = 0x004`, `TIOCM_DTR = 0x002`
  are already named as local consts in `open`; add `TIOCM_CTS = 0x020`,
  `TIOCM_DSR = 0x100`, `TIOCM_CAR (DCD) = 0x040`.
- A blanket delegating `impl<T: Transport + ModemControlLines>
  ModemControlLines for SerialCatSession<T>` in `session.rs`, forwarding to
  `self.transport` (the same delegation shape `SerialCatSession`'s
  `CatSession::flush_rx` already uses for `Transport::flush_rx`).
- **Flag, not required this wave**: `SerialPort::open`'s existing
  behavior unconditionally asserts RTS+DTR **high** at open time (comment:
  "TS-570D uses RTS as receive enable"). Now that a second consumer wants
  active runtime control over RTS specifically for keying (where the
  idle/asserted polarity matters and isn't manual-cited either way), the
  `radio-cat-rs` dispatch should consider adding `initial_rts: bool` /
  `initial_dtr: bool` fields to `SerialConfig` (default `true`, preserving
  today's behavior exactly) so a consumer can opt out of the open-time
  assert if PC-KEYING's expected idle polarity turns out to require it.
  Recorded as a "should consider," not a hard requirement — no evidence
  either way in this manual.

### 10.4 `ft991a`-side consumption design

Once `cat-transport-core`/`cat-transport-serial` ship `ModemControlLines`
(§10.3), `ft991a`'s `radio` crate adds, mirroring the existing
`SharedSession<S>` delegation pattern in `radio/src/ft991a.rs` exactly:

```rust
// radio/src/ft991a.rs — new blanket impl alongside the existing CatSession one.
impl<S: ModemControlLines> ModemControlLines for SharedSession<S> {
    fn set_rts(&self, asserted: bool) -> Result<(), TransportError> {
        let session = self.take();
        let result = session.set_rts(asserted);
        self.put_back(session);
        result
    }
    // set_dtr/read_cts/read_dsr/read_dcd: identical take/put_back shape.
}

// New impl block, additive — does NOT replace or narrow the existing
// `impl<S: CatSession<Error = TransportError>> Ft991a<S>` block above it.
impl<S> Ft991a<S>
where
    S: CatSession<Error = TransportError> + ModemControlLines,
{
    /// Assert/clear RTS — usable for CW keying when Menu item 060 "PC
    /// KEYING" is set to RTS (manual p.8). Present on the RS-232C CAT
    /// connector (pin 7, manual p.1).
    pub fn assert_rts(&self, asserted: bool) -> RadioResult<()> { ... }
    /// Assert/clear DTR — usable when Menu item 060 is set to DTR.
    /// **Not present on the RS-232C 9-pin CAT connector** (no DTR pin,
    /// §10.2) — only reachable via a USB connection, unverified against
    /// this manual (see §10.2's non-manual-cited flag).
    pub fn assert_dtr(&self, asserted: bool) -> RadioResult<()> { ... }
    /// Read CTS (present on the RS-232C connector, pin 8).
    pub fn read_cts(&self) -> RadioResult<bool> { ... }
    // read_dsr/read_dcd: exposed for completeness/future transports, same
    // "not on this connector" doc-comment caveat as assert_dtr.
}
```

This is **additive only** — it does not touch the existing `Radio` trait
or the existing `impl Radio for Ft991a<S>` block (which stays bounded on
`S: CatSession<Error = TransportError>` alone, so mock/fake test sessions
that don't implement `ModemControlLines` keep working unchanged). The new
methods are **inherent methods on `Ft991a<S>`**, not part of the `Radio`
trait — reachable only when the concrete `S` happens to satisfy both
bounds, decided at the wiring layer (`app/src/main.rs`), consistent with
`CLAUDE.md` rule 5 ("`app/main.rs` is the ONLY place concrete types are
wired together"). `ui` does not gain a generic way to call these through
the `Radio` trait this wave — see the follow-up note at the end of this
subsection.

**Why the existing single-`SerialPort` wiring already "just works" for the
RS-232C case, with zero `main.rs` changes**: `main.rs` today does
`Ft991a::new(SerialCatSession::new(port))` where `port: SerialPort`. Since
`SerialPort: ModemControlLines` (§10.3) and `SerialCatSession<T: Transport +
ModemControlLines>: ModemControlLines` (blanket delegation, §10.3), the
resulting `Ft991a<SerialCatSession<SerialPort>>` automatically satisfies
the new impl block's bound — **`assert_rts`/`read_cts` become callable with
no CLI flag, no second constructor, and no `main.rs` edit at all.** This
directly answers the brief's question in point 2: for a first
implementation, **RTS-on-the-same-port is sufficient; no `--port2` is
needed.**

**USB dual-port case: explicitly deferred, not designed this wave.**
Reasoning: the community-documented "two virtual COM ports over USB"
convention isn't manual-cited (§10.2) and isn't needed for the RS-232C
case above. Supporting a genuinely separate physical device for modem
control (as USB's second port, or the 047/071/076/108 family's DATA/USB
audio port would require) means `Ft991a` can no longer assume "the CAT
session's own transport is also the modem-control handle" — it would need
a **second, independent handle** (a generic `M: ModemControlLines` type
parameter, or a `Box<dyn ...>` field, supplied separately from `S:
CatSession` at construction). That's real added complexity with no
concrete requirement driving it yet (this app has no USB dual-port target
to test against, and the RS-232C path already satisfies the ask) — deferred
to a future wave if/when USB dual-port support is actually requested, not
designed speculatively now.

**Follow-up not scoped into this task**: giving `ui` a keybinding that
actually triggers `assert_rts` during CW send is a small separate `ui`/
`app` task, appropriately dispatched *after* this radio-crate task lands
and is reviewed (mirrors Wave 1's "plan → review → code" gate) — not
bundled into the same task, matching how Wave 1's `yaesu` task didn't touch
`ui` either.

### 10.5 Full CAT command coverage — batch breakdown

Re-transcribed the full p.3 master table: **91 top-level commands total**
(`EX` counted once here — see §10.6 for why it isn't "1 of 91" in the naive
sense). 11 are implemented (`FA FB MD TX SM PS AG RG SQ PC ID`). The
remaining **79 non-`EX` commands** group into 10 batches below, sized
5-15 (two are smaller — 4 each — by deliberate choice, noted inline), each
an independently reviewable `yaesu` task against `radio/src/
ft991a_radio.rs` (+ `radio/src/ft991a.rs` client methods,
+ `radio/src/radio_trait.rs` `Radio` trait growth where the concept is
generic enough to belong there per `CLAUDE.md`'s "Radio trait scope").

Cross-batch finding worth calling out up front: **`IF` (p.10), `MR`/`MT`
(p.12), and `OI` (p.13) all share the identical composite payload shape** —
memory-channel-or-VFO number, frequency, clarifier direction/offset, RX/TX
clarifier on/off, mode, VFO/memory/QMB select, CTCSS/DCS status, offset
type (`IF`'s P2-P10 / `MR`'s and `MT`'s equivalent trailing fields / `OI`'s
P2-P10 are the same field sequence). Verifying that shape carefully once
(as `IF`, deferred from Wave 1 for exactly this reason) directly de-risks
`MR`/`MT` (batch 2) and `OI` (batch 10) — recommend a shared parser/struct
factored out of the `IF` task, reused by both. This changes the priority
order (batch 9, containing `IF`, should land before batch 2 and the tail of
batch 10 that needs it — see §10.7).

| # | Batch | Commands (count) | Notes |
|---|-------|-------------------|-------|
| 1 | VFO/split/memory quick-ops | `AB BA AM VM MA CH QI QR QS SV` (10) | Mostly zero/one-param triggers (VFO-A↔B swap, memory recall shortcuts). **Manual inconsistency to flag, not silently resolve**: the master table (p.3) names `VM` "[V/M] KEY FUNCTION," but `VM`'s own per-command box (p.18) is headed "VFO-A TO MEMORY CHANNEL" — identical to `AM`'s heading (p.4). Verify against real hardware or an errata before committing to either meaning. |
| 2 | Memory channel records | `MC MR MW MT` (4, deliberately small — high field-count, own careful pass like `IF`) | `MR`/`MT`'s answer rows are ~30-50 char composite records (memory number, freq, clarifier, mode, tags up to 12 ASCII chars for `MT`). Depends on batch 9's `IF` work landing first (shared field shape, see above). |
| 3 | Clarifier/RIT-XIT + tone squelch + IF-shift | `RT RC RD RU XT CN CT IS` (8) | `RT`=CLAR on/off, `RC`=clear, `RD`=down, `RU`=up (`RU`'s own heading says "RX CLARIFIER PLUS OFFSET" — cross-check against `RT`/`XT`'s RX/TX split), `XT`=TX clarifier on/off, `CN`/`CT`=CTCSS/DCS tone+mode (two lookup tables, p.6, 50 CTCSS tones + 104 DCS codes — transcribe both), `IS`=IF-shift. |
| 4 | Keyer/CW/break-in | `KM KP KR KS KY CS ZI BI SD` (9) | `KM` stores up to 50-char keyer memory messages (5 channels) — variable-length ASCII field, same "digits: n" variable-width shape as `EX`/`DT` (§10.6). `KY` (CW keying: play back a *stored* keyer memory via CAT) is related to but distinct from the RTS/DTR CW-keying feature (§10.2-10.4) — flag the distinction in the task (radio-side automatic memory playback vs. real-time PC-driven keying), don't conflate. |
| 5 | Scan/VOX/busy | `SC VX VD VG BY` (5) | `VD`'s parameter meaning depends on Menu item 142 "VOX SELECT" (MIC vs DATA) per its own doc note (p.17) — same "meaning depends on an EX menu setting" pattern `TX`'s 3-valued answer already established; state that dependency explicitly, don't hide it. |
| 6 | Attenuator/preamp/noise/AGC/notch/filter-width | `RA PA NB NL NR RL GT CO BP BC NA SH` (12) | `SH`'s p.16 bandwidth table (P2 00-21) differs by mode family (SSB/CW/RTTY-PSK, narrow/wide) — six-column lookup table, transcribe in full, don't approximate. |
| 7 | Speech processor/mic/monitor | `PL PR MG ML` (4, deliberately small — coherent "audio chain" theme) | |
| 8 | Band/step/encoder front-panel controls | `BS BU BD FS ED EU EK DN UP` (9) | `BS`'s p.5 16-band table (`00`=1.8MHz … `16`=430MHz, with a documented gap at `13`) — transcribe exactly, including the gap. **Manual inconsistency to flag**: `DN`'s master-table name is "DOWN" but its own per-command box heading (p.6) says "MIC DWN" — same category of mismatch as batch 1's `VM`/`AM`; `UP`'s two headings agree ("UP"). Verify, don't silently pick one. |
| 9 | Meters/status (do this batch early — see cross-batch note above) | `IF RM RI RS MS UL` (6) | `IF` re-verified column-by-column against the manual image (not just extracted text) per Wave 1's own deferral reasoning — this is the one Wave-1 flagged as needing special care, still true. `RM`'s meaning depends on `MS`'s current selection (COMP/ALC/PO/SWR/ID/VDD) — read `MS` first in the task. |
| 10 | Misc system/TX/tuner/DVS | `AC AI DA DT LK OI OS FT TS MX LM PB` (12) | `OI` benefits from batch 9's `IF` work (see cross-batch note). `DT` (date/time) has its own 3-shape variable P2 (date 8 digits / time 6 digits / offset 5 digits signed) selected by P1 — same "variable-width single command" pattern as `EX`, smaller-scale; a good rehearsal for the `EX` task's approach (§10.6). |

**Total accounted for**: 10 (b1) + 4 (b2) + 8 (b3) + 9 (b4) + 5 (b5) + 12
(b6) + 4 (b7) + 9 (b8) + 6 (b9) + 12 (b10) = 79, matching 91 − 11
(implemented) − 1 (`EX`, scoped separately in §10.6).

### 10.6 `EX` menu — actual structure (much smaller framework surface than "153 commands")

Confirmed from the manual (p.7, `EX` box + the full 153-row table on
p.7-9), not assumed: **`EX` is one composite command**, not 153 separate
2-letter codes.

- **Set**: `EX<3-digit menu number P1><variable-width P2>;` — e.g.
  `EX001+0020;`-shaped (P1 is always exactly 3 digits, `001`-`153`; P2's
  width is item-specific, per the table's own "Digits" column).
- **Read**: `EX<3-digit P1>;` → **Answer**: same shape as Set. This is
  another instance of Wave 1's "selector read" pattern (`MD`/`SM`): the
  3-digit P1 alone is structurally a `Set`-shaped frame to
  `cat-framework`'s parser, not a zero-width `Query` — reuse the same
  `handle_command`-disambiguates-by-parameter-length approach already
  established, now with **P1 itself** as the thing that must be parsed out
  and looked up (not just a fixed `"0"` selector).
- P2's width varies **1 to 8 digits** across the 153 items (confirmed
  present in the table: 1, 2, 3, 4, 5, and 8-digit fields — item 151
  "PRESET FREQUENCY" is the 8-digit outlier, `00030000-47000000` Hz; item
  027 "TIME ZONE" and items 064/065 are 5-digit signed values, `-hhmm` /
  `+hhmm` or `±3000` Hz style). Total wire width is therefore `3 + P2-width`
  — around **6 distinct total-length `CommandForm` entries** cover all 153
  items (not 153), confirming the brief's suspicion: **this is substantially
  less framework-level work than 153x a normal command.** The real cost is
  **domain modeling**: 153 items each need a correctly-typed
  value/range/encoding in a lookup table (`EX_MENU_TABLE: &[(u16 /* P1 */,
  ExMenuKind)]` or similar) that `handle_command`'s single `Ex` match arm
  consults to (a) find the item by P1, (b) validate P2 against that item's
  own expected width/range (the same "structural match succeeded, semantic
  validation still per-item" pattern `FA`'s range check already
  demonstrates — no new framework capability needed, confirmed by tracing
  through a deliberately-malformed-P2-width example during this session's
  design pass), (c) format the typed value back onto the wire.
- One field's encoding is **not resolvable from this manual alone**: item
  087 "RADIO ID" shows P2 as literally `----------` (dashes) in the table,
  with no digit count or format given anywhere else in the 20 pages. Flag
  as an open item for whichever `EX` sub-batch reaches item 087 — do not
  guess a width.

**Sizing recommendation**: split into sub-batches by menu-number range
(the table's own visual groupings roughly track themes — TX audio chain
045-079, RTTY/SSB TX chain 092-110, meter/scope 111-136, band-limit/VOX
137-153 are the rough shapes seen on p.7-9), each its own reviewable task
that (for the first sub-batch only) also builds the shared `EX`
command-table plumbing (the ~6 `CommandForm` widths + the `Ex` dispatch
arm's P1-lookup mechanism) that later sub-batches then just add table rows
to — low-risk, easily parallel-reviewable once the first sub-batch's
plumbing lands.

**Priority carve-out directly serving §10.2-10.4**: the first `EX`
sub-batch should be scoped to **just the plumbing + the 9 PTT/keying-
relevant items** — `047` (AM PTT SELECT), `048` (AM PORT SELECT), `060`
(PC KEYING — the one this wave's RTS/DTR feature actually needs), `071`/
`072` (DATA PTT/PORT SELECT), `076`/`077` (FM PKT PTT/PORT SELECT), `108`/
`109` (SSB PTT/PORT SELECT) — rather than starting at menu 001 and working
up numerically. This isn't a hard dependency for §10.4's RTS/DTR wire
capability (a user can set Menu 060 by hand on the front panel without any
CAT support existing), but it completes the feature end-to-end in
software and is cheap to front-load. The remaining ~144 items follow in
further sub-batches, lower priority than the core command batches (§10.5)
— they're settings, not live operating state — but interleavable at the
user's discretion.

### 10.7 UI/emulator growth — explicit flag, not designed this wave

Per Wave 2 §6.1's own stated trigger ("revisit when command count grows"):
**that trigger has now been pulled.** Going from 11 to ~90 top-level
commands plus a 153-item settings menu means `ui`'s flat 9-key `Normal` →
`{TextInput,ListSelect}` design (Wave 2 §6.1-6.7) will not fit — 90+
single-key bindings don't fit on one screen and aren't learnable, exactly
the failure mode Wave 2 correctly avoided designing around prematurely.

**Not designed in detail this session** (per the brief's instruction — flag
only, full design is its own future wave, after command coverage lands).
Rough shape recommendation:

- Reintroduce a **grouped menu layer**, similar in spirit to `ts570d`'s
  `Menu` → `GroupMenu` → `{TextInput,ListSelect}` three-level design
  (which Wave 2 explicitly declined to port down for 9 items, reasoning
  that no longer applies at ~90). Reuse §10.5's batch groupings as the UI's
  group boundaries directly — they're already organized by operational
  theme (clarifier, keyer, scan/VOX, attenuator/noise, band/step, meters,
  misc) and were designed with independent-reviewability in mind, which
  tends to correlate with UI-discoverability grouping too.
  `CommandKind`'s descriptor-table pattern (kept unchanged from Wave 2,
  since Wave 2 already found it command-count-independent) still drives
  each group's contents.
- `EX`'s 153 items need **more** structure than a single flat group menu —
  recommend treating `EX` as its own top-level area with two
  complementary access paths: (a) a themed sub-grouping mirroring §10.6's
  menu-number-range shape, for discoverability/browsing, and (b) a direct
  "enter menu number, then value" power-user shortcut (a `TextInput`
  prompting for a 3-digit `P1` first, then the item-specific value) as an
  escape hatch, since no grouping scheme will give all 153 items a
  dedicated, memorable key. Recommend building (b) regardless of whether
  (a) ships in the same pass — it's the higher-leverage half for an EEPROM-
  editor-style menu this size.
- This is a full, independently-scoped future wave — dispatch once command
  coverage (§10.5/§10.6) has substantially landed, not before (building
  the grouped UI against a still-growing command table risks the same
  rework Wave 2 §"Why no ui/emulator implementation work this wave"
  reasoning already called out once).
- `emulator`'s `tui.rs` (Wave 2 §7.3) will need the same proportional
  growth (more annunciator fields, wider frequency/mode rendering as new
  `Ft991aState` fields land) — flagged as a parallel, smaller follow-up
  each time a `yaesu` batch lands new state fields, not a separate big
  design effort of its own.

### 10.8 Dispatch queue — Wave 3

Ordering rationale: (a) is cross-repo and blocks only §10.4, not §10.5/6,
so it can run **in parallel** with the start of CAT-batch work, not
strictly serially before it. Within `yaesu`'s own CAT-batch work, tasks
share one file (`ft991a_radio.rs`) and should stay **sequential, one
`yaesu` dispatch at a time, reviewed before the next** (per `CLAUDE.md`'s
"Architect Review Workflow" and to avoid merge conflicts on a single file
— unlike Wave 2's `ui`/`emulator` parallel dispatch, which had genuinely
non-overlapping file scopes).

1. **[cross-repo, `radio-cat-rs`, dispatch first]** `ModemControlLines`
   trait in `cat-transport-core` + concrete `SerialPort` impl and
   `SerialCatSession` delegation in `cat-transport-serial`, per §10.3.
   **Route this to `radio-cat-rs`'s own architect/agents** — this repo's
   `yaesu`/`app` agents do not touch that repository. Independent of every
   other item below; nothing here blocks it.
2. **[`yaesu`, this repo]** CAT batch 9 (Meters/status, incl. `IF`'s
   careful re-verification) — dispatched early because batches 2 and 10
   depend on its shared composite-payload finding (§10.5).
3. **[`yaesu`]** `EX` menu plumbing + the 9 PTT/keying-relevant items
   (§10.6's priority carve-out) — dispatched early because it's the
   software-completeness half of this wave's marquee new feature.
4. **[`yaesu`]** CAT batch 2 (Memory channel records) — now unblocked by
   task 2's shared field parser.
5. **[`yaesu`]** CAT batches 1, 3, 4, 5, 6, 7, 8, 10 — remaining order
   flexible (no cross-batch dependencies found among these), suggest
   numeric order for simplicity unless the user wants to reprioritize.
6. **[`yaesu`, this repo, depends on task 1 landing]** `ft991a`-side RTS/DTR
   consumption (§10.4): `SharedSession<S: ModemControlLines>` delegation +
   `Ft991a<S>`'s new `assert_rts`/`assert_dtr`/`read_cts`/`read_dsr`/
   `read_dcd` impl block. Blocked on task 1 (needs the trait to exist to
   compile against); not blocked on task 3 (menu 060 can be set by hand on
   the radio without CAT support). Scope: `radio/` only, mirrors Wave 1's
   "yaesu task doesn't touch ui" precedent.
7. **[`yaesu`, remaining `EX` sub-batches]** the other ~144 `EX` items,
   sub-batched by menu-number range per §10.6 — lower priority than tasks
   2-6, interleavable with task 5 at the user's discretion.
8. **[future wave, not dispatched yet]** `ui`/`app` follow-up: a keybinding
   that calls `Ft991a::assert_rts` for CW send (small, depends on task 6
   landing) — and, separately, the full grouped-menu `ui` redesign (§10.7,
   depends on tasks 2-7 having substantially landed).

Nothing in this wave touches `emulator`'s existing files beyond the
proportional `tui.rs` follow-ups noted in §10.7 — not scoped as a
standalone task list here; fold into each `yaesu` batch's review as "does
`emulator/src/tui.rs` need a new annunciator for this batch's new state
fields," same as Wave 2 intended.

---

## 11. Wave 4 — `ui`/`emulator` full redesign against landed command coverage (2026-07-19)

### 11.0 Scope of this session

§10.7 flagged (not designed) that `ui`'s Wave-2 flat 9-key design would not
survive full command coverage landing. It has now landed: `radio` implements
all 91 top-level commands and 151/153 `EX` items (2 permanently
unresolvable — item 087 "RADIO ID"'s undocumented P2 format, and any other
manual gap found along the way). `ui` itself is unchanged since Wave 2 —
confirmed by reading the actual committed files this session, not assumed:
`ui/src/control.rs` is still exactly the flat 9-entry `command_table()`,
`ui/src/lib.rs`'s `Ft991aDisplay` still has Wave 2's field set, `ui/src/
terminal.rs`'s `run<R: Radio + 'static>` is unchanged, and `emulator/src/
tui.rs` is still the Wave-2-scoped ~600-line file with only `power_on`/
`cat_tx` annunciators. This session designs the redesign in full (no code
written, per the architect's standing prohibition) and produces a sized
dispatch queue.

### 11.1 Sources read this session

- `radio/src/ft991a_radio.rs`: the full `Ft991aCommandId` enum (91 variants,
  confirmed the 10 batch groupings from §10.5 are literally present as enum
  doc-comments, unchanged in shape from the Wave 3 design); `ExMenuValueKind`
  (`Enumerated(&[&str])` / `Range{min,max,step,signed}`), `ExMenuItem`
  (`p1`, `name`, `digits`, `kind`), `EX_MENU_TABLE`; `Ft991aState` (grepped
  representative sections — confirmed it has grown from Wave 2's 9 fields to
  100+ across VFO/mode/TX, the `IF`-composite fields, all 8 meter-select
  fields, and per-`EX`-item fields for every landed sub-batch).
- `radio/src/radio_trait.rs` (full method list via grep, ~90 trait methods,
  every one with a default `NotImplemented` body — confirmed this is already
  the established idiom in this trait, not something Wave 4 introduces).
- `radio/src/ft991a.rs` (full method list via grep): confirmed the `Radio`
  trait impl's method set, and separately confirmed a real, load-bearing gap
  — **~20 client-side methods exist only as `Ft991a<S>` inherent methods,
  never added to `Radio`**: `read_keyer_memory`/`write_keyer_memory`/
  `play_keyer_memory`, `qmb_store`/`qmb_recall`, `quick_split`,
  `toggle_vfo_memory_mode`, `encoder_down`/`encoder_up`/`ent_key`/`mic_up`/
  `mic_down`, `get_antenna_tuner_state`/`set_antenna_tuner_state`,
  `get_dimmer`/`set_dimmer`, `read_date`/`write_date`/`read_time`/
  `write_time`/`read_time_zone_offset`/`write_time_zone_offset`,
  `get_opposite_band_information`, `get_txw_on`/`set_txw_on`,
  `start_dvs_recording`/`stop_dvs_recording`/`get_dvs_recording_channel`/
  `start_dvs_playback`/`stop_dvs_playback`/`get_dvs_playback_channel`,
  `get_contour_on`/`set_contour_on`/`get_contour_frequency_hz`/
  `set_contour_frequency_hz`, `get_apf_on`/`set_apf_on`/
  `get_apf_frequency_hz`/`set_apf_frequency_hz`, `get_manual_notch_on`/
  `set_manual_notch_on`/`get_manual_notch_frequency_hz`/
  `set_manual_notch_frequency_hz`, `get_parametric_mic_eq_on`/
  `set_parametric_mic_eq_on`, `get_information` (the `IF` composite read).
  This is not an oversight — it directly follows `CLAUDE.md`'s "Radio trait
  scope" rule ("FT-991A-specific features... live in the radio crate as
  inherent methods on `Ft991a`, NOT in the `Radio` trait"), and cross-checked
  against `ts570d`'s own sibling rule (identical wording in `ts570d/
  CLAUDE.md`) — but `ts570d/ui/src/terminal.rs` shows `ts570d`'s own
  practice actually put keyer speed, antenna-tuner-thru, and voice-recall
  *onto* its `Radio` trait despite the same stated policy, i.e. the "portable
  concept" bar in practice is drawn more generously than the policy text
  alone suggests. `ft991a`'s `radio_trait.rs` already followed that
  generous-but-not-total reading (keyer speed/pitch/enabled, break-in,
  attenuator, preamp, noise blanker/reduction, AGC, notch, narrow, filter
  width, mic gain, speech processor, monitor, band, fine step, mic up/down,
  auto info, frequency lock, repeater shift, tx vfo, mox are all already on
  the trait) — what's left inherent-only above genuinely is the smaller,
  odder, more FT-991A-idiosyncratic remainder (raw encoder nudges, DVS,
  antenna-tuner numeric state, dimmer, date/time, contour/APF/manual-notch
  frequency pairs, the `IF` composite struct). No `get`/`set` pair exists
  anywhere in `radio` today for reading or writing an arbitrary `EX` menu
  item by number — confirmed by grep; this is a **new** capability Wave 4's
  `EX` escape hatch needs, not a relocation of an existing one.
- `radio/src/ft991a.rs`'s already-landed `ModemControlLines` consumption
  block (§10.3/10.4, confirmed unchanged and already merged):
  `assert_rts`/`assert_dtr`/`read_cts`/`read_dsr`/`read_dcd` are sync `&self`
  inherent methods on `impl<S: CatSession<Error=TransportError> +
  ModemControlLines> Ft991a<S>` — a **separate, additive** impl block from
  the `Radio` trait impl, exactly as designed in Wave 3. Not part of `Radio`
  today, and — confirmed by tracing Rust's coherence rules through this
  design (see §11.3) — cannot become part of `Radio` without breaking any
  non-`ModemControlLines` test double, so this constraint is permanent, not
  an oversight to "just fix."
- `ui/src/{control.rs,layout.rs,terminal.rs,lib.rs}` (full): confirmed
  unchanged since Wave 2 — flat `ControlState::{Normal,TextInput,ListSelect,
  Feedback}`, 9-entry `command_table()`, `Ft991aDisplay` with Wave 2's field
  set, `run<R: Radio + 'static>(radio: R)` unchanged signature, `main.rs`
  moves `radio` into `ui::run(radio)` by value with no reference retained
  afterward (confirms there is no interleaving point for `main.rs` to inject
  its own keybinding once `ui::run` owns the event loop — directly relevant
  to §11.3).
- `src/main.rs` (full): confirmed complete and coherent (the concurrent
  Windows-entry-point session's edits, if any are still in flight, did not
  leave this file in a visibly partial state as read this session) —
  `run_app()` constructs `Ft991a::new(SerialCatSession::new(port))` and
  calls `ui::run(radio).await` once, with no other radio-touching code
  before or after. Confirms `SerialPort` (hence `Ft991a<SerialCatSession<
  SerialPort>>`) is the only concrete wiring that exists in this repo today,
  and (per §10.3) it always implements `ModemControlLines` unconditionally
  — no CLI flag or config disables it.
- `ts570d/ui/src/control.rs` (structural read, first 240 lines + grep for
  the rest): confirmed the real three-level shape — `CommandGroup` (8
  variants), `ControlState::{Menu, GroupMenu{group,cursor}, TextInput,
  ListSelect, Feedback, Diagnostic}`, `CommandKind::{Text,List,Immediate}`,
  `GroupCommand{label,kind}`, one `{theme}_commands() -> Vec<GroupCommand>`
  function per group, `group_commands(group)` dispatcher, `group_command_
  labels(group)`, `select_group_command(group, idx)`. This is the "in
  spirit" structure §10.7 pointed at — ported below with `ft991a`-specific
  group contents, not `ts570d`'s.
- `emulator/src/tui.rs` (grep for structure): confirmed still the Wave-2
  three-column layout (`draw_meter_col`/`draw_rx_smeter`/`draw_tx_meter`,
  `draw_lcd_main`/`draw_ann_line`/`draw_vfo_b_line`/`draw_freq_block`/
  `draw_mode_row`, `draw_command_panel`) rendering only `RadioState`'s Wave-2
  field set (`power_on`, `cat_tx`, `vfo_a_hz`, `vfo_b_hz`, `mode`, `smeter`)
  — confirmed **not yet grown** to match `Ft991aState`'s 100+ fields, exactly
  the gap §10.7 flagged.

### 11.2 The grouped-menu structure

Confirmed concretely against the real `Radio`/`Ft991aExtras` split (§11.3),
not the abstract batch list. **12 groups**, reusing the CAT batch
groupings' theme boundaries (§10.5) as UI group boundaries, per §10.7's
recommendation, sized 4-12 items each (matching `ts570d`'s own 8-groups-of-
5-12 precedent):

| # | Group | Source | Item count (approx) | Backing |
|---|-------|--------|----|---------|
| 1 | Frequency & Levels | first-slice 9 (unchanged since Wave 1/2) | 9 | 100% `Radio` |
| 2 | VFO / Memory Quick-Ops | batch 1 (`AB BA AM VM MA CH QI QR QS SV`) | 10 | mixed: `AB/BA/SV`(swap)/`CH` on `Radio`; `QI/QR`(QMB)/`QS`(quick split)/`VM`(toggle_vfo_memory_mode) on `Ft991aExtras` only |
| 3 | Memory Channels | batch 2 (`MC MR MW MT`) | 4 | 100% `Radio` (`get/set_memory_channel`, `read/write_memory_channel`, `read/write_memory_channel_tag`) |
| 4 | Clarifier / Tone / IF-Shift | batch 3 (`RT RC RD RU XT CN CT IS`) | 8 | 100% `Radio` |
| 5 | Keyer / CW / Break-In | batch 4 (`KM KP KR KS KY CS ZI BI SD`) + RTS keying | 9 + 1 | mixed: break-in/keyer-speed/pitch/enabled/zero-in/cw-spot on `Radio`; `KM`/`KY` (keyer memory store/play) on `Ft991aExtras`; new RTS CW-key toggle on `CwKeying` |
| 6 | Scan / VOX / Busy | batch 5 (`SC VX VD VG BY`) | 5 | 100% `Radio` |
| 7 | Attenuator / Noise / AGC / Notch / Filter | batch 6 (`RA PA NB NL NR RL GT CO BP BC NA SH`) | 12 | mixed: attenuator/preamp/NB/NR/AGC/auto-notch/narrow/filter-width on `Radio`; contour (`CO`)/manual-notch (`BC`/`BP`? — confirm exact code-to-field mapping in the task)/APF on `Ft991aExtras` |
| 8 | Speech / Mic / Monitor | batch 7 (`PL PR MG ML`) | 4 | mostly `Radio` (mic gain, speech-proc level/on, monitor on/level); parametric mic EQ on `Ft991aExtras` |
| 9 | Band / Step / Encoder | batch 8 (`BS BU BD FS ED EU EK DN UP`) | 9 | mixed: band select/up/down, fine step, mic up/down on `Radio`; raw encoder nudges (`ED`/`EU`)/ENT key (`EK`) on `Ft991aExtras` |
| 10 | Meters / Status | batch 9 (`IF RM RI RS MS UL`) | 6 | mixed: `select_meter`/`get_selected_meter`/`get_meter` on `Radio`; `IF` composite (`get_information`), radio-indicator (`RI`), PLL-unlock (`RS`→`get_pll_unlocked`), menu-mode-active (`UL`) on `Ft991aExtras` — this group is read-heavy (mostly display, few keybindings) |
| 11 | System / Tuner / DVS | batch 10 (`AC AI DA DT LK OI OS FT TS MX LM PB`) | 12 | mixed: auto-info/frequency-lock/repeater-shift/tx-vfo/mox on `Radio`; antenna tuner state, dimmer, date/time/tz, opposite-band info, TXW, DVS record/playback on `Ft991aExtras` |
| 12 | `EX` Menu | 151/153 landed items | 151 | its own two-path design, §11.4 — needs new `Ft991aExtras` methods, doesn't exist as `Radio`/`Ft991aExtras` methods at all today |

**Decision on straddling groups**: keep the operational-theme boundary (not
a `Radio`-vs-`Ft991aExtras` boundary) — splitting, e.g., "Keyer" into two
separate groups because `KS`/`KP` are on `Radio` but `KM`/`KY` are on
`Ft991aExtras` would fragment an operator's mental model for no UI benefit.
This is safe to do because §11.3 resolves `ui::run`'s bound to include both
traits unconditionally — every group's `CommandKind` descriptor can call
either trait's methods interchangeably without the UI code itself needing
to know or care which trait backs a given item.

**Group 1 note**: the existing flat 9-command screen becomes literally
`CommandGroup::FrequencyLevels`'s contents, unchanged in validation/keys —
this group is the "no worse than today" baseline the redesign must preserve,
not something to redesign for its own sake.

### 11.3 RTS/DTR keybinding placement — resolved concretely

**Resolution: it belongs in `ui`, reachable via `ui::run`'s own generic
bound, not via a separate `main.rs`-side raw keybinding outside the
`Radio`-trait-based UI.**

Worked through concretely, not deferred:

1. **`main.rs` cannot host it as a raw keybinding outside `ui::run`.**
   `main.rs` today calls `ui::run(radio).await` exactly once, moving `radio`
   into it by value; `ui::run`'s single sequential event loop (§6.6, still
   accurate) owns the terminal and all key events for the process's entire
   interactive lifetime. There is no point where control returns to
   `main.rs` mid-session for it to intercept a keypress — `main.rs` would
   have to *not* delegate to `ui::run` at all and reimplement its own event
   loop, which defeats the entire point of the `ui` crate. Ruled out.
2. **`ui::run<R: Radio + 'static>` cannot call `Ft991a::assert_rts` etc.
   generically** — those are inherent methods on `Ft991a<S>`, not on
   `Radio`, and (confirmed by tracing Rust's coherence rules through this
   specific case) **cannot** be added to `Radio` as trailing default-body
   methods the way the rest of the trait is idiomed: `Ft991a<S>`'s
   `ModemControlLines`-backed impl needs the extra `S: ModemControlLines`
   bound, and Rust forbids two `impl Radio for Ft991a<S>` blocks with
   overlapping `S` (a blanket "all `S: CatSession`" default-`NotImplemented`
   impl plus a specialized "`S: CatSession + ModemControlLines`" override
   impl is exactly the overlapping-instance case `rustc` rejects as E0119
   without the unstable `min_specialization` feature, which this project
   does not use). This is why Wave 3 correctly made these inherent methods
   in the first place (§10.4) — Wave 4 doesn't reopen that call, it works
   out how `ui` reaches them anyway.
3. **The actual fix: two new traits in the `radio` crate, plus widening
   `ui::run`'s bound.** Define, in `radio` (new file or appended to
   `radio_trait.rs`):
   - `Ft991aExtras` (async, default bodies returning
     `RadioError::NotImplemented`, exactly like every existing `Radio`
     method) — wraps the ~20 inherent-only async methods found in §11.1,
     **plus the new** `get_ex_menu_item(p1: u16) -> RadioResult<i32>` /
     `set_ex_menu_item(p1: u16, value: i32) -> RadioResult<()>` pair
     `EX`'s escape hatch needs (§11.4). Implemented **unconditionally** for
     `impl<S: CatSession<Error = TransportError>> Ft991aExtras for
     Ft991a<S>` — the exact same bound `Radio`'s own impl already uses, so
     this is a single, non-overlapping impl with zero coherence risk, and
     every `Ft991a<S>` that satisfies `Radio` automatically satisfies this
     too.
   - `CwKeying` (sync `&self`, matching `ModemControlLines`'s own
     sync-fn precedent, default bodies returning `NotImplemented`) — wraps
     the already-landed `assert_rts`/`assert_dtr`/`read_cts`/`read_dsr`/
     `read_dcd` inherent methods. Implemented for `impl<S: CatSession<Error
     = TransportError> + ModemControlLines> CwKeying for Ft991a<S>` — the
     *same* bound the existing §10.4 impl block already uses (this is a
     thin wrapper around what's already there, not new radio-side logic).
     This is the **one** trait that stays genuinely conditional on the
     transport, by design — a future non-serial transport without modem
     control lines simply won't implement it, a compile-time signal rather
     than a silent runtime `NotImplemented`.
   - `ui::run`'s signature widens to `pub async fn run<R: Radio +
     Ft991aExtras + CwKeying + 'static>(radio: R) -> UiResult<()>`. This
     does **not** change `main.rs`'s call site (`ui::run(radio).await`
     stays textually identical) — it changes only the compile-time
     constraint, and `main.rs`'s only concrete wiring
     (`Ft991a<SerialCatSession<SerialPort>>`) already satisfies all three
     bounds today with no code change, because `SerialPort` implements
     `ModemControlLines` unconditionally (§10.3 — no config flag disables
     it). This is a real, disclosed widening of `ui::run`'s generic
     contract (worth calling out to the `ui`/`yaesu` agents explicitly,
     not silently treated as "no behavior change"), but it costs nothing
     against today's actual wiring.
   - `ui`'s own in-crate `MockRadio` test doubles (in `ui/src/lib.rs` and
     `ui/src/terminal.rs`, per `CLAUDE.md` rule 6) need a one-line
     `impl Ft991aExtras for MockRadio {}` / `impl CwKeying for MockRadio {}`
     added (inheriting the trait's own `NotImplemented` defaults) to keep
     compiling against the new bound — small, mechanical, flagged for the
     `ui` skeleton task (§11.5, Task U1).
4. **Why this doesn't violate `CLAUDE.md` rule 4** ("`ui` may depend on
   `radio`... but NEVER on `cat-transport-serial` or any transport crate").
   `Ft991aExtras`/`CwKeying` are defined in `radio`, not in a transport
   crate — `ui` gains a dependency on two more `radio`-crate-defined traits,
   not on `cat_transport_core::ModemControlLines` or any transport type
   directly. `ui` never needs to name `ModemControlLines` itself.
5. **Practical consequence for scope**: since `ui::run`'s bound now requires
   `Ft991aExtras` unconditionally, `ui` is no longer a "any `Radio`
   implementation" UI in the abstract — it is (and, per this repo's actual
   history, always effectively has been, since no second radio type has
   ever been wired to it) an FT-991A-shaped UI. This is recorded as an
   explicit, disclosed scope narrowing, not a silent one — flagged for the
   `ui` skeleton task to note in its own module docs.
6. **Keybinding placement and semantics**: `K` in the Keyer/CW/Break-In
   group (group 5, §11.2), `CommandKind::Immediate`, toggle semantics
   mirroring the existing `T`/`ToggleTx` idiom exactly (not hold-to-key —
   crossterm's default keyboard-event mode does not reliably deliver
   key-release events across terminals without opting into the Kitty
   keyboard protocol, which is out of scope for this wave; toggle is the
   pragmatic, already-precedented choice). `Ft991aDisplay` gains a new
   `rts_asserted: bool` field, tracked **locally** (not polled — there is no
   "read back what I asserted" ioctl; `ModemControlLines::read_cts` reads
   the *status* line CTS, not the *control* line RTS's own asserted state),
   toggled optimistically on keypress and rolled back if `assert_rts`
   returns an `Err`, mirroring how `ExecuteAction` results already flow
   into `ControlState::Feedback` today.

### 11.4 `EX` menu — two access paths

**Blocking prerequisite, not previously flagged**: confirmed by grep that
`radio` has **no** client-side way to read or write an arbitrary `EX` item
today — no `get_ex_menu_item`/`set_ex_menu_item` exists on `Ft991a<S>` or
`Radio` (the only existing `EX`-adjacent method, `get_menu_mode_active`,
reads `RS`'s menu-mode-active flag, a different thing entirely). Both UI
paths below are blocked on a `yaesu` task adding these two methods to the
new `Ft991aExtras` trait (§11.3) first — sized small (wraps the existing
`Ft991aCommandId::Ex` wire form + `EX_MENU_TABLE` lookup/parse/format logic
that `ft991a_radio.rs`'s server-side `handle_command` already has, from the
client side).

**Second gap worth flagging for that same task**: `ExMenuValueKind::
Enumerated` stores only raw wire strings (e.g. `&["0","1","2","3"]` for
item 060 "PC KEYING"), with no attached human-readable label
(`"OFF"`/`"DAKY"`/`"RTS"`/`"DTR"`). A `ListSelect`-driven UI for `EX`
enumerated items would otherwise have to show raw digit strings as its
options, which is materially worse UX across 90+ enumerated items than the
named legends the manual actually gives every one of them. Recommend the
same `yaesu` task extend `ExMenuValueKind::Enumerated` to `&'static [(&
'static str /* wire */, &'static str /* label */)]` (or add a parallel
labels table) while it's already touching this type for `get`/
`set_ex_menu_item` — not required for (b) to function at all (could fall
back to raw wire digits), but flagged as a real UX gap, not silently
absorbed into "the UI will figure it out."

**(a) Themed browsing sub-groups**: built by bucketing `EX_MENU_TABLE`'s
actual `p1` values at **runtime from the real table**, not a hand-maintained
parallel list in `ui` (avoids drift as more sub-batches land) — five
sub-groups by `p1` range, per §10.6's own page-layout observation:
001-046 (general/AGC/CW), 047-079 (TX audio chain, incl. the PTT/port-select
family), 080-091 (mixed — verify exact boundary against the manual when
building this), 092-110 (RTTY/SSB TX chain), 111-136 (meter/scope), 137-153
(band-limit/VOX). Each sub-group is a `GroupMenu`-style scrollable list
(`ts570d`'s `GroupMenu{group,cursor}` shape already scrolls — reuse
directly) showing `"{p1:03} {name}"`, `Enter` opens a `TextInput`/
`ListSelect` sized by that item's own `ExMenuValueKind` (same value-entry
UI as path (b) below — the two paths converge on the same value-entry state
once an item is selected, they only differ in *how the item is found*).

**(b) Number-entry escape hatch — concrete state machine**:

```
Normal
  '['X']' (top-level key, always visible, independent of whether (a)'s
           groups exist yet) or from inside the EX group's own screen
    -> ControlState::ExNumberEntry { buffer: String, error: Option<String> }

ExNumberEntry (reuses the TextInput rendering shell, new InputAction variant)
  typing digits (max 3) -> buffer grows
  Enter ->
    parse buffer as u16 P1
    look up EX_MENU_TABLE.iter().find(|i| i.p1 == p1)
    None                     -> error = Some("No such menu item"), stay in ExNumberEntry
    Some(item) -> ControlState::ExValueEntry { item, .. } (see below)
  Esc -> ControlState::Normal (or back to the EX group screen, if entered from there)

ExValueEntry { item: &'static ExMenuItem, .. }
  forks on item.kind:
    ExMenuValueKind::Enumerated(values) ->
      reuse ControlState::ListSelect verbatim, options built from `values`
      (+ labels once the yaesu task above lands), action = new
      SelectAction::SetExMenuItem(item.p1)
    ExMenuValueKind::Range{min,max,step,signed} ->
      reuse ControlState::TextInput verbatim, prompt shows "{name}
      ({min}..={max}, step {step})", action = new
      InputAction::SetExMenuItem(item.p1), validated the same way
      `validate_text_input` already validates ranges today (generalized
      from hardcoded per-field ranges to a range read off `item.kind`)
  Enter/confirm -> KeyResult::Execute(ExecuteAction::SetExMenuItem(p1, value))
    -> terminal.rs's execute_action calls radio.set_ex_menu_item(p1, value)
       (new Ft991aExtras method, §11.4 top)
  Esc -> back to ExNumberEntry (path (b)) or the EX group screen (path (a))
```

A **read-first-then-edit** UX (calling `get_ex_menu_item(p1)` immediately
after a valid P1 is entered, to pre-fill/pre-select the value entry with the
item's *current* value — same "pre-select current value" courtesy the
existing Mode `ListSelect` already gives via `initial_list_cursor`) is
recommended but not load-bearing; flag it as a should-have for the
implementing task, not a hard requirement.

**Dependency answer to the brief's explicit question**: the escape hatch
(b) does **not** have a hard technical dependency on the grouped-menu
skeleton (U1, §11.5) — it needs only its own new `ControlState` variants and
a place to bind its top-level key, which could in principle be bolted onto
the *current* flat `Normal` screen. It **does** have a hard dependency on
the `get`/`set_ex_menu_item` `yaesu` prerequisite task above. **Sequencing
recommendation** (not a hard requirement): build it after U1 lands anyway,
since bolting a 10th key onto a flat screen about to be replaced wastes
review effort on code with a one-task shelf life. Path (a) does depend on
(b)'s value-entry state machine existing first (they share `ExValueEntry`),
so (a) strictly follows (b).

### 11.5 `emulator/tui.rs` proportional growth

**Shape recommendation: stay a flat, wider annunciator list — do not build
a grouped/paginated emulator display.** Reasoning, checked against the
emulator's actual job (a debugging/testing aid showing *everything at once*
for whoever is driving the real hardware protocol against it), not assumed:

- `ui`'s grouped-menu redesign (§11.2) solves a *keybinding-discoverability*
  problem (90+ things can't each get a memorable single key) — the
  emulator has **no keybindings** to discover; `tui.rs` is read-only
  display, so the discoverability pressure that forced `ui`'s redesign
  doesn't apply to it at all.
- `ts570d/emulator/tui.rs`'s own precedent (already read in Wave 2, §7)
  handles a comparably large annunciator set (RIT/XIT/split/antenna/AGC/
  noise/etc.) as a flat, denser annunciator line/column, not a paginated or
  grouped view — the working precedent for "how much annunciator density is
  too much for one screen" already exists and hasn't needed paging.
- Concrete growth, by existing `tui.rs` region (§7.3's Wave 2 file map,
  extended):
  - `draw_ann_line`: grows from 2 annunciators (`power_on`, `cat_tx`) to
    ~15-20 — clarifier (RX/TX on, offset), keyer enabled, scan state, VOX
    on, attenuator on, preamp mode, noise blanker/reduction on, AGC mode,
    auto-notch on, narrow on, frequency lock, mox, break-in on, PLL
    unlocked/menu-mode (from the `IF`/`RS`/`UL` fields) — same visual
    idiom (`on_style`/dim-when-off) already established, just more of them,
    wrapped across 2-3 lines instead of 1.
  - `draw_freq_block`/`draw_mode_row`: unchanged in shape, but add the
    clarifier offset readout next to VFO A/B (a natural extension of the
    existing frequency block, not a new region).
  - `draw_meter_col`/`draw_rx_smeter`/`draw_tx_meter`: extend to the full
    6-way `MS`-selected meter (COMP/ALC/PO/SWR/ID/VDD, `Ft991aState`'s
    `*_meter` fields) instead of just S-meter — the column already exists
    for exactly this purpose per Wave 2's own design note ("TX-side meter
    column has no FT-991A equivalent yet" — that gap is now closed by
    batch 9 landing `RM`/`MS`).
  - `draw_command_panel`'s `lookup_description`: retarget to the now-full
    `FT991A_COMMAND_TABLE` (91 entries incl. `EX`) — mechanical, no shape
    change.
  - **New, not in Wave 2's map**: an `EX`-state summary is explicitly **out
    of scope** for `tui.rs` — displaying live values for up to 151 settings
    items on a debugging screen whose job is "what is the radio doing right
    now" (operating state), not "what are all its menu settings," would
    dilute the screen's actual purpose. Recommend a **separate, optional**
    `--dump-ex` flag or keypress that prints the full `EX` state to the log
    panel/stderr on demand instead of a permanent screen region — flagged
    as a nice-to-have for the emulator task to scope in or explicitly defer,
    not a requirement.
- One task (not split by batch) — the whole growth is mechanical field-by-
  field extension of already-established rendering idioms, not new design
  work per field; splitting it would create more review overhead than the
  work warrants, unlike `ui`'s group-content tasks (§11.6) where each group
  genuinely needs its own keybinding/validation design pass.

### 11.6 Dispatch queue — Wave 4

Ordering rationale: the `radio`-crate prerequisite (P1) blocks everything
else that needs `Ft991aExtras`/`CwKeying`/`get_ex_menu_item` to exist, so it
must land and be reviewed first. The `ui` skeleton (U1) then blocks every
group-content task, since they all add content into the state machine and
render functions U1 establishes. Group-content tasks share `ui/src/
{control.rs,layout.rs}` (two files, not disjoint per group) — per
`CLAUDE.md`'s Architect Review Workflow and Wave 3's own precedent for
`yaesu`'s single-file CAT batches, these run **sequentially, one at a time,
reviewed before the next** (unlike Wave 2's `ui`-vs-`emulator` parallel
dispatch, which had genuinely non-overlapping file scopes). The `emulator`
task is independent of all of these (different crate, no shared files) and
can run any time, including in parallel with the whole `ui` sequence.

1. **[`yaesu`, `radio/` only]** Add `Ft991aExtras` trait (~20 existing
   inherent methods re-exposed as trait methods with `NotImplemented`
   defaults, unconditional impl for `Ft991a<S: CatSession>`) + `CwKeying`
   trait (thin wrapper over the already-landed `ModemControlLines`
   consumption, impl bounded on `+ ModemControlLines`) + new
   `get_ex_menu_item`/`set_ex_menu_item` methods on `Ft991aExtras` + the
   `ExMenuValueKind::Enumerated` label-table extension (§11.4). Blocks
   everything below. Sized comparably to one of Wave 3's CAT batches.
2. **[`ui`]** Grouped-menu skeleton: `CommandGroup` (12 variants, §11.2),
   `ControlState::{Menu, GroupMenu, TextInput, ListSelect, Feedback}` (no
   `Diagnostic` — still not warranted, unchanged from Wave 2's §6.1 call,
   revisit only if a future wave adds a self-test harness), port the
   existing 9-command flat screen into `CommandGroup::FrequencyLevels`
   unchanged, widen `run`'s bound to `Radio + Ft991aExtras + CwKeying`, fix
   up `MockRadio` test doubles (§11.3 point 3's last bullet). Depends on
   task 1. This is the largest single task in the queue — recommend the
   `ui` agent flag if it wants to split skeleton-plumbing from
   group-1-porting, architect's call once scoped.
3. **[`ui`]** Populate groups 3 (Memory Channels), 4 (Clarifier/Tone/
   IF-Shift), 6 (Scan/VOX/Busy) — the three groups that are 100% `Radio`-
   trait-backed, no `Ft991aExtras` involvement, good "prove the skeleton
   works" follow-up. Depends on task 2.
4. **[`ui`]** Populate group 5 (Keyer/CW/Break-In) **including** the RTS
   CW-keying keybinding (§11.3 point 6) — the wave's other marquee feature,
   front-loaded like Wave 3 front-loaded the `EX` PTT sub-batch. Depends on
   tasks 1-3.
5. **[`ui`]** Populate group 2 (VFO/Memory Quick-Ops) and group 9
   (Band/Step/Encoder) — both mixed `Radio`/`Ft991aExtras`, moderate
   complexity. Depends on task 4 (file-sequencing, not a real design
   dependency).
6. **[`ui`]** Populate group 7 (Attenuator/Noise/AGC/Notch/Filter) and
   group 8 (Speech/Mic/Monitor). Depends on task 5.
7. **[`ui`]** Populate group 10 (Meters/Status, read-heavy/display-focused)
   and group 11 (System/Tuner/DVS, the most `Ft991aExtras`-heavy group —
   date/time read/write needs its own small multi-field `TextInput` design,
   flagged for this task specifically). Depends on task 6.
8. **[`ui`]** `EX` menu number-entry escape hatch, path (b) (§11.4).
   Depends on task 1 (client methods) and task 2 (skeleton, for the
   top-level key placement — recommended sequencing, not a hard
   dependency per §11.4's explicit answer). Can run any time after tasks
   1-2 land; placed here for review-queue simplicity, not a strict gate on
   tasks 3-7.
9. **[`ui`]** `EX` menu themed browsing, path (a) (§11.4) — depends on task
   8 (shares `ExValueEntry`).
10. **[`emulator`]** `tui.rs` proportional growth (§11.5) — independent of
    tasks 2-9; only depends on task 1 not at all (emulator reads
    `Ft991aState`, already fully populated by the landed Wave 3 `yaesu`
    batches, not by this wave's new traits). Can be dispatched in parallel
    with the entire `ui` sequence above.

Not dispatched this session, per the architect's standing prohibition on
writing code or dispatching subagents directly — this section records the
design and queue for the coordinating session to dispatch from.
