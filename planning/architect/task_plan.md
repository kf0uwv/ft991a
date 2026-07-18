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

## Next steps (Wave 2+, not dispatched yet)

1. Review Wave 1 output (both tasks) with the user before proceeding.
2. `ui` agent: ratatui TUI against the reviewed `radio::Radio` trait
   surface from Wave 1; `app` agent updates `src/main.rs`/`ui` placeholder
   accordingly.
3. `emulator` agent: PTY-hosted `CatFramework<Ft991aRadio>` process, mirroring
   `ts570d/emulator`, added as a workspace member + `app`'s dev-dependency
   at that point.
4. `yaesu` agent, follow-on waves: `IF` (careful page-10 re-verification),
   `RM`/`RI`, attenuator/preamp/noise/clarifier/scan/VOX, memory channels,
   the 153-entry `EX` menu table, and the remaining commands listed in §2's
   "explicitly out of scope" list — each its own reviewed task, not bundled.
