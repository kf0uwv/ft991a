//! FT-991A emulator state machine behind the generic CAT framework.
//!
//! `FT991A_COMMAND_TABLE` is derived from
//! `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf`, re-verified page-by-page
//! against the manual's own per-command Set/Read/Answer tables (not just the
//! p.3 master O/X table) — see `planning/yaesu/task_plan.md` for the full
//! citation list and one confirmed correction to the architect's original
//! transcription (`AG`/`RG`/`SQ` are plain zero-width-query commands, not
//! "selector reads" like `MD`/`SM`).
//!
//! # Wire formats used (11 first-slice commands)
//!
//! | Command | Code | Query          | Set                    | Notes |
//! |---------|------|----------------|-------------------------|-------|
//! | VFO A   | FA   | `FA;`          | `FA<9 digits>;`         | p.9, range 30,000-470,000,000 Hz |
//! | VFO B   | FB   | `FB;`          | `FB<9 digits>;`         | p.9, same shape as FA |
//! | Mode    | MD   | `MD0;` (selector read) | `MD0<hex digit>;` | p.11, modes 1(LSB)-E(C4FM) |
//! | TX      | TX   | `TX;` → 0/1/2  | `TX<0/1>;`              | p.17, 3-valued answer |
//! | S-meter | SM   | `SM0;` (selector read) | none (read-only) | p.17 |
//! | Power   | PS   | `PS;`          | `PS<0/1>;`              | p.14, wake-sequence quirk (see `Ft991a::set_power_on`'s doc comment) |
//! | AF gain | AG   | `AG;`          | `AG0<3 digits>;`        | p.4, 000-255 |
//! | RF gain | RG   | `RG;`          | `RG0<3 digits>;`        | p.15, 000-255 |
//! | Squelch | SQ   | `SQ;`          | `SQ0<3 digits>;`        | p.17, 000-100 |
//! | TX power| PC   | `PC;`          | `PC<3 digits>;`         | p.14, 005-100 watts |
//! | ID      | ID   | `ID;`          | none (read-only)        | p.10, fixed `0670` |
//!
//! # Wire formats used (batch 9: meters/status, 6 commands)
//!
//! | Command | Code | Query                    | Set                     | Notes |
//! |---------|------|---------------------------|--------------------------|-------|
//! | Info    | IF   | `IF;` → 28-byte composite | none (read-only)        | p.10, see [`ChannelStatusFields`] |
//! | Read meter | RM | `RM<selector 0-8>;` (selector read) | none (read-only) | p.15, meaning of `0`/`2` depends on `MS` |
//! | Radio info | RI | `RI<selector 0,3-7,A>;` (selector read) | none (read-only) | p.15, status-flag selectors, note the gap |
//! | Radio status | RS | `RS;` → 0/1 | none (read-only)  | p.16, 0=NORMAL MODE 1=MENU MODE |
//! | Meter select | MS | `MS;` → 0-5 | `MS<0-5>;`         | p.12, selects which meter `RM`'s `0`/`2` reads |
//! | PLL unlock | UL | `UL;` → 0/1 | none (read-only)     | p.18, 0=Lock 1=Unlock |
//!
//! `IF`'s composite payload was re-verified column-by-column against the
//! manual's page image directly (not just extracted text) per Wave 1's own
//! deferral note — see [`ChannelStatusFields`] for the settled field
//! boundaries and citation. Corrections found against the architect's
//! summary citations (`planning/architect/task_plan.md` §10.5): `RS`'s
//! detail box is entirely on printed p.16 (not "p.15-16" — the master
//! table entry is on p.3, the detail box on p.16 only), and `MS`'s detail
//! box is entirely on printed p.12 (not "p.12-13").
//!
//! # Wire formats used (`EX` menu, first sub-batch: shared plumbing + the 9
//! PTT/keying-relevant items)
//!
//! | Command | Code | Read              | Set                        | Notes |
//! |---------|------|--------------------|------------------------------|-------|
//! | Menu    | EX   | `EX<3-digit P1>;` (selector read) | `EX<3-digit P1><P2>;` | p.7-9, see [`ExMenuItem`]/[`EX_MENU_TABLE`] |
//!
//! `EX` is one composite command covering all 153 menu items (manual
//! p.7-9), not 153 separate 2-letter codes — per
//! `planning/architect/task_plan.md` §10.6. Its Read row (`EX<P1>;`) is
//! another "selector read" (same treatment as `Md`/`Sm`/`Rm`/`Ri` above):
//! structurally a 3-character `Set` to the parser, not a zero-width
//! `Query`. This sub-batch builds the shared plumbing (all 6 of the full
//! table's distinct P2 digit widths, confirmed by re-transcribing every
//! row's own "Digits" column, not just the architect's "~6" estimate — see
//! [`EX_SET_FORMS`]) plus exactly 9 `EX_MENU_TABLE` rows: **047** (AM PTT
//! SELECT), **048** (AM PORT SELECT), **060** (PC KEYING — the item this
//! wave's RTS/DTR CW-keying feature actually reads/writes), **071**/**072**
//! (DATA PTT/PORT SELECT), **076**/**077** (FM PKT PTT/PORT SELECT),
//! **108**/**109** (SSB PTT/PORT SELECT).
//!
//! # Wire formats used (`EX` menu, second sub-batch: items 001-046, minus 027)
//!
//! Adds 45 more `EX_MENU_TABLE` rows, menu numbers 001-046 (manual p.7),
//! except **027** "TIME ZONE" — explicitly skipped as unresolvable from
//! this manual (no wire-encoding formula given, unlike its signed-range
//! siblings 035/039 — see [`EX_MENU_TABLE`]'s doc comment), same treatment
//! as item 087 "RADIO ID". This sub-batch introduces the first
//! continuous-numeric-range menu items (vs. the first sub-batch's small
//! named pick-lists only) — see [`ExMenuValueKind`] for the resulting
//! `Enumerated`/`Range` value-encoding split, and [`EX_MENU_TABLE`]'s doc
//! comment for the full per-item table, citations, and judgment calls
//! (default-value policy, range-vs-enumerated classification, the
//! adjacent-but-out-of-range 068/069 digit-order misread caught during
//! verification). The remaining items (049-153, minus 087) are explicitly
//! out of scope for this task (later sub-batches, per §10.6's sizing
//! recommendation) — any other `P1` cleanly resolves to `"?;"` (see
//! [`ex_menu_item`]), never a panic.
//!
//! # Wire formats used (`EX` menu, third sub-batch: items 049-079)
//!
//! Adds 26 more `EX_MENU_TABLE` rows, menu numbers 049-079 (manual p.7-8,
//! printed p.7/p.8), minus **060/071/072/076/077** — already landed by the
//! first sub-batch (they fall inside this numeric range but were never
//! duplicated). Same two-method verification as prior sub-batches
//! (`pdftotext -layout` plus the rendered page image, read directly at high
//! resolution) — see [`EX_MENU_TABLE`]'s doc comment for the full per-item
//! table and citations.
//!
//! **068/069 digit-width discrepancy, definitively resolved — the prior
//! sub-batch's stated image-based resolution was itself wrong, not just the
//! `pdftotext` pass**: this task re-rendered printed p.8 (PDF page 9) at
//! 300 DPI and zoomed directly on the 068/069 rows. The manual's own printed
//! table unambiguously shows **068 "DATA HCUT FREQ" with Digits=1 and 069
//! "DATA HCUT SLOPE" with Digits=2** — i.e. the *same* 1/2 order
//! `pdftotext -layout` already reported, not the 2/1 order the second
//! sub-batch's findings claimed to have confirmed from the image. That
//! prior image-read was mistaken (row misalignment or similar), not this
//! task's re-check. However, the manual's literal printed Digits column is
//! **functionally impossible** for 068: its own P2 legend is `00: OFF,
//! 01: 700 Hz ~ 67: 4000 Hz` — encoding values up to `67` requires 2 ASCII
//! digits, which cannot fit in Digits=1. Every other `*HCUT FREQ`/
//! `*HCUT SLOPE` pair on this page, without exception (043/044 AM, 052/053
//! CW, 066/067's own immediately-preceding sibling-pair DATA LCUT, 092/093
//! and 094/095 RTTY, 102/103 and 104/105 SSB), is 2/1 — FREQ always wider
//! than SLOPE. Implemented as **068 Digits=2, 069 Digits=1** — the
//! functionally-necessary, sibling-pattern-corroborated reading — treating
//! the manual's printed `1`/`2` for this one row pair as a genuine
//! typesetting error (most likely an adjacent-row swap in the source
//! layout), not a real protocol difference. This is a judgment call, not a
//! manual-literal transcription, and is flagged for hardware verification
//! like any other judgment call in this table — but skipping both items as
//! "unresolvable" was rejected as the worse choice, since the swap
//! diagnosis is corroborated by two independent kinds of evidence (legend
//! arithmetic + universal sibling pattern), not a guess.
//!
//! **Continuous-range items** (049, 054, 057, 064, 065, 073, 075, 078, plus
//! the `*LCUT`/`*HCUT FREQ` pairs) reuse [`ExMenuValueKind::Range`]; **064/
//! 065 "OTHER DISP/SHIFT (SSB)"** are this sub-batch's first 5-digit signed
//! items (`-3000~+3000 Hz, 10 Hz steps`, magnitude width 4) — same
//! sign-then-magnitude encoding as 035/039, just a wider magnitude field,
//! confirming [`ExMenuValueKind::Range`]'s `signed` handling generalizes
//! with no code change. All widths this sub-batch needs (1, 2, 3, 4, 5) were
//! already present in [`EX_SET_FORMS`] (built ahead-of-need by the first
//! sub-batch) — no new `CommandForm` width was needed.
//!
//! The remaining items (080-153, minus 087) were explicitly out of scope for
//! that task; the fourth sub-batch below completes them.
//!
//! # Wire formats used (`EX` menu, fourth sub-batch: items 080-153, minus 087)
//!
//! Adds 71 more `EX_MENU_TABLE` rows: menu numbers 080-153 (manual p.7-9,
//! printed p.8-9), minus **087** "RADIO ID" (still permanently unresolvable
//! — literal dashes, no digit count given anywhere in the manual, same
//! treatment as the first sub-batch's finding) and minus **108/109** "SSB
//! PTT/PORT SELECT" — already landed by the first sub-batch (they fall
//! inside this numeric range but were never duplicated). This completes
//! `EX_MENU_TABLE`: every one of the 153 manual menu numbers now has a row
//! except 027 ("TIME ZONE") and 087 ("RADIO ID"), both permanently
//! unresolvable from this manual alone (see their own entries above/below).
//! Same two-method verification as prior sub-batches (`pdftotext -layout`
//! plus a 300 DPI rendered-page-image read, column-by-column) — see
//! [`EX_MENU_TABLE`]'s doc comment for the full per-item table and
//! citations. All widths this sub-batch needs (1, 2, 3, 4, 5, 8) were
//! already present in [`EX_SET_FORMS`] — no new `CommandForm` width needed.
//!
//! **100 "RTTY SHIFT FREQ", a genuine manual typo resolved via
//! corroboration, not a skip**: the manual's own printed P2 legend reads
//! `1: 170 Hz  1: 200 Hz  2: 425 Hz  3: 850 Hz` — a duplicate `1:` label,
//! confirmed identical on both `pdftotext -layout` and a 300 DPI page-image
//! re-render (not an extraction artifact). Every other 4-value single-digit
//! selector in this 153-row table, without exception (e.g. 012, 016, 029,
//! 030, 031, 032, 090), is zero-based (`0..=N-1`); the sole 2-exceptions
//! (072/077's "PORT SELECT" family, already documented, and 101 "RTTY MARK
//! FREQ" itself, a genuine 2-value 1-based field) are a narrower,
//! independently-documented pattern that does not extend to a 4-value
//! field like this one. Real-world amateur-radio RTTY convention
//! independently corroborates the same reading: 170 Hz is the near-universal
//! default/standard FSK shift, matching this table's own convention of `0`
//! as the neutral/default/most-common option elsewhere (e.g. color/mode
//! selectors). Resolved as **`0: 170 Hz  1: 200 Hz  2: 425 Hz  3: 850 Hz`**
//! (the first printed `1:` treated as a typesetting error for `0:`) — two
//! independent corroborating signals, not a guess, same rigor standard as
//! the third sub-batch's 068/069 resolution. Implemented as
//! `Enumerated(&["0", "1", "2", "3"])`.
//!
//! **147 "DATA VOX DELAY" step assumption, a documented judgment call**:
//! this item's own P2 cell reads `30 ~ 3000 msec (P2 = 0030 ~ 3000)` — no
//! step note, confirmed not a rendering/column-truncation artifact via the
//! same 300 DPI re-render. Its sibling **144 "VOX DELAY"** (identical
//! quantity, MIC vs. DATA variant — the same duality batch 5's `VD`
//! command finding already ties to menu items 144/147 by manual citation)
//! *does* state `10 msec/step` for the same `0030~3000` range. Applying
//! `step=10` to 147 too (rather than a literal `step=1`) is a judgment
//! call, not a manual-stated fact for 147's own row — flagged for review,
//! not silently assumed, but implemented rather than left at the
//! functionally-implausible literal default of allowing every millisecond
//! value.
//!
//! **116 "SCP SPAN FREQ" has a documented gap**: legal P2 values are
//! `03`-`07` only (`00`-`02` absent from the manual, no legend entry) —
//! transcribed exactly via `Enumerated(&["03", "04", "05", "06", "07"])`,
//! same treatment as `RI`'s selector gap (batch 9) and item 028's `GPS/232C
//! SELECT` gap (second sub-batch).
//!
//! **Parametric-EQ sextet (119-136)**: 6 structurally-identical
//! FREQ/LEVEL/BWTH triples (3 "PRMTRC EQ1-3" + 3 "P-PRMTRC EQ1-3"). FREQ
//! items are contiguous-from-`00` named frequency points (`00`=OFF,
//! `01..=N`=specific Hz points) — modeled as unsigned [`ExMenuValueKind::Range`]
//! over the contiguous wire integers, same convention already established
//! for `*LCUT`/`*HCUT FREQ` items (a plain `00..=max` integer range, not an
//! exhaustive `Enumerated` list of named Hz labels — the display-Hz meaning
//! of each integer is not itself stored). LEVEL items are signed
//! `-20..=+10` (manual's own `(P2 = -20 ~ -00 or +00 ~ +10)` formula, same
//! sign-then-magnitude shape as 035/039/064/065). BWTH items are unsigned
//! `01..=10`.
//!
//! # Wire formats used (batch 2: memory channel records, `MC MR MW MT`)
//!
//! | Command | Code | Query/Read              | Set/Write                        | Notes |
//! |---------|------|--------------------------|------------------------------------|-------|
//! | Memory channel select | MC | `MC;` → `MC<3 digits>;` | `MC<3 digits>;`, 001-117 | p.11 |
//! | Memory channel read   | MR | `MR<3 digits>;` (selector read) → 28-byte composite | none (read-only) | p.12, shares `IF`'s [`ChannelStatusFields`] P1-P10 shape exactly |
//! | Memory channel write  | MW | none (write-only, no read/answer) | `MW<25-byte body>;` (same [`ChannelStatusFields`] shape as `MR`'s answer) | p.12 |
//! | Memory channel write/tag | MT | `MT<3 digits>;` (selector read) → 41-byte composite | `MT<38-byte body>;` ([`ChannelStatusFields`] shape + fixed byte + 12-char tag) | p.12, see [`MemoryChannelRecord`] |
//!
//! Per the architect's cross-batch finding (`planning/architect/task_plan.md`
//! §10.5), `MR`'s answer and `MW`'s Set both carry the **identical** 25-byte
//! P1-P10 body `IF`'s answer already established — re-verified column-by-
//! column against the manual page image, not assumed. `MT` is a genuine
//! superset (same 25 bytes + a fixed reserved byte + the 12-character ASCII
//! tag) — see [`MemoryChannelRecord`]'s doc comment for the full citation,
//! the two narrow divergences from `IF`'s use of the same shape (channel
//! range, P7/select semantics), and the tag's character-set/padding
//! judgment call.
//!
//! # Wire formats used (batch 1: VFO/split/memory quick-ops, `AB BA AM VM MA
//! CH QI QR QS SV`)
//!
//! | Command | Code | Read/Query | Set              | Notes |
//! |---------|------|------------|-------------------|-------|
//! | VFO-A to VFO-B  | AB | none | `AB;` (Action, 0-width) | p.4, copies `vfo_a_hz` into `vfo_b_hz` |
//! | VFO-B to VFO-A  | BA | none | `BA;` (Action)          | p.4, copies `vfo_b_hz` into `vfo_a_hz` |
//! | VFO-A to memory | AM | none | `AM;` (Action)          | p.4, stores VFO-A into the `MC`-selected channel |
//! | `[V/M]` key     | VM | none | `VM;` (Action)          | p.18, see judgment call below |
//! | Memory to VFO-A | MA | none | `MA;` (Action)          | p.11, recalls the `MC`-selected channel into VFO-A |
//! | Channel up/down | CH | none | `CH<0/1>;`              | p.5, steps `MC`'s selected channel, wraps 1..=117 |
//! | QMB store       | QI | none | `QI;` (Action)          | p.14, stores VFO-A into a dedicated QMB slot |
//! | QMB recall      | QR | none | `QR;` (Action)          | p.14, recalls the QMB slot into VFO-A |
//! | Quick split     | QS | none | `QS;` (Action)          | p.15, toggles `Ft991aState::split` |
//! | Swap VFO        | SV | none | `SV;` (Action)          | p.17, swaps `vfo_a_hz`/`vfo_b_hz` |
//!
//! All ten are write-only triggers per the manual's own p.3 master table
//! (Set O, Read X, Ans X for every one of these ten rows) — confirmed
//! against each command's own per-command Set/Read/Answer box too (manual
//! printed p.4-5, p.11, p.14-15, p.17-18; PDF pages 5-6, 12, 15-16, 18-19
//! per the documented "+1" cover-page offset). Nine of the ten (`CH` is the
//! exception, with a required 1-digit selector) carry **no parameter at
//! all** — genuine `CommandOperation::Action` commands (`cat-framework`'s
//! dedicated zero-width-trigger-with-no-response operation kind), not
//! `Query`/`Set` — the first use of `action_forms` in this crate's command
//! table (mirrors `ts570d`'s own `ACTION` const for its analogous `TX`/`RX`/
//! `RC`/`RU`/`RD`/`UP`/`DN` triggers).
//!
//! **`VM`/`AM` manual heading inconsistency — flagged, then resolved via
//! corroborating evidence, not from the wire-format box alone.** The master
//! table (p.3) names `VM` `"[V/M] KEY FUNCTION"`, but `VM`'s own
//! per-command box (printed p.18) is headed **"VFO-A TO MEMORY CHANNEL"**
//! — copied verbatim from `AM`'s own heading (printed p.4). Per this
//! task's brief, the wire-format Set/Read/Answer boxes were checked first
//! to see if they disambiguate the two commands' actual meaning — they do
//! **not**: both `AM`'s and `VM`'s boxes are identically shaped
//! (`A M ;` / `V M ;`, zero-width Set, blank Read, blank Answer; no P1/P2
//! columns at all for either), so the wire shape alone cannot distinguish
//! "store" from "toggle mode". Resolved instead via three corroborating,
//! independent signals: (1) `grep`ing the full 20-page manual text for `[`
//! finds exactly **one** bracketed entry in the entire document — `VM`'s
//! own master-table name, `"[V/M] KEY FUNCTION"` — strongly suggesting this
//! naming convention specifically marks "emulates pressing a physical
//! front-panel key" (the manual's `[...]` bracket notation matches how
//! front-panel button legends are printed elsewhere on the radio), distinct
//! from ordinary command-name prose used for every other row; (2) it would
//! be redundant for the manual to define two CAT commands with byte-for-
//! byte identical wire shapes and identical purposes — `AM` already
//! unambiguously covers "store VFO-A into memory" via its own equally
//! explicit, non-bracketed heading; (3) well-established real-world Yaesu
//! operating behavior (outside this manual): the physical `[V/M]` button on
//! Yaesu transceivers including the FT-991A toggles the radio between VFO
//! operation and Memory-channel (recall) operation — it does not store
//! anything. This implementation therefore treats `VM` as **toggling
//! `Ft991aState::channel_select`** between `0` (VFO) and `1` (Memory) —
//! reusing `IF`'s own P7 VFO/Memory/QMB select field (batch 9) rather than
//! inventing new state, a deliberate design choice since `VM`'s toggle and
//! `IF`'s P7 report the same underlying concept. Any `channel_select` value
//! other than `0` is treated as "not VFO" and toggled back to `0`. **This
//! is a documented judgment call, not a manual-proven fact** — the
//! wire-format box genuinely does not disambiguate `AM` from `VM`, and the
//! resolution rests on the bracket-notation heuristic and outside
//! real-hardware knowledge, not on anything printed in `VM`'s own box.
//! Flagged for architect/hardware review, not silently assumed correct.
//!
//! **State-model constraint inherited from Wave 1, not new to this task**:
//! `Ft991aState` has a single `mode: u8` field, not one per VFO. `AB`/`BA`/
//! `SV` therefore only copy/swap `vfo_a_hz`/`vfo_b_hz` (frequency), since
//! there is no separate VFO-B mode to read or write. `AM`/`MA` (which
//! interact with `MemoryChannelRecord`, which DOES carry its own `mode`)
//! copy/restore `mode` alongside frequency and the clarifier/tone/offset
//! fields, giving full symmetry with a real memory-channel store/recall.
//!
//! **`CH`'s up/down wrap-around, a documented judgment call**: the manual
//! (printed p.5) gives no boundary behavior for `CH` at channel 1 or 117.
//! This implementation wraps (117→1 on UP, 1→117 on DOWN) rather than
//! clamping — a reasonable, commonly-seen radio convention, not itself
//! manual-cited.
//!
//! **`QI`/`QR` (QMB store/recall), a new dedicated storage slot**: `IF`'s
//! P7 legend (batch 9) already lists `3`=QMB and `4`=QMB-MT as select
//! values distinct from `1`=Memory, confirming the Quick Memory Bank is a
//! separate single-slot storage location, not one of the 117 numbered
//! memory channels `MC`/`AM`/`MA` address. Modeled as
//! `Ft991aState::qmb: MemoryChannelRecord`, storing/recalling VFO-A's
//! frequency/mode/clarifier/tone/offset the same way `AM`/`MA` do for a
//! numbered channel (reusing [`MemoryChannelRecord`] for convenient
//! symmetry, even though `QI`/`QR` never touch its `tag` field).
//!
//! **`QS` (Quick Split), a documented judgment call**: the manual gives
//! `QS` no Read/Answer row at all (write-only trigger, like the other
//! Action commands here) and no separate "split on"/"split off" command
//! exists anywhere in the 91-command master table. Modeled as a plain
//! boolean toggle (`Ft991aState::split`), flipped on each `QS;` — the most
//! defensible reading of a parameterless "quick" toggle action absent any
//! stated on/off semantics.
//!
//! # Wire formats used (batch 3: clarifier/RIT-XIT + tone + IF-shift, `RT RC
//! RD RU XT CN CT IS`)
//!
//! | Command | Code | Query/Read | Set | Notes |
//! |---------|------|------------|-----|-------|
//! | RX Clarifier on/off | RT | `RT;` | `RT<0/1>;` | p.16, gates `clarifier_offset_hz`'s effect on RX |
//! | Clarifier clear      | RC | none | `RC;` (Action) | p.15, zeroes `clarifier_offset_hz` only |
//! | Clarifier down       | RD | none | `RD<4 digits>;` (0000-9999 Hz) | p.15, sets `clarifier_offset_hz = -<value>` |
//! | Clarifier up         | RU | none | `RU<4 digits>;` (0000-9999 Hz) | p.16 ("RX CLARIFIER PLUS OFFSET"), sets `clarifier_offset_hz = +<value>` |
//! | TX Clarifier on/off  | XT | `XT;` | `XT<0/1>;` | p.19, gates the same `clarifier_offset_hz`'s effect on TX |
//! | CTCSS/DCS number     | CN | `CN0<0/1>;` (selector read) | `CN0<0/1><3 digits>;` | p.5, see [`CTCSS_TONES`]/[`DCS_CODES`] |
//! | CTCSS/DCS mode       | CT | `CT0;` (selector read) | `CT0<0-4>;` | p.5, reuses `Ft991aState::tone_status` (already landed for `IF`'s P8) |
//! | IF-shift             | IS | `IS0;` (selector read) | `IS0<sign><4 digits>;` | p.10, see the resolved P2-width discrepancy below |
//!
//! **RX/TX clarifier relationship — confirmed from the manual, not
//! assumed**: `RT`'s own per-command box (printed p.16) is headed just
//! "CLAR" with `P1  0: RX Clarifier "OFF"  1: RX Clarifier "ON"`, and
//! `XT`'s own box (printed p.18, "TX CLAR") is `P1  0: TX CLAR "OFF"  1: TX
//! CLAR "ON"` — both simple, independent on/off flags, each shaped exactly
//! like `TX`'s own `<0/1>` Set (reusing [`QUERY0`]/[`SET_1`]). There is
//! **no** `XD`/`XU` pair anywhere in the master table (p.3) alongside
//! `RD`/`RU` — confirming (not merely assuming) that the FT-991A has **one**
//! shared clarifier offset value, not independent RX/TX offsets: `RT`/`XT`
//! are independent *gates* on whether that single offset is applied to
//! RX/TX respectively (the standard ham-radio RIT/XIT pattern — one
//! offset dial, two independent apply-to-RX/apply-to-TX switches), while
//! `RD`/`RU`/`RC` adjust/clear the one shared value. This is also exactly
//! the shape `IF`'s own P3/P4/P5 fields already model
//! (`ChannelStatusFields::clarifier_offset_hz`/`rx_clarifier_on`/
//! `tx_clarifier_on`, landed in batch 9/2 before any batch-3 `Set` command
//! existed to change them) — `Ft991aState` already carries exactly these
//! three fields with a doc comment noting "clarifier commands are batch 3",
//! this task wires the `Set` side up rather than inventing new state.
//! `RU`'s own heading ("RX CLARIFIER PLUS OFFSET") was flagged by the task
//! brief as a possible RX-only signal — cross-checked against `IF`'s P3
//! legend (`"Clarifier Direction +: Plus Shift, --: Minus Shift"`, no
//! RX/TX qualifier at all) and the absence of any `XD`/`XU` command: the
//! "RX" in `RU`'s heading is almost certainly a naming leftover/shorthand,
//! not evidence of a second, TX-specific offset value — there is exactly
//! one `clarifier_offset_hz`, and `RT`/`XT` independently gate its
//! application.
//!
//! **`RD`/`RU`'s direction encoding, confirmed via `IF`'s own P3 field, not
//! guessed**: `IF`'s P3 legend (already landed, batch 9) reads
//! `"Clarifier Direction +: Plus Shift, --: Minus Shift"` immediately
//! followed by `"Clarifier Offset: 0000-9999 (Hz)"` — i.e. IF's own
//! composite answer already represents the clarifier as sign+magnitude,
//! matching `RD`'s ("DOWN" → minus direction) and `RU`'s ("PLUS OFFSET" →
//! plus direction) command names exactly. Modeled as **absolute sets**, not
//! incremental steps: both `RD`'s and `RU`'s own boxes give P1 an explicit
//! `0000-9999 (Hz)` magnitude field (unlike `CH`'s pure `0`/`1` direction
//! selector with no magnitude, batch 1's genuine incremental-step
//! command) — `RD<magnitude>;` sets `clarifier_offset_hz = -<magnitude>`,
//! `RU<magnitude>;` sets `clarifier_offset_hz = +<magnitude>`, overwriting
//! whatever was there before, not adding to it. This reading is the most
//! literal one supported by the wire format itself; the manual never uses
//! the word "increment" or "step" for either command. Flagged as the
//! judgment call it is (an alternative "step by magnitude" reading cannot
//! be fully ruled out from text alone), but absolute-set is what the
//! explicit magnitude field most directly supports.
//!
//! **`RC` (CLAR CLEAR), a documented judgment call**: zero-width Action
//! trigger (manual p.15, Set O Read X Ans X, no P1 at all). Modeled as
//! zeroing `clarifier_offset_hz` only — `RT`/`XT`'s on/off gates are left
//! untouched, matching how a physical "CLR" button next to a RIT/XIT dial
//! typically zeroes the dial reading without disabling RIT/XIT itself. Not
//! itself manual-cited beyond "the manual gives RC no parameter to specify
//! otherwise."
//!
//! **`CT`, a genuine selector-read reusing already-landed state**: manual
//! p.5's box (`C T P1 P2 ;` Set/Answer, `C T P1 ;` Read) is structurally
//! identical to `MD`'s own selector-read shape (1-char read, 2-char write)
//! — reuses that same `handle_command`-disambiguates-by-`params.len()`
//! pattern. `CT`'s P2 (`0`:CTCSS OFF `1`:CTCSS ENC/DEC `2`:CTCSS ENC
//! `3`:DCS ENC/DEC `4`:DCS ENC) is byte-for-byte the same legend `IF`'s P8
//! already uses (`Ft991aState::tone_status`, landed batch 9 with a doc
//! comment explicitly noting "tone commands are batch 3") — this task wires
//! `CT`'s `Set` onto that existing field rather than adding a new one.
//!
//! **`CN`, the two lookup tables**: manual p.5's box (`C N P1 P2 P3 P3 P3
//! ;` Set/Answer, `C N P1 P2 ;` Read) selects, via P2, whether P3 indexes
//! [`CTCSS_TONES`] (`P2=0`, `000`-`049`, Table 1) or [`DCS_CODES`] (`P2=1`,
//! `000`-`103`, Table 2) — a genuine selector read like `CT`/`MD`, but with
//! P2 itself carried in both the read and write forms (2-char read, 5-char
//! write) rather than fixed. Both tables were transcribed in full from the
//! manual's own Table 1/Table 2 images (printed p.6) and cross-checked
//! against the well-known standard 50-tone CTCSS / 104-code DCS lists used
//! industry-wide (not FT-991A-specific — the same 50/104 values appear
//! across Yaesu/Kenwood/Icom equipment) as an independent verification
//! pass; one single-digit misread on the initial image pass (DCS index 078:
//! `465` vs. the correct `466`, immediately adjacent to index 077's `465` —
//! an easy 5/6 visual confusion in small print) was caught and corrected
//! this way, flagged here rather than silently fixed. `Ft991aState` gains
//! two new raw-index fields, `ctcss_tone_number: u8` (0-49) and
//! `dcs_code_number: u8` (0-103) — mirroring `mode: u8`'s own
//! "raw wire index, not the domain type" pattern (the domain-typed Hz/code
//! conversion lives at the `Ft991a` client boundary, see `ft991a.rs`).
//!
//! **`IS`, a resolved manual discrepancy — P2 is 4 digits, not the 3 the
//! per-command box's column diagram shows**: `IS`'s own box (printed p.10)
//! gives the Set/Answer row as `I S P1 -/+ P2 P2 P2 ;` — only **three** P2
//! cells — but the manual's own general "Parameters" worked example
//! (printed p.2) states outright "when the correct parameter is
//! `IS0+1000` (IF SHIFT)" and separately calls out `IS0+100;` (3 P2 digits)
//! as an error ("Not enough digits (Only three frequency digits given)"),
//! confirming the *correct* form needs **four**. This is independently
//! corroborated by the box's own stated range, `-1200 ~ +1200 Hz` — `1200`
//! itself needs 4 digits, which a 3-digit field could never represent
//! (max `999`). Two independent signals (the p.2 worked example + error
//! catalog, and the box's own numeric range) agree with each other and
//! disagree only with the box's column-count diagram, which is treated as
//! the error here (almost certainly a one-cell drafting omission) —
//! resolved in favor of 4 digits, not silently: `IS`'s write width is
//! `1(P1) + 1(sign) + 4(magnitude) = 6` characters, matching the p.2
//! `"IS0+1000;"` example exactly (body `"0+1000"`, 6 characters). The
//! `-1200~+1200 Hz (20 Hz steps)` range is enforced as `magnitude <= 1200
//! && magnitude % 20 == 0` — the "20 Hz steps" text taken literally, a
//! judgment call not independently re-confirmed by a second manual signal.
//! New state: `Ft991aState::if_shift_hz: i16`.
//!
//! # Wire formats used (batch 4: keyer/CW/break-in, `KM KP KR KS KY CS ZI
//! BI SD`)
//!
//! | Command | Code | Query/Read | Set | Notes |
//! |---------|------|------------|-----|-------|
//! | Keyer memory   | KM | `KM<1-digit channel>;` (selector read) | `KM<1-digit channel><1-50 char message>;` | p.10, see [`Ft991aState::keyer_memories`] |
//! | Key pitch      | KP | `KP;` | `KP<2 digits>;` | p.10, 00-75 maps 300-1050 Hz, 10 Hz steps |
//! | Keyer on/off   | KR | `KR;` | `KR<0/1>;` | p.10 |
//! | Key speed      | KS | `KS;` | `KS<3 digits>;` | p.11, 004-060 WPM |
//! | CW keying      | KY | none (write-only) | `KY<1 char>;` | p.11, triggers stored-memory playback — see below |
//! | CW spot        | CS | `CS;` | `CS<0/1>;` | p.6 |
//! | Zero in        | ZI | none (write-only, zero-width) | `ZI;` (Action) | p.18, "(CW AUTO ZERO IN Function)" |
//! | Break-in       | BI | `BI;` | `BI<0/1>;` | p.5 |
//! | CW break-in delay | SD | `SD;` | `SD<4 digits>;` | p.16, 0030-3000 msec |
//!
//! All nine wire shapes were re-verified directly against each command's own
//! per-command Set/Read/Answer box (manual printed p.5-6, p.10-11, p.16,
//! p.18; PDF pages 6-7, 11-12, 17, 19 per the documented "+1" cover-page
//! offset), not just the p.3 master table, and cross-checked with
//! `pdftotext -layout` against the rendered page images (no discrepancies
//! found between the two extraction methods for any of the nine).
//!
//! **`KM`, a genuine variable-width "selector read" — the batch's highest-
//! risk item**: manual p.10's box gives Set as `K M P1 P2 P2 P2 P2 ~ P2 ;`
//! (`P1`: 1-digit channel, 1-5; `P2`: "Message Characters (up to 50
//! characters)"), Read as `K M P1 ;`, Answer the same shape as Set. Same
//! "selector read" treatment as `MD`/`EX`/`MT` above: a bare 1-character
//! parameter (`P1` alone) is the read, anything longer is a write. Unlike
//! `EX`'s handful of fixed widths, `KM`'s P2 is genuinely free-length (not
//! one of a small enumerated set of widths), so [`KM_SET_FORMS`] uses
//! `cat-framework`'s `CommandForm::variable` (a range, not a discrete list)
//! for the write form — `2..=51` total characters (1-digit `P1` + 1 to 50
//! message characters) — rather than 50 separate fixed-width entries.
//! **Documented consequence, not manual-stated**: since the read form is
//! exactly 1 character wide, the write form's minimum width must be at
//! least 2 to stay structurally distinguishable from a read — meaning this
//! implementation cannot accept a 0-character message (there is no way to
//! *clear* a channel back to empty via `KM` alone in this implementation;
//! the manual states no minimum message length itself). Message content is
//! validated against the same general-parameter character-set rule already
//! used for `MT`'s tag (manual p.2: printable ASCII space (0x20) through
//! tilde (0x7E), excluding `;`) via [`is_valid_ascii_wire_content`] (renamed
//! from this batch's predecessor `is_valid_tag_wire`, since the same
//! validation now backs two unrelated variable-ASCII-content fields, not
//! just `MT`'s tag). No factory default is stated for any of the 5
//! channels; empty string (`""`, i.e. "vacant") is this implementation's
//! default, same category of open item as `MemoryChannelRecord::tag`'s
//! default.
//!
//! **`KY`, related to but distinct from the not-yet-consumed RTS/DTR
//! CW-keying feature — kept conceptually and code-wise separate, per this
//! task's explicit brief**: manual p.11's box gives Set as `K Y P1 ;`
//! (1-char `P1`), blank Read row, blank Answer row — confirmed write-only
//! against the p.3 master table (`Set O Read X Ans X`). `P1`'s legend is
//! `1`-`5`: "Keyer Memory 'N' Playback" (`N`=`P1`), `6`-`9`,`A`: "Message
//! Keyer 'N' Playback" (`N`=`P1`-5, with `A`→`N`=5) — **`KY` triggers the
//! radio to autonomously transmit a pre-stored
//! [`Ft991aState::keyer_memories`] message** (stored via `KM`), in one of
//! two playback "families." This is unrelated to `radio-cat-rs`'s
//! already-landed `ModemControlLines`/RTS-DTR trait
//! (`planning/architect/task_plan.md` §10.2-10.4, not yet consumed on this
//! repo's side): that feature is real-time, PC-driven keying of arbitrary
//! Morse timing via serial control lines (no CAT command involved, no
//! pre-stored message), whereas `KY` is entirely CAT-driven, addresses only
//! pre-stored `KM` content, and carries no timing information at all. **The
//! two "playback family" values are NOT two independent storage systems**,
//! confirmed by cross-referencing the already-landed `EX` menu items
//! 018-022 "CW MEMORY 1"-"5" (`EX` first sub-batch, manual p.7-9), each of
//! which selects `0: TEXT` or `1: MESSAGE` playback mode for the *same*
//! numbered `KM` channel — `KY`'s `1`-`5`/`6`-`A` split almost certainly
//! selects between these two playback modes for the one 5-channel `KM`
//! store, not a second store. This implementation therefore does **not**
//! add a duplicate 5-channel message array for "Message Keyer" —
//! [`KeyerPlaybackMode`] just tags which of the two playback families a
//! `KY` trigger names, both referring to the same
//! [`Ft991aState::keyer_memories`] entry. **`KY` mutates no persisted
//! `Ft991aState` field** — there is nothing meaningful for this
//! software-only emulator to represent about an asynchronous audio
//! playback event (no simulated sidetone/RF output exists anywhere in this
//! crate); success is observable only via the returned
//! `CommandOutcome::events` (see the test), a documented judgment call, the
//! same treatment `ZI` below gets for the same reason.
//!
//! **`ZI`, likewise event-only, no persisted state**: manual p.18's box is
//! `Z I ;`, zero-width, blank Read/Answer rows — confirmed write-only
//! zero-width `Action` (`Set O Read X Ans X`), same shape as batch 1's nine
//! Action triggers. Its only annotation is `"(CW AUTO ZERO IN Function)"` —
//! a real FT-991A auto-zeroes the receive frequency against an incoming CW
//! signal's actual pitch; this emulator has no simulated received-signal
//! frequency anywhere to zero-beat against, so — like `KY` above — there is
//! nothing to mutate. Modeled as a pure acknowledgment, observable only via
//! `CommandOutcome::events`.
//!
//! **`KP`/`KR`/`KS`/`CS`/`BI`/`SD`, plain query/set pairs, no residual
//! ambiguity**: all six are shaped exactly like `Tx`/`Ps`/`Rt`/`Xt` above
//! (zero-width Query, fixed-width Set, both O per the p.3 master table).
//! None has a manual-stated factory default; this implementation's
//! per-field defaults (documented on each new [`Ft991aState`] field below)
//! are arbitrary choices, same category of open item as `meter_select`'s
//! default (batch 9).
//!
//! New state: `Ft991aState::{keyer_memories, key_pitch, keyer_on,
//! key_speed, cw_spot_on, break_in_on, cw_break_in_delay_ms}`.
//!
//! # Wire formats used (batch 5: scan/VOX/busy, `SC VX VD VG BY`)
//!
//! | Command | Code | Query/Read | Set | Notes |
//! |---------|------|------------|-----|-------|
//! | Scan       | SC | `SC;` → 0/1/2 | `SC<0/1/2>;` | p.16, 0=OFF 1=ON(UP) 2=ON(DOWN) |
//! | VOX status | VX | `VX;` | `VX<0/1>;` | p.18 |
//! | VOX delay time | VD | `VD;` | `VD<4 digits>;` | p.17, 0030-3000 msec, 10 msec steps — see below, meaning depends on `EX` menu 142 |
//! | VOX gain   | VG | `VG;` | `VG<3 digits>;` | p.18, 000-100 |
//! | Busy       | BY | `BY;` → `BY<P1><P2>;` | none (read-only) | p.5, P1=RX busy 0/1, P2 fixed `0` |
//!
//! All five wire shapes were re-verified directly against each command's own
//! per-command Set/Read/Answer box (manual printed p.5, p.16-18; PDF pages
//! 6, 17-19 per the documented "+1" cover-page offset), not just the p.3
//! master table, cross-checked with `pdftotext -layout` against the rendered
//! page images (no discrepancies found between the two extraction methods
//! for any of the five).
//!
//! **`SC`, a plain 3-valued Set/Query pair — NOT the same shape as `TX`'s
//! 3-valued answer**: manual p.16's box gives Set as `S C P1 ;` (`P1`:
//! `0`=Scan "OFF", `1`=Scan "ON" (UP ward), `2`=Scan "ON" (DOWN ward)),
//! Read as `S C ;`, Answer the same shape as Set — confirmed read/write per
//! the p.3 master table (`Set O Read O Ans O`). Unlike `TX`'s `2` (an
//! answer-only value this state machine never produces via `Set`), all
//! three of `SC`'s values are legally settable — this is a plain three-way
//! enumerated state, modeled as [`ScanState`] at the `Radio` trait/client
//! boundary (mirroring [`TxState`](crate::radio_trait::TxState)'s
//! `TryFrom<u8>` shape) and as a raw `u8` (`Ft991aState::scan_state`, same
//! "raw wire digit, not the domain type" pattern `cat_tx`/`mode` already
//! use) in emulator state.
//!
//! **`VX`/`VG`, plain query/set pairs, no residual ambiguity**: manual
//! p.18's `VX` box (`V X P1 ;`, `P1` 0/1) is shaped exactly like `BI`/`CS`
//! above; `VG`'s box (`V G P1 P1 P1 ;`, `P1` 000-100) is shaped exactly
//! like `KS`'s 3-digit range. Neither has a manual-stated factory default;
//! `false`/`0` are this implementation's arbitrary choices, same category
//! of open item as this crate's other undocumented defaults.
//!
//! **`VD`, the batch's highest-risk item — its own doc note is transcribed
//! and preserved, not silently dropped, per this task's explicit brief**:
//! manual p.17's box (`V D P1 P1 P1 P1 ;`, `P1` 0030-3000, 10 msec
//! multiples) carries a doc note printed directly under the wire diagram,
//! transcribed verbatim: *"VD command has different parameters to be
//! changed according to the setting of Menu item '142 VOX SELECT'. 'MIC':
//! VOX DELAY. 'DATA': DATA VOX DELAY."* Cross-referenced against the
//! already-landed `EX` menu table (`ft991a_radio.rs`'s [`EX_MENU_TABLE`]) —
//! confirmed via `pdftotext -layout` of manual p.18 (PDF p.19) that menu
//! item **142 "VOX SELECT"** (`0: MIC 1: DATA`, 1 digit) is a real,
//! distinct `EX` menu row, alongside two further items that echo the same
//! MIC-vs-DATA split for the *adjacent* gain/anti-VOX settings: **143**
//! "VOX GAIN" (000-100, matching `VG`'s own range exactly), **144** "VOX
//! DELAY" (30-3000 msec, matching `VD`'s own range exactly), **146** "DATA
//! VOX GAIN", and **147** "DATA VOX DELAY" — i.e. the front-panel menu
//! system models `VD`'s (and, by the same adjacent-item pattern, possibly
//! `VG`'s) MIC/DATA duality as **two separate stored settings** (items
//! 144/147), not one value whose *interpretation* toggles. **Menu item 142
//! is explicitly NOT implemented by this task** (out of scope per the task
//! brief — only 9 `EX` items have landed so far, in a different sub-batch,
//! and 142/143/144/146/147 are not among them) — this emulator therefore
//! has no CAT-reachable way to select MIC vs. DATA, and cannot expose two
//! separate stored values the way the front panel's menu system implies it
//! might. `Ft991aState::vox_delay_ms` and `Ft991aState::vox_gain` are each
//! modeled as **one** shared value, addressed unconditionally by `VD`/`VG`
//! regardless of what menu 142 would (if implemented) currently select —
//! **the same "meaning depends on an EX menu setting not yet CAT-reachable"
//! category of open item [`Ft991a::get_tx_state`](crate::ft991a::Ft991a::get_tx_state)'s
//! doc comment already flags for `TX`'s 3-valued answer** (there, the `2`
//! value's real-world cause — front panel, footswitch, VOX — is likewise
//! outside what any landed CAT command alone can distinguish). Documented
//! on [`Ft991aState::vox_delay_ms`] and repeated on
//! [`Ft991a::get_vox_delay`](crate::ft991a::Ft991a::get_vox_delay)/
//! [`Ft991a::set_vox_delay`](crate::ft991a::Ft991a::set_vox_delay)'s own doc
//! comments, per this task's explicit instruction to state the dependency
//! plainly rather than hide it. **`VG`'s own box carries no equivalent doc
//! note** (only `VD`'s does) even though menu 143/146 suggest the same
//! MIC/DATA duality could plausibly apply to VOX gain too — flagged here as
//! an observation, not implemented as an assumed dependency, since nothing
//! on `VG`'s own manual page states it.
//!
//! **`BY`, read-only, no CAT-reachable driver — same simplification
//! category as `RI`'s status bits**: manual p.5's box has a blank Set wire
//! row (only the P1/P2 legend text, no `B Y P1 P2 ;` diagram under "Set" —
//! confirmed write-incapable against the p.3 master table, `Set X Read O
//! Ans O`), Read `B Y ;`, Answer `B Y P1 P2 ;` (`P1`: 0/1 RX busy off/on,
//! `P2`: fixed `0`, not itself a meaningful reported value). This emulator
//! has no simulated received-signal/squelch-open condition anywhere (the
//! same gap [`Ft991aState::ri_status`]'s doc comment already notes for
//! `RI`), so `Ft991aState::rx_busy` always reports `false` — a documented
//! simplification, not a manual-specified default.
//!
//! New state: `Ft991aState::{scan_state, vox_on, vox_gain, vox_delay_ms,
//! rx_busy}`.
//!
//! # Wire formats used (batch 6: attenuator/preamp/noise/AGC/notch/
//! filter-width, `RA PA NB NL NR RL GT CO BP BC NA SH`)
//!
//! | Command | Code | Query/Read | Set | Notes |
//! |---------|------|------------|-----|-------|
//! | RF attenuator | RA | `RA0;` → `RA0<P2>;` | `RA0<0/1>;` | p.15, selector read |
//! | Pre-amp (IPO) | PA | `PA0;` → `PA0<P2>;` | `PA0<0-2>;` | p.14, 0=IPO 1=AMP1 2=AMP2 |
//! | Noise blanker status | NB | `NB0;` → `NB0<P2>;` | `NB0<0/1>;` | p.13, selector read |
//! | Noise blanker level | NL | `NL0;` → `NL0<P2>;` | `NL0<3 digits>;` | p.13, 000-010 |
//! | Noise reduction | NR | `NR0;` → `NR0<P2>;` | `NR0<0/1>;` | p.13, selector read |
//! | Noise reduction level | RL | `RL0;` → `RL0<P2>;` | `RL0<2 digits>;` | p.15, 01-15 |
//! | AGC function | GT | `GT0;` → `GT0<P3>;` | `GT0<0-4>;` | p.10, write/report domain mismatch, see below |
//! | Contour | CO | `CO0<P2>;` → `CO0<P2><4 digits>;` | `CO0<P2><4 digits>;` | p.5, 4-item selector, see below |
//! | Manual notch | BP | `BP0<P2>;` → `BP0<P2><3 digits>;` | `BP0<P2><3 digits>;` | p.5, 2-item selector, see below |
//! | Auto notch | BC | `BC0;` → `BC0<P2>;` | `BC0<0/1>;` | p.4, selector read |
//! | Narrow | NA | `NA0;` → `NA0<P2>;` | `NA0<0/1>;` | p.13, wire code confirmed `NA` despite a manual typo, see below |
//! | Width | SH | `SH0;` → `SH0<P2>;` | `SH0<2 digits>;` | p.16, 00-21, see [`SH_BANDWIDTH_TABLE`] |
//!
//! All twelve wire shapes were re-verified directly against each command's
//! own per-command Set/Read/Answer box (manual printed p.4-5, p.10, p.13-16;
//! PDF pages 5-6, 11, 14-17 per the documented "+1" cover-page offset), not
//! just the p.3 master table, cross-checked with `pdftotext -layout` against
//! the rendered page images.
//!
//! **`RA`/`PA`/`NB`/`NR`/`BC`/`NA`, plain selector reads — same shape as
//! `CT` (batch 3)**: each of these six commands' own box gives a Read row
//! that carries the fixed `P1=0` selector byte (`R A P1 ;`, etc.) rather
//! than a zero-width read — structurally identical to `MD`/`CT`'s "selector
//! read" pattern, not the zero-width `Query` shape `RT`/`XT`/`BI` use
//! despite all being simple on/off (or, for `PA`, 3-valued) toggles. `PA`'s
//! `P2` is 3-valued (`0`=IPO, `1`=AMP1, `2`=AMP2) — modeled as
//! [`crate::radio_trait::PreampMode`] at the client/trait boundary rather
//! than a `bool`, since `ts570d::Radio::get_preamp`/`set_preamp`'s plain
//! bool cannot represent the FT-991A's two distinct gain stages.
//!
//! **`NL`/`RL`, the same selector-read shape with a wider `P2`**: `NL`
//! (manual p.13) is `N L P1 P2 P2 P2 ;` (3-digit level, 000-010); `RL`
//! (manual p.15) is `R L P1 P2 P2 ;` (2-digit level, 01-15) — both plain
//! numeric-range fields, no further structure.
//!
//! **`GT`, AGC's write/report domain mismatch — transcribed and resolved,
//! not silently smoothed over**: manual p.10's box gives Set as
//! `G T P1 P2 ;` (`P2` 5-valued: `0`=OFF, `1`=FAST, `2`=MID, `3`=SLOW,
//! `4`=AUTO) but Answer as `G T P1 P3 ;` (`P3` **7**-valued: the same four,
//! plus `4`=AUTO-FAST, `5`=AUTO-MID, `6`=AUTO-SLOW) — a genuine asymmetry in
//! the manual's own wire diagram (confirmed via both the rendered page image
//! and `pdftotext -layout`, not an extraction artifact), the same category
//! of "answer can express more than Set can" situation `TX`'s 3-valued
//! answer already established, but here the extra values sit *inside* the
//! same wire width rather than requiring a different one. `Ft991aState::
//! agc_mode` stores the full 7-valued (`P3`) domain; `handle_command`'s `Gt`
//! Set arm accepts `P2` `0`-`4` and stores it directly (`P2` `0`-`3` map
//! onto `P3` `0`-`3` verbatim; `P2=4` "AUTO" stores as `P3=4`, i.e.
//! `AUTO-FAST`) — **a documented judgment call, not a manual fact**: the
//! manual gives no way to determine which of `AUTO-FAST`/`AUTO-MID`/
//! `AUTO-SLOW` a plain "AUTO" `Set` should resolve to, and `AUTO-MID`/
//! `AUTO-SLOW` (`P3` `5`/`6`) are consequently unreachable via any `Set`
//! command in this emulator — only reportable if seeded directly via
//! [`Ft991aRadio::from_state`]. See [`crate::radio_trait::AgcMode`]'s own
//! doc comment for the client-facing consequence of this asymmetry.
//!
//! **`CO`, a 4-item selector embedded directly in the command (not the `EX`
//! menu system)**: manual p.5's box gives Set as
//! `C O P1 P2 P3 P3 P3 P3 ;` — `P1` fixed `0`, `P2` a 4-valued item
//! selector (`0`=CONTOUR ON/OFF, `1`=CONTOUR FREQ, `2`=APF ON/OFF, `3`=APF
//! FREQ), `P3` always 4 wire digits whose *meaning* depends on `P2`:
//! `P2=0`/`P2=2` use `P3` as a `0000`/`0001` boolean; `P2=1` uses `P3` as a
//! direct Hz value (`0010`-`3200`, CONTOUR frequency); `P2=3` uses `P3` as a
//! `0000`-`0050` raw index the manual states maps onto `-250`..`+250` Hz
//! ("APF Frequency: -250 - 250 Hz") — the endpoints and step count are
//! manual-stated but not an explicit formula, so the linear mapping
//! [`apf_raw_to_hz`]/[`apf_hz_to_raw`] use (`raw=0`→`-250`, `raw=25`→`0`,
//! `raw=50`→`+250`) is a documented judgment call, not manual-cited beyond
//! those three data points. Read is `C O P1 P2 ;` (2 chars: fixed `P1` +
//! item selector `P2`); Answer mirrors Set (6 chars). Kept `Ft991a`-
//! inherent-only, not on the `Radio` trait — CONTOUR/APF are FT-991A-named
//! parametric-EQ/audio-peaking features with no generic-radio-concept
//! precedent in `CLAUDE.md`'s "Radio trait scope" list or in `ts570d::Radio`
//! (checked before deciding, same practice prior batches used), and batch
//! 7 (`PL PR MG ML`, speech processor/mic/monitor) is the architect's own
//! separately-scoped "audio chain" batch — `CO` was assigned to *this*
//! batch instead, not folded into that theme, so it is treated on its own
//! terms here rather than assumed to belong with batch 7's concepts.
//!
//! **`BP`, a 2-item selector, same shape as `CO` with one fewer `P3`
//! digit**: manual p.5's box gives Set as `B P P1 P2 P3 P3 P3 ;` — `P2`
//! 2-valued (`0`=Manual NOTCH ON/OFF, `1`=Manual NOTCH LEVEL), `P3` always 3
//! wire digits: `P2=0` uses `P3` as a `000`/`001` boolean; `P2=1` uses `P3`
//! as a raw index (`001`-`320`) the manual states is "NOTCH Frequency: x 10
//! Hz" — i.e. `actual_hz = P3 * 10` (range 10-3200 Hz, a direct
//! manual-stated multiplier, not a judgment call like `CO`'s APF mapping).
//! Read is `B P P1 P2 ;` (2 chars); Answer mirrors Set (5 chars). Kept
//! `Ft991a`-inherent-only, not on the `Radio` trait, for the same "FT-991A-
//! specific 2-item selector, no generic-concept precedent" reasoning as
//! `CO` — a deliberate asymmetry against `BC` (Auto Notch), a plain boolean
//! kept on the trait (see below), documented as a judgment call rather than
//! silently applied.
//!
//! **`NA`, a genuine manual wire-diagram typo — resolved via corroborating
//! evidence, not followed literally**: manual p.13's own per-command box is
//! headed `NA` / `NARROW`, correctly placed in the master table's
//! alphabetical `N`-block (p.3, between `MW` and `NB`) — but the box's own
//! wire-diagram cells literally spell out `M A P1 P2 ;` / `M A P1 ;` (the
//! two-letter code `MA`, not `NA`) in all three of its Set/Read/Answer rows.
//! Confirmed present verbatim via both the rendered page image and
//! `pdftotext -layout` (not an extraction artifact) — this is the same
//! category of manual self-inconsistency as batch 1's `VM`/`AM` heading
//! clash and batch 2's `MW` P7 legend-vs-diagram mismatch, both resolved via
//! corroborating evidence rather than either escalated or silently
//! normalized. Three signals agree here: (1) the master table's own
//! alphabetized listing and heading unambiguously name this command `NA`;
//! (2) `MA` is already a distinct, differently-shaped, already-landed batch
//! 1 command (`MA`, "MEMORY CHANNEL TO VFO-A" — a zero-width `Action`
//! trigger, manual p.11) — treating this box's wire cells literally would
//! silently collide two unrelated commands onto the same two-letter code, an
//! untenable protocol design no CAT radio would actually ship; (3) `NA`'s
//! `P1`/`P2` legend (`0: Fixed` / `0: OFF 1: ON`) and overall shape are
//! byte-for-byte identical to `NB`'s immediately adjacent box (manual p.13,
//! same page), strongly suggesting the wire-diagram cells were copy-pasted
//! from a different, similarly-shaped template row without updating the
//! command-code letters. Resolved in favor of `NA` (the master table's own
//! code), implemented with the same selector-read shape as `NB`/`NR`/`BC`.
//!
//! **`SH`, the batch's highest-risk item — full six-column bandwidth table
//! transcribed, not approximated or sampled**: manual p.16's box gives Set
//! as `S H P1 P2 P2 ;` (`P1` fixed `0`, `P2` a 2-digit raw table index,
//! `00`-`21`, legend text just "00 (See Table)"). The bandwidth table
//! itself (immediately below the wire diagram, same page) has six value
//! columns — SSB (Narrow), SSB (Wide), CW (Narrow), CW (Wide), RTTY/PSK
//! (Narrow), RTTY/PSK (Wide) — transcribed row-for-row into
//! [`SH_BANDWIDTH_TABLE`], cross-checked via both the rendered page image
//! and `pdftotext -layout` (both agree exactly, no discrepancy found, unlike
//! batch 3's `CN` DCS-table transcription which did catch a one-digit
//! misread this way). **Critically, `SH`'s own wire format carries no mode
//! or narrow/wide parameter at all** — the table's column headers ("SSB",
//! "CW", "RTTY/PSK", "Narrow"/"Wide") are never referenced anywhere in
//! `SH`'s own Set/Read/Answer box text; which column a given `P2` index
//! actually means is determined entirely by the radio's *current* mode and
//! *current* narrow/wide state at the time `P2` is applied, neither of
//! which is part of this command's own wire bytes. This is the same
//! "meaning depends on state outside this command's own wire bytes"
//! category of open item `VD`'s menu-142 dependency (batch 5) and `RM`'s
//! `MS`-dependent selectors (batch 9) already established. Two further
//! judgment calls, both flagged rather than silently resolved:
//! - **Mode family mapping** ([`mode_family_for`]): nothing on `SH`'s own
//!   page states which of the FT-991A's 14 [`crate::radio_trait::Mode`]
//!   variants fall into "SSB"/"CW"/"RTTY/PSK" — this implementation maps
//!   only the modes literally named by the table's own column headers
//!   (`LSB`/`USB`→SSB, `CW`/`CW-R`→CW, `RTTY-LSB`/`RTTY-USB`→RTTY/PSK) and
//!   leaves `FM`/`AM`/`DATA-LSB`/`DATA-FM`/`FM-N`/`DATA-USB`/`AM-N`/`C4FM`
//!   unmapped (`None`) rather than guessing whether the `DATA-*` modes
//!   share the "SSB" or "RTTY/PSK" family.
//! - **Narrow/wide selection**: `NA`'s own on/off state (this same batch)
//!   is a well-evidenced, but not literally-stated, candidate for "which of
//!   each column-pair applies" — [`filter_bandwidth_hz`] takes `narrow: bool`
//!   as an explicit parameter for exactly this reason, but nothing on
//!   either `SH`'s or `NA`'s own page states the two commands are linked.
//!
//! `handle_command`'s `Sh` Set arm deliberately does **not** cross-validate
//! the written `P2` against the currently active mode/`NA` state (i.e. it
//! accepts any `P2` in `0..=21` unconditionally, even values that are `-`
//! /invalid for the current mode) — consistent with every other batch's
//! precedent of not inventing cross-command write-time validation beyond
//! what a command's own wire format states (`EX`'s per-item validation is
//! self-contained; `RM`'s `MS` dependency is read-time interpretation only,
//! never write-time rejection). [`SH_BANDWIDTH_TABLE`]/[`filter_bandwidth_hz`]
//! are exposed as pure, `Ft991a`-inherent-only lookup functions (not called
//! from `handle_command` at all) for a future caller (e.g. a `ui`/`emulator`
//! crate) that already knows the current mode and narrow/wide state to
//! resolve the actual Hz value itself.
//!
//! New state: `Ft991aState::{attenuator_on, preamp_mode, noise_blanker_on,
//! noise_blanker_level, noise_reduction_on, noise_reduction_level, agc_mode,
//! contour_on, contour_freq_hz, apf_on, apf_freq_hz, manual_notch_on,
//! manual_notch_freq_hz, auto_notch_on, narrow_on, filter_width_index}`.
//!
//! ## Wave 3 — CAT batch 7: speech processor/mic/monitor (`PL PR MG ML`)
//!
//! Per `planning/architect/task_plan.md` §10.5's batch 7 row — the
//! architect's own deliberately smallest batch (4 commands), described only
//! as a coherent "audio chain" theme with no further per-command notes,
//! unlike most other batches' rows. All four confirmed from their own
//! per-command boxes (manual printed p.11 `MG`, p.12 `ML`, p.14 `PL`/`PR` —
//! PDF pages 12, 13, 15 respectively, the established "+1" cover-page offset
//! holding again, re-verified via this task's own read of the master
//! table's page footer), not assumed from the 2-letter codes alone, per this
//! task's explicit instruction.
//!
//! **`MG` (MIC GAIN), the simplest of the four**: manual p.11. Set
//! `MG<3-digit P1>;` (`000`-`100`), Read `MG;` (zero-width — **not** a
//! selector read), Answer `MG<3-digit P1>;`. Same plain query/set shape as
//! `PC`/`PL` — no selector byte anywhere in the wire format.
//!
//! **`PL` (SPEECH PROCESSOR LEVEL)**: manual p.14. Set `PL<3-digit P1>;`
//! (`000`-`100`), Read `PL;` (zero-width), Answer `PL<3-digit P1>;`. Same
//! shape as `MG`.
//!
//! **`PR`, a genuine manual heading typo — resolved via the master table and
//! the command's own wire content, not followed literally**: `PR`'s own
//! per-command box (manual p.14, immediately below `PL`'s) is headed
//! "SPEECH PROCESSOR LEVEL" — byte-for-byte identical to `PL`'s heading,
//! confirmed present verbatim via the rendered page image (not a
//! `pdftotext` extraction artifact). Same category of manual
//! self-inconsistency as batch 1's `VM`/`AM` heading clash and batch 6's
//! `NA` wire-cell typo. The master table (manual p.3) independently names
//! this command just "SPEECH PROCESSOR" (no "LEVEL"), and `PR`'s own wire
//! content is unambiguous: Set `PR<P1><P2>;` (2 chars) — `P1` selects
//! **which** feature (`0`: Speech Processor, `1`: Parametric Microphone
//! Equalizer), `P2` is that feature's on/off state, with an unusual,
//! explicitly non-zero-based encoding (`1`: "OFF", `2`: "ON" — the only
//! on/off command in this crate's table so far that doesn't use `0`/`1`,
//! transcribed exactly, not normalized). Read is `PR<P1>;` (1 char,
//! selector read, same shape as `CT`/`RA`/`PA`); Answer mirrors Set.
//! Resolved as an on/off toggle command (not a level command) via the
//! master table's own heading plus the wire content itself — flagged here
//! and in `findings.md`, not silently picked.
//!
//! **`ML` (MONITOR LEVEL), the batch's only composite command**: manual
//! p.12. Set `ML<P1><P2 P2 P2>;` (4 chars) — `P1` selects which sub-value
//! `P2` represents (`0`: MONI "ON/OFF", `1`: MONI Level), `P2` is always 3
//! wire digits regardless of `P1`, but its *meaning* depends on `P1`:
//! `P1=0` → `P2` is `000` (OFF) or `001` (ON); `P1=1` → `P2` is `000`-`100`
//! (the level). Read is `ML<P1>;` (1 char, selector read); Answer mirrors
//! Set. Same two-width shape as `NL`/`RL`/`SH` (batch 6), but with a
//! genuine, non-fixed `P1` selector (unlike those three, whose `P1` is
//! always the fixed byte `"0"`) — closer in spirit to `CN`'s two-item `P1`
//! selector (batch 3), just with a fixed-width `P2` instead of `CN`'s
//! selector-dependent lookup-table index.
//!
//! New state: `Ft991aState::{mic_gain, speech_processor_level,
//! speech_processor_on, parametric_mic_eq_on, monitor_on, monitor_level}`.
//! No manual-stated factory default exists for any of the six — all six
//! follow this crate's established "arbitrary implementation default,
//! documented per-field" convention (`0`/`false` in every case, the same
//! minimum-legal-value convention prior batches used for undocumented level
//! defaults).
//!
//! `Radio` trait scope: checked `ts570d::Radio` first, per this crate's
//! established practice. `ts570d::Radio` already has direct precedent for
//! three of these four commands' underlying concepts —
//! `get_mic_gain`/`set_mic_gain` (u8) and
//! `get_speech_processor`/`set_speech_processor` (bool) — added as trait
//! methods here (`get_mic_gain`/`set_mic_gain` for `MG`,
//! `get_speech_processor_level`/`set_speech_processor_level` for `PL`,
//! `get_speech_processor_on`/`set_speech_processor_on` for `PR`'s `P1=0`
//! branch). `PR`'s `P1=1` branch (Parametric Microphone Equalizer on/off)
//! has no `ts570d::Radio` precedent and no `CLAUDE.md`-listed generic
//! concept to attach to — kept `Ft991a`-inherent-only
//! (`get_parametric_mic_eq_on`/`set_parametric_mic_eq_on`), the same
//! "FT-991A-named parametric feature, no generic precedent" treatment batch
//! 6 gave `CO`/`BP`. `ML`'s monitor on/off and level have no direct
//! `ts570d::Radio` precedent either (`ts570d`'s own `radio_trait.rs` has no
//! "monitor" concept at all) — added to the trait anyway as a **documented
//! judgment call, not silently decided**: an audio monitor (hearing one's
//! own transmitted signal) is a standard, near-universal transceiver
//! concept (often called "sidetone monitor" on other radios), not an
//! FT-991A-named feature the way CONTOUR/APF/parametric-EQ are, and
//! `CLAUDE.md`'s "Radio trait scope" section's "gain controls"/"etc."
//! language is broad enough to plausibly cover it — flagged here for
//! architect review rather than assumed correct, since (unlike
//! `MG`/`PL`/`PR`'s speech-processor branch) there is no `ts570d::Radio`
//! method to point to as direct precedent.
//!
//! ## Wave 3 — CAT batch 8: band/step/encoder front-panel controls (`BS BU
//! BD FS ED EU EK DN UP`)
//!
//! Per `planning/architect/task_plan.md` §10.5's batch 8 row. All nine
//! confirmed from their own per-command boxes: `BS`/`BU`/`BD` manual p.4-5
//! (PDF pages 5-6), `FS` p.9 (PDF p.10), `ED`/`EK`/`EU` p.7 (PDF p.8), `DN`
//! p.6 (PDF p.7), `UP` p.17 (PDF p.18) — the established "PDF page = printed
//! footer + 1" offset re-confirmed once more (page 1 is an unprinted cover).
//!
//! **`BS`'s full 16-band table, transcribed exactly, including the
//! documented gap at index `13`** (manual p.5):
//!
//! | P1 | Band | P1 | Band | P1 | Band |
//! |----|------|----|------|----|------|
//! | `00` | 1.8 MHz | `06` | 18 MHz | `12` | MW |
//! | `01` | 3.5 MHz | `07` | 21 MHz | `13` | *(no entry — documented gap)* |
//! | `02` | 5 MHz | `08` | 24.5 MHz | `14` | AIR |
//! | `03` | 7 MHz | `09` | 28 MHz | `15` | 144 MHz |
//! | `04` | 10 MHz | `10` | 50 MHz | `16` | 430 MHz |
//! | `05` | 14 MHz | `11` | GEN | | |
//!
//! Sixteen real bands, wire values `00`-`16`, with `13` genuinely absent
//! from the manual's own table (not a transcription gap introduced here —
//! present verbatim on the page image). `BS` is Set-only (manual p.3: `Set
//! O Read X Ans X`) — no `Read`/`Answer` wire form exists at all, so there
//! is no CAT-reachable way to query the current band back; this
//! implementation's `selected_band` state is inspectable only via
//! `Ft991aRadio::state()`/`from_state` in tests, same treatment already
//! established for other write-only fields (e.g. `Mw`). [`Band`] (in
//! `radio_trait.rs`) is the validated 16-value domain type;
//! [`Band::try_from`] rejects `13` (and any value `> 16`) exactly like
//! `RI`'s selector gap did in batch 9 — a hard rejection, never a silent
//! accept.
//!
//! **`BU`/`BD` (BAND UP/DOWN)**: both Set-only, 1-digit `P1` that is
//! documented `"0: Fixed"` (a required literal, not real data — manual
//! p.4-5). Modeled as stepping [`Ft991aState::selected_band`] to the
//! next/previous entry in the 16-band table (skipping the `13` gap),
//! wrapping past either end — the manual states no boundary behavior,
//! same documented judgment-call category as batch 1's `CH` wrap-around.
//! See [`next_band`]/[`prev_band`].
//!
//! **`FS` (FAST STEP)**: manual p.9. `F S P1 ;` Set (`0`/`1`), `F S ;`
//! Read, `F S P1 ;` Answer — a plain bidirectional bool, same shape as
//! `RT`/`XT`/`CS`/`BI`. `P1` toggles "VFO-A FAST Key" on/off. Direct
//! `ts570d::Radio::get_fine_step`/`set_fine_step` precedent (confirmed by
//! reading `ts570d/radio/src/radio_trait.rs` directly before adding).
//!
//! **`ED`/`EU` (ENCODER DOWN/UP)**: manual p.7. Set `E D P1 P2 P2 ;` / `E U
//! P1 P2 P2 ;` (3 total wire digits: 1-digit `P1` + 2-digit `P2`), no
//! `Read`/`Answer` (Set-only, manual p.3). `P1` selects which physical
//! encoder (`0`=MAIN, `1`=SUB, `8`=MULTI — see [`EncoderSelector`]); `P2` is
//! `01`-`99` "Frequency Steps," with the legend's own parenthetical caveat
//! `"01: (Fixed) Step (Except when encoder function is set to
//! 'frequency')"` — i.e. the actual Hz-per-step mapping depends on what
//! function the selected encoder is currently assigned to (front-panel/menu
//! state this emulator doesn't model at all). **Judgment call**: structural
//! and range validation is enforced (`P1` in `{0,1,8}`, `P2` in `1..=99`),
//! but neither command mutates any persisted `Ft991aState` field — same "no
//! simulate-able effect" treatment `KY`/`ZI`/`EK` (batch 4/below) already
//! established, and unlike `DN`/`UP` below, `ts570d::Radio` has no
//! "encoder" concept at all to borrow a concrete effect from. Kept
//! `Ft991a`-inherent-only (`encoder_down`/`encoder_up`), per this task's own
//! framing that encoder controls are FT-991A-front-panel-specific.
//!
//! **`EK` (ENT KEY)**: manual p.7. Zero-width Action trigger (`E K ;`, no
//! parameter, no `Read`/`Answer`). Mutates no `Ft991aState` field — same
//! category as `ZI`/`RC` (nothing meaningful for this software-only
//! emulator to represent for a front-panel "confirm" key-press). Kept
//! `Ft991a`-inherent-only (`ent_key`).
//!
//! **`DN`/`UP`, the batch's flagged manual inconsistency — resolved, not
//! silently picked**: the master table (p.3) names these plainly `"DOWN"`/
//! `"UP"`. `UP`'s own per-command box (p.17) agrees exactly (`"UP"`). `DN`'s
//! own per-command box (p.6), by contrast, is headed **"MIC DWN"** — the
//! only occurrence of the word "MIC" anywhere in the manual's 20 pages
//! combined with UP/DOWN semantics (confirmed via a full-text search). Both
//! wire formats are structurally identical zero-width Action triggers (`D N
//! ;` / `U P ;`, manual p.3: `Set O Read X Ans X` for both) — the wire
//! format alone cannot disambiguate, same starting position as batch 1's
//! `VM`/`AM`. Resolved via cross-radio corroboration rather than guessing
//! from the heading text alone, per this task's own instruction: `ts570d`'s
//! Kenwood TS-570D CAT protocol has **wire-identical** `UP`/`DN` commands
//! (`ts570d/radio/src/ts570d_radio.rs`: `definition!(Up, "UP", "Frequency
//! Up", ...)`, `definition!(Dn, "DN", "Frequency Down", ...)`), implemented
//! by `ts570d::Radio::mic_up`/`mic_down` under a `radio_trait.rs` section
//! literally titled `"MIC up/down (write-only momentary)"`, backed by
//! `ts570d_radio_handlers.rs`'s own comments (`"UP — VFO frequency up by 100
//! Hz (Menu 02 default step, write-only)"`) — a **different manufacturer's**
//! CAT protocol independently landing on the exact same two 2-letter codes
//! for the exact same "hand mic UP/DWN button" concept is strong,
//! independent corroboration (not merely a naming coincidence given the
//! byte-identical codes) that FT-991A's `DN`/`UP` are the same physical
//! control, and that `DN`'s own box heading ("MIC DWN") is the accurate,
//! specific description while the master table's plain "DOWN"/"UP" are
//! generic labels (`UP`'s box just happens to already match the generic
//! label, needing no clarifying prefix). Modeled after `ts570d`'s own
//! implementation exactly: each press steps `Ft991aState::vfo_a_hz` by a
//! fixed [`MIC_STEP_HZ`] (documented arbitrary choice — the FT-991A manual
//! states no Hz-per-press value for `DN`/`UP` any more than the Kenwood
//! manual does for its `UP`/`DN`; this implementation does **not** tie the
//! step size to `FS`'s `fast_step_on` state, mirroring `ts570d`'s own design
//! where `FS`/`UP`/`DN` are unlinked despite being adjacent commands),
//! saturating at `FA`'s documented `30_000..=470_000_000` Hz range. Direct
//! `ts570d::Radio::mic_up`/`mic_down` precedent — added to the `Radio` trait
//! under the same names.
//!
//! New state: `Ft991aState::{selected_band, fast_step_on}`. Neither has a
//! manual-stated factory default; `0` (`00`, 1.8 MHz) and `false` are this
//! implementation's arbitrary choices, same category as prior batches'
//! undocumented defaults.
//!
//! `Radio` trait scope: checked `ts570d::Radio` first, per this crate's
//! established practice. Direct precedent found for `FS`
//! (`get_fine_step`/`set_fine_step`) and `DN`/`UP` (`mic_up`/`mic_down`,
//! same method names) — both added to the trait unchanged. `BS`/`BU`/`BD`
//! (`set_band`/`band_up`/`band_down`) have **no** `ts570d::Radio`
//! precedent (`ts570d` has no band concept anywhere), but per this task's
//! own framing ("band select/up/down are fairly generic") and this crate's
//! established "near-universal transceiver concept, added anyway, flagged
//! for review" treatment (same category as batch 5's `ScanState` and batch
//! 7's `ML`), all three join the trait too — `set_band` has no paired
//! getter (see `BS`'s own section above for why). `ED`/`EU`/`EK` stay
//! `Ft991a`-inherent-only — FT-991A-specific front-panel concepts with no
//! `ts570d::Radio` precedent and no clean generic abstraction to attach to.
//!
//! ## Wave 3 — CAT batch 10 (last of the 10 core batches): misc
//! system/TX/tuner/DVS (`AC AI DA DT LK OI OS FT TS MX LM PB`)
//!
//! Per `planning/architect/task_plan.md` §10.5's batch 10 row — the last of
//! the 10 core CAT batches; after this task lands, all 91 top-level (non-
//! `EX`) CAT commands are implemented (the only remaining scope is the `EX`
//! menu's ~144 unimplemented items and the RTS/DTR consumption wiring, per
//! §10.8). All twelve confirmed from their own per-command boxes: `AC`/`AI`
//! manual p.4 (PDF p.5), `DA`/`DT` p.6 (PDF p.7), `FT`/`FS`(cross-checked
//! only) p.9 (PDF p.10), `LK`/`LM` p.11 (PDF p.12), `OI`/`OS`/`MX` p.13 (PDF
//! p.14), `PB` p.14 (PDF p.15), `TS`/`UL`(cross-checked only) p.17 (PDF
//! p.18) — via the rendered page image directly, cross-checked against
//! `pdftotext -layout` for each box, the established "PDF page = printed
//! footer + 1" offset re-confirmed once more.
//!
//! **`TS`, a genuine mismatch against the architect's dispatch-prompt
//! guess — resolved from the manual, not the guess**: the architect's task
//! description speculated `TS` might be "tuning step." Its own per-command
//! box (manual p.17) is unambiguously headed **"TXW"**, a plain
//! zero-width-query/1-digit-set boolean (`0`/`1`) with no further
//! elaboration anywhere in the manual of what "TXW" stands for. Implemented
//! per the manual's actual wire box (which is all that's needed to
//! implement it correctly) rather than the guessed name; **not** modeled as
//! a tuning-step concept, since nothing on `TS`'s own page supports that
//! reading. Kept `Ft991a`-inherent-only (`get_txw_on`/`set_txw_on`) — no
//! `ts570d::Radio` precedent exists for a same-named concept, and this
//! crate cannot confirm what "TXW" controls with confidence, so it isn't
//! generalized onto the `Radio` trait.
//!
//! **`AC` (ANTENNA TUNER CONTROL)**: manual p.4. Set `AC<P1><P2><P3>;` (3
//! digits: `P1`="0"/`P2`="0" both fixed literals, `P3` real data — `0`:
//! Tuner "OFF", `1`: Tuner "ON", `2`: "Tuning Start / Tuning Stop"). Read
//! `AC;` (**zero-width**, not a selector read — confirmed via the image,
//! unlike most of this crate's other fixed-`P1`-selector commands). Answer
//! mirrors Set, same `P3` label as Set (no write/report domain split, unlike
//! `FT` below). `P3=2`'s literal "Tuning Start / Tuning Stop" phrasing
//! suggests a momentary toggle-like trigger, but the manual gives Answer the
//! identical 3-value domain as Set with no separate transient-vs-persisted
//! distinction — this implementation stores whatever `P3` was last written
//! verbatim and echoes it back unchanged (no special-casing), the most
//! literal reading available. Kept `Ft991a`-inherent-only
//! (`get_antenna_tuner_state`/`set_antenna_tuner_state`) — both this crate's
//! own `CLAUDE.md` and `ts570d`'s (`"antenna tuner"` explicitly named in
//! both repos' "TS-570D/FT-991A-specific features... inherent methods, NOT
//! in the `Radio` trait" language) rule this out as a trait concept, and
//! `ts570d::Radio` itself keeps its own antenna-tuner methods
//! (`set_antenna_tuner_thru`/`start_antenna_tuning`) off its trait too —
//! independent confirmation, not just a same-repo-CLAUDE.md coincidence.
//!
//! **`AI` (AUTO INFORMATION)**: manual p.4. Set `AI<P1>;` (`0`/`1`), Read
//! `AI;`, Answer `AI<P1>;` — plain bidirectional bool, same shape as
//! `RT`/`XT`/`CS`/`FS`. The manual's own note ("This parameter is set to
//! '0' (OFF) automatically when the transceiver is turned 'OFF'") describes
//! a cross-command side effect (`PS` powering off should reset `AI`) that
//! this implementation does **not** enforce — consistent with this crate's
//! established practice of not inventing cross-command interactions beyond
//! what a command's own wire format states (e.g. `SH`'s write arm
//! deliberately not cross-validating against `NA`/mode state, batch 6);
//! flagged here as a documented simplification, not silently dropped.
//! Direct `ts570d::Radio::set_auto_info(mode: u8)` precedent (checked
//! before adding) — added to the `Radio` trait as
//! `get_auto_info_on`/`set_auto_info_on` (a plain bool, since the FT-991A's
//! own `AI` is a 2-valued on/off, unlike `ts570d`'s 4-valued `0`-`3` "auto
//! information mode" — a deliberate, documented divergence from mirroring
//! `ts570d`'s exact signature).
//!
//! **`DA` (DIMMER)**: manual p.6. Set `DA<P1 P1><P2 P2><P3 P3>;` (6 digits:
//! `P1`="00" fixed, `P2` "01"-"02" LED Indicators Brightness Level, `P3`
//! "00"-"15" TFT Display Brightness Level — all three counts re-verified
//! directly against the page image's column diagram, not just the extracted
//! legend text, since `P2`'s narrow 2-value range looked suspicious at
//! first glance; the image confirms it exactly). Read `DA;` (zero-width).
//! Answer mirrors Set. FT-991A-specific display-hardware feature, no
//! generic-radio-concept precedent anywhere — kept `Ft991a`-inherent-only
//! (`get_dimmer`/`set_dimmer`).
//!
//! **`DT` (DATE AND TIME), this batch's rehearsal of the `EX`-style
//! "selector determines payload shape" pattern, per the architect's own
//! framing**: manual p.6. `P1` (`0`=Date, `1`=Time(UTC), `2`=Time
//! differential/Time Zone) selects `P2`'s shape: `P1=0` → `P2` is 8 digits
//! `yyyymmdd`; `P1=1` → `P2` is 6 digits `hhmmss` (24-hour); `P1=2` → `P2`
//! is 1-char sign + 4 digits `hhmm`, range `-12:00`..`+14:00` in 30-minute
//! increments (5 total). Read is `DT<P1>;` (1 char, selector read, same
//! treatment as `MD`/`EX`/`CT`). Total wire widths: `1` (read), `1+8=9`
//! (date write), `1+6=7` (time write), `1+5=6` (offset write) — four
//! distinct `DT_SET_FORMS` entries, all structurally distinguishable by
//! length alone; `handle_command`'s `Dt` arm additionally checks `P1`
//! against the width actually used (e.g. a 6-char frame with `P1="0"` is
//! structurally a legal `DT_SET_FORMS` width but semantically wrong for a
//! date, and is rejected, not silently reinterpreted) — the same
//! "structural match succeeded, semantic validation still per-item" pattern
//! `EX`/`FA` already established. Month/day/hour/minute/second are
//! range-checked (`1..=12`/`1..=31`/`0..=23`/`0..=59`/`0..=59`, no
//! leap-year/month-length calendar validation — the manual states no such
//! rule and no other command in this crate enforces cross-field calendar
//! logic either); the time-zone offset is checked against `-720..=840`
//! minutes in 30-minute steps. New state: `Ft991aState::{date_year,
//! date_month, date_day, time_hour, time_minute, time_second,
//! time_zone_offset_min}` — none has a manual-stated factory default;
//! `0000-01-01`/`00:00:00`/`+0000` are this implementation's arbitrary
//! choices, same category of open item as prior batches' undocumented
//! defaults. Kept `Ft991a`-inherent-only (`read_date`/`write_date`/
//! `read_time`/`write_time`/`read_time_zone_offset`/
//! `write_time_zone_offset`) — a system/utility clock setting, not named in
//! `CLAUDE.md`'s "Radio trait scope" list and with no `ts570d::Radio`
//! precedent (an older HF-only rig with no such command), closer in kind to
//! `EX`'s menu-access system settings (also kept off the trait) than to a
//! core operating concept like frequency/mode/PTT.
//!
//! **`LK` (LOCK)**: manual p.11. Set `LK<P1>;` (`0`/`1`, "VFO-A DIAL Lock"),
//! Read `LK;`, Answer `LK<P1>;` — plain bidirectional bool. Direct
//! `ts570d::Radio::get_frequency_lock`/`set_frequency_lock` precedent
//! (checked before adding) — added to the trait under those same names.
//!
//! **`OI` (OPPOSITE BAND INFORMATION), confirmed to share `IF`'s
//! [`ChannelStatusFields`] shape exactly, column-by-column against the
//! manual image — not assumed from the cross-batch finding alone**: manual
//! p.13. Read `OI;` (**zero-width query**, not a selector read — `OI` is
//! read-only, manual p.3: `Set X Read O Ans O`). Answer's `P1`-`P10`
//! sequence (`001-117` memory channel, `P2` VFO-B frequency, `P3` clarifier
//! sign+offset, `P4`/`P5` RX/TX clarifier on/off, `P6` mode, `P7`
//! VFO/Memory select, `P8` CTCSS/DCS status, `P9` fixed, `P10` offset type)
//! is byte-for-byte identical in field order, width, and legend text to
//! `IF`'s own P1-P10 sequence (batch 9) — confirmed by cross-checking `OI`'s
//! column diagram against `IF`'s own (both share the identical apparent
//! "`P2` gets only 4 visible digit-cells in the first 10-column sub-row"
//! rendering quirk) and, independently, against `FA`'s unambiguous 8+1
//! two-row split for its own 9-digit frequency field (which resolves the
//! same rendering quirk unambiguously: the 5th `P2` digit cell is present
//! but tightly spaced in the low-resolution page render, not actually
//! missing) — both signals agree `OI`'s `P2` is the same full 9-digit
//! frequency field `IF`'s is, not a truncated 8-digit one. Reuses
//! [`ChannelStatusFields::to_wire_string`] directly, **no changes** to that
//! struct. The **sole** documented difference from `IF`'s own payload is
//! which frequency it reports (`OI`'s `P2` is `vfo_b_hz`, `IF`'s is
//! `vfo_a_hz`) — confirmed directly from each command's own legend text
//! ("VFO-B Frequency (Hz)" vs. "VFO-A Frequency (Hz)"). **Judgment call,
//! inherited from Wave 1's single-shared-non-VFO-specific-fields state
//! model, not a new discovery**: this emulator has no separate per-VFO
//! clarifier/mode/select/tone-status/offset-type state (the same
//! constraint batch 1's `AB`/`BA`/`SV` already documented — `Ft991aState`
//! has one `mode` field, not one per VFO), so `OI`'s `P1`/`P3`-`P10` fields
//! all read from the exact same shared state `IF` already uses
//! (`if_channel`, `clarifier_offset_hz`, `rx_clarifier_on`,
//! `tx_clarifier_on`, `mode`, `channel_select`, `tone_status`,
//! `offset_type`) — only the frequency genuinely differs. Flagged for
//! architect/hardware review, not silently assumed correct. Kept
//! `Ft991a`-inherent-only (`get_opposite_band_information`) — `IF`'s own
//! identical composite payload was kept off the trait too (batch 9, "a
//! composite status dump, not a generic concept"), and `OI` inherits that
//! same treatment for the same reason, not a fresh decision.
//!
//! **`OS` (OFFSET / REPEATER SHIFT)**: manual p.13. Set `OS<P1><P2>;` (`P1`
//! fixed "0", `P2` `0`=Simplex/`1`=Plus Shift/`2`=Minus Shift — "*This
//! command can be activated only with an FM mode," a front-panel-context
//! caveat this emulator does not enforce, consistent with `AI`'s similar
//! unenforced cross-command note above), Read `OS<P1>;` (1 char, selector
//! read, same shape as `CT`/`RA`), Answer mirrors Set. **Reuses
//! [`Ft991aState::offset_type`] directly** — that field was declared back
//! in batch 9 for `IF`'s `P10` with a doc comment explicitly deferring its
//! `Set` side to "batch 10" (this task); no new state field needed. Added to
//! the `Radio` trait (`get_repeater_shift`/`set_repeater_shift`, a new
//! [`crate::radio_trait::RepeaterShift`] enum) as a **documented judgment
//! call, no direct `ts570d::Radio` precedent** (that trait has no FM-repeater
//! concept at all — an HF-only rig) — added anyway since repeater-shift
//! direction is a standard, near-universal concept on VHF/UHF-capable
//! transceivers like the FT-991A, the same "near-universal, no direct
//! precedent, flagged for review" treatment `ML`/`Band`/`ScanState` already
//! received in prior batches.
//!
//! **`FT` (FUNCTION TX), a genuine write/report domain mismatch — confirmed
//! via the image, not a re-derivation of `GT`'s batch-6 finding but the
//! same category**: manual p.9. Set `FT<P1>;` where `P1` is `2`="VFO-A Band
//! Transmitter: TX" or `3`="VFO-B Band Transmitter: TX"; Read `FT;`
//! (zero-width); Answer `FT<P2>;` where `P2` is `0`="VFO-A..." or
//! `1`="VFO-B..." — the **same two semantic states**, but Set's wire values
//! (`2`/`3`) and Answer's wire values (`0`/`1`) are literally different
//! digits for identical meanings (unlike `GT`'s widening mismatch, this is a
//! direct 1:1 remap: `2`→`0`, `3`→`1`). `Ft991aState::tx_vfo_select` stores
//! the Answer-domain value (`0`/`1`) directly; the `Set` arm translates
//! `"2"`→`0`/`"3"`→`1` before storing, any other `P1` value (including `0`
//! or `1`, which are only legal on the Answer side) is rejected. Direct
//! `ts570d::Radio::get_tx_vfo`/`set_tx_vfo` precedent (checked before
//! adding: `ts570d`'s own signature is `0`=VFO A, `1`=VFO B, `2`=Memory) —
//! added under the same trait method names, using the FT-991A's own 2-value
//! domain (no memory-channel TX-select option exists for `FT`, unlike
//! `ts570d`'s 3-valued version — a documented, deliberate narrowing, not an
//! oversight).
//!
//! **`TS`**: see above (the architect's-guess-vs-manual mismatch section).
//!
//! **`MX` (MOX SET)**: manual p.13. Set `MX<P1>;` (`0`/`1`), Read `MX;`,
//! Answer `MX<P1>;` — plain bidirectional bool, same shape as `AI`/`LK`.
//! "MOX" (manually keying the transmitter, independent of CAT `TX`/`RX`) has
//! no `ts570d::Radio` precedent, but is structurally and conceptually
//! adjacent to the already-trait-level PTT concept (`transmit`/`receive`/
//! `get_tx_state`) — added to the trait (`get_mox_on`/`set_mox_on`) as a
//! **documented judgment call**, same "near-universal transceiver concept,
//! no direct precedent, flagged for review" treatment as `AI`/`OS`/`MX`'s
//! siblings in this batch.
//!
//! **`LM` (LOAD MESSAGE / DVS RECORD) and `PB` (PLAY BACK / DVS PLAYBACK),
//! the FT-991A's Digital Voice Storage feature — direct structural analog
//! to batch 4's `KM`/`KY` keyer-memory store/playback pair, kept
//! `Ft991a`-inherent-only for the same reason**: manual p.11 (`LM`) and p.14
//! (`PB`). Both share the shape `<CMD><P1><P2>;` (`P1` fixed "0" = "DVS",
//! `P2` one digit `0`-`5`), Read `<CMD><P1>;` (1 char, selector read),
//! Answer mirrors Set — same two-width shape as `CT`/`OS`. **The two
//! commands' `P2=1..=5` semantics are genuinely different, transcribed
//! exactly, not assumed symmetric**: `LM`'s legend phrases every non-zero
//! value as "CH 'N' Recording **Start/Stop**" (a per-channel toggle — the
//! same channel value stops an in-progress recording of that channel, any
//! other non-zero value starts a new one), while `PB`'s legend phrases its
//! non-zero values as "CH 'N' Playback **Start**" only (no toggle wording —
//! sending `N` always (re)starts channel `N`'s playback, unconditionally).
//! `P2=0` always means "Stop" for both. Modeled as
//! `Ft991aState::dvs_recording_channel`/`dvs_playback_channel` (`0`
//! = stopped, `1`-`5` = the active channel, raw wire-shaped values — same
//! "store the wire domain directly" convention `scan_state`/`agc_mode`
//! already use), with `Lm`'s write arm implementing the toggle
//! (`P2==current` → stop, else → start `P2`) and `Pb`'s write arm always
//! overwriting unconditionally. No audio is actually simulated (same "no
//! simulate-able effect beyond structural/semantic validation" category as
//! `KY`/`ZI`/`ED`/`EU`) — only the active-channel-or-stopped state is
//! tracked. Kept `Ft991a`-inherent-only (`start_dvs_recording`/
//! `stop_dvs_recording`/`get_dvs_recording_channel`,
//! `start_dvs_playback`/`stop_dvs_playback`/`get_dvs_playback_channel`) —
//! directly mirroring `KM`/`KY`'s own "FT-991A-specific stored-message
//! system, no generic concept" exclusion from batch 4, applied here to
//! stored *audio* instead of stored *CW text*.
//!
//! New state, full list: `Ft991aState::{antenna_tuner_state, auto_info_on,
//! led_brightness, tft_brightness, date_year, date_month, date_day,
//! time_hour, time_minute, time_second, time_zone_offset_min, lock_on,
//! tx_vfo_select, txw_on, mox_on, dvs_recording_channel,
//! dvs_playback_channel}` (`offset_type` is reused, not new — see `OS`
//! above).
//!
//! `Radio` trait scope summary: `AI` (`get_auto_info_on`/`set_auto_info_on`,
//! direct `ts570d::Radio` precedent), `LK`
//! (`get_frequency_lock`/`set_frequency_lock`, direct precedent), `OS`
//! (`get_repeater_shift`/`set_repeater_shift`, judgment call), `FT`
//! (`get_tx_vfo`/`set_tx_vfo`, direct precedent, narrowed domain), `MX`
//! (`get_mox_on`/`set_mox_on`, judgment call) were added to the trait. `AC`
//! (antenna tuner — explicitly excluded by both this repo's and `ts570d`'s
//! own `CLAUDE.md`), `DA` (dimmer — FT-991A display hardware, no generic
//! concept), `DT` (date/time — system/utility setting, no `CLAUDE.md`
//! listing or `ts570d::Radio` precedent), `OI` (composite status dump,
//! following `IF`'s own established off-trait precedent), `TS` (meaning
//! genuinely uncertain even after reading the manual directly), and
//! `LM`/`PB` (FT-991A-specific DVS system, mirroring `KM`/`KY`'s exclusion)
//! were kept `Ft991a`-inherent-only — six judgment calls, all flagged above
//! individually rather than silently decided.

use std::convert::Infallible;

use cat_framework::{
    CatCommandCatalog, CatRadio, CommandDefinition, CommandForm, CommandOperation, CommandOutcome,
    CommandRequest, CommandTable, ProtocolErrorKind, ResponseBuilder, ResponseDisposition,
};

/// FT-991A command identifier owned by the radio crate.
///
/// Exactly the 11 first-slice commands (see module docs). Deliberately NOT
/// padded with placeholder variants for commands outside this wave's scope
/// (`IF`, `RM`/`RI`, memory channels, the `EX` menu, etc.) — the enum grows
/// alongside command coverage in later waves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ft991aCommandId {
    Fa,
    Fb,
    Md,
    Tx,
    Sm,
    Ps,
    Ag,
    Rg,
    Sq,
    Pc,
    Id,
    // Batch 9: meters/status.
    If,
    Rm,
    Ri,
    Rs,
    Ms,
    Ul,
    // `EX` menu, first sub-batch: shared plumbing + the 9 PTT/keying items.
    Ex,
    // Batch 2: memory channel records.
    Mc,
    Mr,
    Mw,
    Mt,
    // Batch 1: VFO/split/memory quick-ops.
    Ab,
    Ba,
    Am,
    Vm,
    Ma,
    Ch,
    Qi,
    Qr,
    Qs,
    Sv,
    // Batch 3: clarifier/RIT-XIT + tone + IF-shift.
    Rt,
    Rc,
    Rd,
    Ru,
    Xt,
    Cn,
    Ct,
    Is,
    // Batch 4: keyer/CW/break-in.
    Km,
    Kp,
    Kr,
    Ks,
    Ky,
    Cs,
    Zi,
    Bi,
    Sd,
    // Batch 5: scan/VOX/busy.
    Sc,
    Vx,
    Vd,
    Vg,
    By,
    // Batch 6: attenuator/preamp/noise/AGC/notch/filter-width.
    Ra,
    Pa,
    Nb,
    Nl,
    Nr,
    Rl,
    Gt,
    Co,
    Bp,
    Bc,
    Na,
    Sh,
    // Batch 7: speech processor/mic/monitor.
    Mg,
    Pl,
    Pr,
    Ml,
    // Batch 8: band/step/encoder front-panel controls.
    Bs,
    Bu,
    Bd,
    Fs,
    Ed,
    Eu,
    Ek,
    Dn,
    Up,
    // Batch 10 (last of the 10 core batches): misc system/TX/tuner/DVS.
    Ac,
    Ai,
    Da,
    Dt,
    Lk,
    Oi,
    Os,
    Ft,
    Ts,
    Mx,
    Lm,
    Pb,
}

const QUERY0: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Query, 0)];
const SET_1: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 1)];
/// `KP`'s single Set width (2-digit raw pitch value, manual p.10).
const SET_2: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 2)];
const SET_3: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 3)];
const SET_4: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 4)];
const SET_9: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 9)];
/// `MW`'s single Set width: [`ChannelStatusFields::WIRE_WIDTH`] (25 bytes,
/// P1-P10 — manual p.12, see [`MemoryChannelRecord`]'s doc comment).
const SET_25: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 25)];
/// `MT`'s two Set widths: the 3-byte selector-only read (`MT<P0>;`, same
/// "selector read" treatment as `Md`/`Sm`/`Rm`/`Ri`/`Ex` above) and the
/// 38-byte write body ([`ChannelStatusFields::WIRE_WIDTH`] + 1 reserved
/// byte + the 12-character tag — manual p.12).
const MT_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 3),
    CommandForm::fixed(CommandOperation::Set, 38),
];
const NONE: &[CommandForm] = &[];
/// Zero-width, parameterless trigger form (batch 1: `AB BA AM VM MA QI QR
/// QS SV`; batch 3: `RC`) — mirrors `ts570d`'s own `ACTION` const for its
/// analogous `TX`/`RX`/`RC`/`RU`/`RD`/`UP`/`DN` triggers.
const ACTION: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Action, 0)];

/// `MD`'s `set_forms`: the selector-only read width (1 char: `"0"`) plus the
/// real write width (2 chars: `"0"` + mode hex digit).
///
/// This is the one genuine "selector read" in the first slice (along with
/// `SM`): the manual's Read row for `MD` is `MD0;`, not the zero-width `MD;`
/// `cat-framework`'s parser would otherwise require to classify a frame as
/// `Query`. Any non-empty parameter is matched against `set_forms` instead
/// (see `cat_framework::CommandTable::parse`), so both widths must live
/// here, and `handle_command` disambiguates read-vs-write by
/// `request.parameters.raw().len()` rather than `request.operation` alone.
const MD_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 2),
];

/// `EX`'s `set_forms`: the selector-only read width (3 chars: the P1 menu
/// number alone) plus every distinct total wire width (`3 + P2 digits`)
/// used anywhere in the full 153-row menu table (manual p.7-9).
///
/// Same "selector read" shape as [`MD_SET_FORMS`], generalized: `EX<P1>;`
/// is structurally a 3-byte `Set` to `cat-framework`'s parser (not a
/// zero-width `Query`), and `EX<P1><P2>;` is a write whose total width
/// depends on which menu item `P1` names. Re-transcribing the "Digits"
/// column for **all 153 rows** (not just this sub-batch's 9) found exactly
/// six distinct P2 widths — **1, 2, 3, 4, 5, 8** — confirming the
/// architect's "~6" estimate (`planning/architect/task_plan.md` §10.6)
/// exactly, with one excluded outlier: item 087 "RADIO ID" shows P2 as
/// literal dashes (`----------`) with no digit count given anywhere in the
/// manual — explicitly flagged there as unresolvable, and correctly absent
/// from both this width list and [`EX_MENU_TABLE`] (no item 087 row; not
/// this task's scope regardless, since only 9 specific items are
/// implemented here — see module docs).
///
/// This yields total wire widths `3 + {1,2,3,4,5,8} = {4,5,6,7,8,11}`. All
/// six write widths are included **now**, even though this sub-batch's
/// [`EX_MENU_TABLE`] only populates digit-width-1 (total width 4) rows —
/// per the architect's "later sub-batches then just add table rows"
/// design: a later sub-batch adding e.g. item 001 "AGC FAST DELAY"
/// (4-digit P2, total width 7) needs no change here, only a new
/// `EX_MENU_TABLE` row. A wire frame at any of these widths whose `P1`
/// isn't (yet) in `EX_MENU_TABLE` — or whose actual parameter width
/// disagrees with that item's own registered `digits` — fails the
/// per-item semantic check in `handle_command`'s `Ex` arm and cleanly
/// returns `"?;"`, the same "structural match succeeded, semantic
/// validation still per-item" pattern `FA`'s range check already
/// demonstrates.
const EX_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 3), // read: EX<P1>;
    CommandForm::fixed(CommandOperation::Set, 4), // write, P2 digits=1
    CommandForm::fixed(CommandOperation::Set, 5), // write, P2 digits=2
    CommandForm::fixed(CommandOperation::Set, 6), // write, P2 digits=3
    CommandForm::fixed(CommandOperation::Set, 7), // write, P2 digits=4
    CommandForm::fixed(CommandOperation::Set, 8), // write, P2 digits=5
    CommandForm::fixed(CommandOperation::Set, 11), // write, P2 digits=8 (item 151, the sole 8-digit outlier)
];

/// `CT`'s two Set widths: the selector-only read (`CT0;`, 1 char) and the
/// write width (`CT0<P2>;`, 2 chars) — structurally identical shape to
/// [`MD_SET_FORMS`] (same "selector read" pattern), kept as its own named
/// const per this table's existing one-const-per-command convention (manual
/// p.5).
const CT_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 2),
];

/// `CN`'s two Set widths: the selector-only read (`CN0<P2>;`, 2 chars:
/// fixed P1 + the CTCSS/DCS table selector P2) and the write width
/// (`CN0<P2><3-digit P3>;`, 5 chars) — manual p.5.
const CN_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 2),
    CommandForm::fixed(CommandOperation::Set, 5),
];

/// `IS`'s two Set widths: the selector-only read (`IS0;`, 1 char) and the
/// write width (`IS0<sign><4-digit magnitude>;`, 6 chars — see the module
/// docs' "IS, a resolved manual discrepancy" section for why this is 4
/// digits, not the 3 the per-command box's column diagram literally shows).
const IS_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 6),
];

/// `KM`'s two Set widths: the selector-only read (`KM<P1>;`, 1 char — the
/// channel digit alone) and a genuinely **variable**-width write
/// (`KM<P1><P2>;`, 2-51 chars: 1-digit `P1` + 1 to 50 message characters,
/// manual p.10) — see the module docs' "KM, a genuine variable-width
/// selector read" section for why this uses `CommandForm::variable` rather
/// than a discrete list of widths like [`EX_SET_FORMS`].
const KM_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::variable(CommandOperation::Set, 2, 51),
];

/// `RA`'s two Set widths: the selector-only read (`RA0;`, 1 char) and the
/// write width (`RA0<P2>;`, 2 chars) — same "selector read" shape as
/// [`CT_SET_FORMS`] (manual p.15).
const RA_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 2),
];

/// `PA`'s two Set widths — same shape as [`RA_SET_FORMS`] (manual p.14).
const PA_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 2),
];

/// `NB`'s two Set widths — same shape as [`RA_SET_FORMS`] (manual p.13).
const NB_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 2),
];

/// `NR`'s two Set widths — same shape as [`RA_SET_FORMS`] (manual p.13).
const NR_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 2),
];

/// `BC`'s two Set widths — same shape as [`RA_SET_FORMS`] (manual p.4).
const BC_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 2),
];

/// `NA`'s two Set widths — same shape as [`RA_SET_FORMS`] (manual p.13). See
/// module docs' "NA, a genuine manual wire-diagram typo" section: the
/// per-command box's own wire cells literally read `M A P1 P2 ;` (not `N A
/// P1 P2 ;`), resolved in favor of the master-table code `NA`.
const NA_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 2),
];

/// `GT`'s two Set widths: the selector-only read (`GT0;`, 1 char) and the
/// write width (`GT0<P2>;`, 2 chars). Same total widths as [`RA_SET_FORMS`],
/// but `GT`'s Answer reports a wider domain (`P3`, 0-6) than `GT`'s own Set
/// accepts (`P2`, 0-4) at the *same* wire width — see module docs' "GT, AGC's
/// write/report domain mismatch" section (manual p.10).
const GT_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 2),
];

/// `NL`'s two Set widths: the selector-only read (`NL0;`, 1 char) and the
/// write width (`NL0<3-digit P2>;`, 4 chars — manual p.13).
const NL_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 4),
];

/// `RL`'s two Set widths: the selector-only read (`RL0;`, 1 char) and the
/// write width (`RL0<2-digit P2>;`, 3 chars — manual p.15).
const RL_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 3),
];

/// `SH`'s two Set widths: the selector-only read (`SH0;`, 1 char) and the
/// write width (`SH0<2-digit P2>;`, 3 chars — manual p.16). Same total
/// widths as [`RL_SET_FORMS`], kept as its own named const per this table's
/// one-const-per-command convention.
const SH_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 3),
];

/// `CO`'s two Set widths: the selector read (`CO0<P2>;`, 2 chars: fixed P1 +
/// item selector P2) and the write width (`CO0<P2><4-digit P3>;`, 6 chars —
/// manual p.5). See module docs' "CO, a 4-item selector" section.
const CO_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 2),
    CommandForm::fixed(CommandOperation::Set, 6),
];

/// `BP`'s two Set widths: the selector read (`BP0<P2>;`, 2 chars) and the
/// write width (`BP0<P2><3-digit P3>;`, 5 chars — manual p.5). Same shape as
/// [`CO_SET_FORMS`], one fewer `P3` digit.
const BP_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 2),
    CommandForm::fixed(CommandOperation::Set, 5),
];

/// `PR`'s two Set widths: the selector-only read (`PR<P1>;`, 1 char) and the
/// write width (`PR<P1><P2>;`, 2 chars) — same shape as
/// [`CT_SET_FORMS`]/[`RA_SET_FORMS`] (manual p.14). Unlike those, `PR`'s own
/// `P1` is a genuine two-valued feature selector (`0`=Speech Processor,
/// `1`=Parametric Mic EQ), not a fixed `"0"` byte — see module docs' "PR, a
/// genuine manual heading typo" section.
const PR_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 2),
];

/// `ML`'s two Set widths: the selector-only read (`ML<P1>;`, 1 char) and the
/// write width (`ML<P1><3-digit P2>;`, 4 chars — manual p.12). Same total
/// widths as [`NL_SET_FORMS`]; unlike `NL`, `ML`'s `P1` is a genuine
/// two-valued selector (`0`=MONI on/off, `1`=MONI level), not a fixed `"0"`
/// byte — see module docs' "ML, the batch's only composite command"
/// section.
const ML_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 4),
];

/// `DA`'s single Set width: `P1`(2, fixed "00") + `P2`(2) + `P3`(2) = 6
/// digits (manual p.6).
const SET_6: &[CommandForm] = &[CommandForm::fixed(CommandOperation::Set, 6)];

/// `DT`'s four Set widths: the selector-only read (`DT<P1>;`, 1 char) plus
/// the three P1-selected write widths — `1+5=6` (P1=2, time zone offset),
/// `1+6=7` (P1=1, time), `1+8=9` (P1=0, date) — manual p.6. See module docs'
/// "DT" section for the full per-shape citation.
const DT_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 6),
    CommandForm::fixed(CommandOperation::Set, 7),
    CommandForm::fixed(CommandOperation::Set, 9),
];

/// `OS`'s two Set widths: the selector-only read (`OS0;`, 1 char) and the
/// write width (`OS0<P2>;`, 2 chars) — same "selector read" shape as
/// [`CT_SET_FORMS`] (manual p.13).
const OS_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 2),
];

/// `LM`'s two Set widths — same shape as [`OS_SET_FORMS`] (manual p.11).
const LM_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 2),
];

/// `PB`'s two Set widths — same shape as [`OS_SET_FORMS`] (manual p.14).
const PB_SET_FORMS: &[CommandForm] = &[
    CommandForm::fixed(CommandOperation::Set, 1),
    CommandForm::fixed(CommandOperation::Set, 2),
];

macro_rules! definition {
    // Explicit controller read/write capability, plus action forms (batch
    // 1's `AB`/`BA`/`AM`/`VM`/`MA`/`QI`/`QR`/`QS`/`SV`, mirroring
    // `ts570d`'s own explicit-capability-plus-action macro arm).
    ($id:ident, $code:literal, $name:literal, $query:expr, $set:expr, $action:expr, $readable:expr, $writable:expr) => {
        CommandDefinition {
            id: Ft991aCommandId::$id,
            code: $code,
            name: $name,
            description: $name,
            query_forms: $query,
            set_forms: $set,
            action_forms: $action,
            response_forms: NONE,
            readable: $readable,
            writable: $writable,
        }
    };
    // Explicit controller read/write capability, no action forms (existing
    // call sites, e.g. MD/SM).
    ($id:ident, $code:literal, $name:literal, $query:expr, $set:expr, $readable:expr, $writable:expr) => {
        definition!($id, $code, $name, $query, $set, NONE, $readable, $writable)
    };
    // Derive read/write from the presence of query / set / action forms.
    ($id:ident, $code:literal, $name:literal, $query:expr, $set:expr, $action:expr) => {
        definition!(
            $id,
            $code,
            $name,
            $query,
            $set,
            $action,
            !$query.is_empty(),
            !$set.is_empty() || !$action.is_empty()
        )
    };
    // Derive read/write from the presence of query / set forms, no action
    // forms (existing call sites, the majority of this table).
    ($id:ident, $code:literal, $name:literal, $query:expr, $set:expr) => {
        definition!($id, $code, $name, $query, $set, NONE)
    };
}

static DEFINITIONS: &[CommandDefinition<Ft991aCommandId>] = &[
    definition!(Fa, "FA", "VFO A Frequency", QUERY0, SET_9),
    definition!(Fb, "FB", "VFO B Frequency", QUERY0, SET_9),
    definition!(Md, "MD", "Operating Mode", NONE, MD_SET_FORMS, true, true),
    definition!(Tx, "TX", "TX Set", QUERY0, SET_1),
    // SM: selector read (`SM0;`), no write at all (manual p.17 Set row is
    // blank) — readable/writable stated explicitly, not derived.
    definition!(Sm, "SM", "S-Meter Reading", NONE, SET_1, true, false),
    definition!(Ps, "PS", "Power Switch", QUERY0, SET_1),
    definition!(Ag, "AG", "AF Gain", QUERY0, SET_4),
    definition!(Rg, "RG", "RF Gain", QUERY0, SET_4),
    definition!(Sq, "SQ", "Squelch Level", QUERY0, SET_4),
    definition!(Pc, "PC", "Power Control", QUERY0, SET_3),
    definition!(Id, "ID", "Identification", QUERY0, NONE),
    // Batch 9: meters/status.
    //
    // IF/RS/UL are plain zero-width-query, read-only commands (manual Set
    // row blank): readable/writable derived from QUERY0/NONE, same pattern
    // as `Id`.
    definition!(If, "IF", "Information", QUERY0, NONE),
    definition!(Rs, "RS", "Radio Status", QUERY0, NONE),
    definition!(Ul, "UL", "PLL Unlock Status", QUERY0, NONE),
    // RM/RI are genuine "selector reads" (manual Read row carries a P1
    // selector, e.g. `RM3;`) — structurally a `Set` to the parser, same
    // treatment as `Sm`: explicit readable=true, writable=false.
    definition!(Rm, "RM", "Read Meter", NONE, SET_1, true, false),
    definition!(Ri, "RI", "Radio Information", NONE, SET_1, true, false),
    // MS is both readable (zero-width query) and writable (1-digit set).
    definition!(Ms, "MS", "Meter Select", QUERY0, SET_1),
    // EX menu: selector read (structurally a Set, same treatment as
    // Md/Rm/Ri above) — explicit readable=true, writable=true (both
    // capabilities live inside EX_SET_FORMS' width-3 vs. width-{4..11}
    // forms, so `!query_forms.is_empty()`-style derivation doesn't apply).
    definition!(Ex, "EX", "Menu", NONE, EX_SET_FORMS, true, true),
    // Batch 2: memory channel records. `Mc` is a plain zero-width-query
    // command like `Fa`/`Ps` (readable/writable derived). `Mr`/`Mw` are
    // one-directional (manual p.3: `MR` Set=X, `MW` Read=X/Ans=X) —
    // explicit readable/writable, same treatment as `Sm`/`Rm`/`Ri` above.
    // `Mt` is a genuine "selector read" like `Md`/`Ex` (both readable and
    // writable; both capabilities live inside MT_SET_FORMS' two widths).
    definition!(Mc, "MC", "Memory Channel", QUERY0, SET_3),
    definition!(Mr, "MR", "Memory Channel Read", NONE, SET_3, true, false),
    definition!(Mw, "MW", "Memory Channel Write", NONE, SET_25, false, true),
    definition!(
        Mt,
        "MT",
        "Memory Channel Write/Tag",
        NONE,
        MT_SET_FORMS,
        true,
        true
    ),
    // Batch 1: VFO/split/memory quick-ops. Nine of the ten are zero-width
    // Action triggers (manual p.3: Set O, Read X, Ans X for all ten rows);
    // `Ch` is the exception, a required 1-digit Set selector, no Action
    // form at all (there is no `"CH;"` with no parameter — manual p.5).
    definition!(Ab, "AB", "VFO-A to VFO-B", NONE, NONE, ACTION),
    definition!(Ba, "BA", "VFO-B to VFO-A", NONE, NONE, ACTION),
    definition!(Am, "AM", "VFO-A to Memory Channel", NONE, NONE, ACTION),
    definition!(Vm, "VM", "[V/M] Key Function", NONE, NONE, ACTION),
    definition!(Ma, "MA", "Memory Channel to VFO-A", NONE, NONE, ACTION),
    definition!(Ch, "CH", "Channel Up/Down", NONE, SET_1),
    definition!(Qi, "QI", "QMB Store", NONE, NONE, ACTION),
    definition!(Qr, "QR", "QMB Recall", NONE, NONE, ACTION),
    definition!(Qs, "QS", "Quick Split", NONE, NONE, ACTION),
    definition!(Sv, "SV", "Swap VFO", NONE, NONE, ACTION),
    // Batch 3: clarifier/RIT-XIT + tone + IF-shift. RT/XT are plain
    // zero-width-query commands like Ps/Ms (readable/writable derived). RC
    // is a zero-width Action trigger like the batch-1 quick-ops. RD/RU are
    // write-only (manual p.3: Set O, Read X, Ans X) — explicit
    // readable/writable, same treatment as Sm/Rm/Ri/Mr/Mw above. CN/CT/IS
    // are genuine "selector reads" like Md/Ex/Mt (both readable and
    // writable; both capabilities live inside their own two-width forms).
    definition!(Rt, "RT", "RX Clarifier", QUERY0, SET_1),
    definition!(Rc, "RC", "Clarifier Clear", NONE, NONE, ACTION),
    definition!(Rd, "RD", "Clarifier Down", NONE, SET_4, false, true),
    definition!(
        Ru,
        "RU",
        "Clarifier Up (RX Clarifier Plus Offset)",
        NONE,
        SET_4,
        false,
        true
    ),
    definition!(Xt, "XT", "TX Clarifier", QUERY0, SET_1),
    definition!(Cn, "CN", "CTCSS/DCS Number", NONE, CN_SET_FORMS, true, true),
    definition!(Ct, "CT", "CTCSS/DCS Mode", NONE, CT_SET_FORMS, true, true),
    definition!(Is, "IS", "IF Shift", NONE, IS_SET_FORMS, true, true),
    // Batch 4: keyer/CW/break-in. KM is a genuine "selector read" like
    // Md/Ex/Mt (both readable and writable; both capabilities live inside
    // KM_SET_FORMS' two widths). KP/KR/KS/CS/BI/SD are plain
    // zero-width-query commands like Ps/Ms/Rt/Xt (readable/writable
    // derived). KY is write-only with a required 1-char selector, no
    // Action form (manual p.3: Set O Read X Ans X) — same shape as
    // batch 1's Ch. ZI is a zero-width Action trigger like Rc/the batch-1
    // quick-ops.
    definition!(Km, "KM", "Keyer Memory", NONE, KM_SET_FORMS, true, true),
    definition!(Kp, "KP", "Key Pitch", QUERY0, SET_2),
    definition!(Kr, "KR", "Keyer", QUERY0, SET_1),
    definition!(Ks, "KS", "Key Speed", QUERY0, SET_3),
    definition!(Ky, "KY", "CW Keying", NONE, SET_1),
    definition!(Cs, "CS", "CW Spot", QUERY0, SET_1),
    definition!(Zi, "ZI", "Zero In", NONE, NONE, ACTION),
    definition!(Bi, "BI", "Break-In", QUERY0, SET_1),
    definition!(Sd, "SD", "CW Break-In Delay Time", QUERY0, SET_4),
    // Batch 5: scan/VOX/busy. SC/VX/VD/VG are plain zero-width-query
    // commands like Ps/Ms/Rt/Xt/Kp/Ks (readable/writable derived). BY is
    // read-only (manual p.3: Set X Read O Ans O) — derived from QUERY0/NONE,
    // same pattern as If/Rs/Ul above (no explicit readable/writable needed).
    definition!(Sc, "SC", "Scan", QUERY0, SET_1),
    definition!(Vx, "VX", "VOX Status", QUERY0, SET_1),
    definition!(Vd, "VD", "VOX Delay Time", QUERY0, SET_4),
    definition!(Vg, "VG", "VOX Gain", QUERY0, SET_3),
    definition!(By, "BY", "Busy", QUERY0, NONE),
    // -- Batch 6: attenuator/preamp/noise/AGC/notch/filter-width -------
    //
    // All twelve are "selector read" shapes (capability lives entirely in
    // `set_forms`, `query_forms` is NONE) — same treatment as `CT`/`CN`/`IS`
    // above.
    definition!(Ra, "RA", "RF Attenuator", NONE, RA_SET_FORMS, true, true),
    definition!(Pa, "PA", "Pre-Amp (IPO)", NONE, PA_SET_FORMS, true, true),
    definition!(
        Nb,
        "NB",
        "Noise Blanker Status",
        NONE,
        NB_SET_FORMS,
        true,
        true
    ),
    definition!(
        Nl,
        "NL",
        "Noise Blanker Level",
        NONE,
        NL_SET_FORMS,
        true,
        true
    ),
    definition!(Nr, "NR", "Noise Reduction", NONE, NR_SET_FORMS, true, true),
    definition!(
        Rl,
        "RL",
        "Noise Reduction Level",
        NONE,
        RL_SET_FORMS,
        true,
        true
    ),
    definition!(Gt, "GT", "AGC Function", NONE, GT_SET_FORMS, true, true),
    definition!(Co, "CO", "Contour", NONE, CO_SET_FORMS, true, true),
    definition!(Bp, "BP", "Manual Notch", NONE, BP_SET_FORMS, true, true),
    definition!(Bc, "BC", "Auto Notch", NONE, BC_SET_FORMS, true, true),
    definition!(Na, "NA", "Narrow", NONE, NA_SET_FORMS, true, true),
    definition!(Sh, "SH", "Width", NONE, SH_SET_FORMS, true, true),
    // -- Batch 7: speech processor/mic/monitor --------------------------
    //
    // `MG`/`PL` are plain query/set, no selector byte at all (same shape as
    // `PC`). `PR`/`ML` are "selector read" shapes (capability lives
    // entirely in `set_forms`, `query_forms` is NONE) — same treatment as
    // `CT`/`RA` above.
    definition!(Mg, "MG", "Mic Gain", QUERY0, SET_3),
    definition!(Pl, "PL", "Speech Processor Level", QUERY0, SET_3),
    definition!(Pr, "PR", "Speech Processor", NONE, PR_SET_FORMS, true, true),
    definition!(Ml, "ML", "Monitor Level", NONE, ML_SET_FORMS, true, true),
    // -- Batch 8: band/step/encoder front-panel controls -----------------
    //
    // `BS` is Set-only, 2-digit (readable/writable derived from
    // NONE/SET_2). `BU`/`BD`/`ED`/`EU` are Set-only with a required literal
    // digit or digits (derived from NONE/SET_1 or NONE/SET_3, same
    // treatment as `KY`). `FS` is a plain bidirectional bool (QUERY0/SET_1,
    // same shape as `RT`/`XT`/`CS`/`BI`). `EK`/`DN`/`UP` are zero-width
    // Action triggers (same treatment as `RC`/`ZI`/batch 1's quick-ops).
    definition!(Bs, "BS", "Band Select", NONE, SET_2),
    definition!(Bu, "BU", "Band Up", NONE, SET_1),
    definition!(Bd, "BD", "Band Down", NONE, SET_1),
    definition!(Fs, "FS", "Fast Step", QUERY0, SET_1),
    definition!(Ed, "ED", "Encoder Down", NONE, SET_3),
    definition!(Eu, "EU", "Encoder Up", NONE, SET_3),
    definition!(Ek, "EK", "Ent Key", NONE, NONE, ACTION),
    definition!(Dn, "DN", "Down (MIC DWN)", NONE, NONE, ACTION),
    definition!(Up, "UP", "Up", NONE, NONE, ACTION),
    // -- Batch 10 (last of the 10 core batches): misc system/TX/tuner/DVS --
    //
    // `AC`/`AI`/`LK`/`FT`/`TS`/`MX` are plain zero-width-query commands
    // (readable/writable derived). `DA` is likewise plain query/set, no
    // selector byte (same shape as `MG`/`PL`). `OI` is read-only, zero-width
    // query (derived from QUERY0/NONE, same pattern as `IF`/`BY`). `DT`/
    // `OS`/`LM`/`PB` are "selector read" shapes (capability lives entirely
    // in `set_forms`, `query_forms` is NONE) — same treatment as `CT`/`RA`.
    definition!(Ac, "AC", "Antenna Tuner Control", QUERY0, SET_3),
    definition!(Ai, "AI", "Auto Information", QUERY0, SET_1),
    definition!(Da, "DA", "Dimmer", QUERY0, SET_6),
    definition!(Dt, "DT", "Date and Time", NONE, DT_SET_FORMS, true, true),
    definition!(Lk, "LK", "Lock", QUERY0, SET_1),
    definition!(Oi, "OI", "Opposite Band Information", QUERY0, NONE),
    definition!(
        Os,
        "OS",
        "Offset (Repeater Shift)",
        NONE,
        OS_SET_FORMS,
        true,
        true
    ),
    definition!(Ft, "FT", "Function TX", QUERY0, SET_1),
    definition!(Ts, "TS", "TXW", QUERY0, SET_1),
    definition!(Mx, "MX", "MOX Set", QUERY0, SET_1),
    definition!(
        Lm,
        "LM",
        "Load Message (DVS Record)",
        NONE,
        LM_SET_FORMS,
        true,
        true
    ),
    definition!(
        Pb,
        "PB",
        "Play Back (DVS Playback)",
        NONE,
        PB_SET_FORMS,
        true,
        true
    ),
];

/// FT-991A command table used by the generic framework.
pub static FT991A_COMMAND_TABLE: CommandTable<Ft991aCommandId> = CommandTable::new(DEFINITIONS);

/// FT-991A's fixed radio identifier (manual p.10). Not stored state — the
/// 4-character answer is treated as an opaque string, not parsed as hex or
/// decimal (the manual gives no basis to prefer either interpretation).
pub const FT991A_ID: &str = "0670";

/// `CN`'s Table 1 (CTCSS Tone Chart, manual printed p.6), indexed
/// `000`-`049` (index into this array). Stored in **deci-Hz** (tenths of a
/// Hertz, i.e. the manual's Hz value × 10) rather than `f32` directly — a
/// documented judgment call, not manual-cited — so that
/// [`ctcss_tone_index`] can look up a caller-supplied Hz value by exact
/// integer equality (rounding to the nearest tenth first) instead of
/// comparing floats for equality, which is unsound in general. Transcribed
/// in full from the manual's own table image and cross-checked against the
/// well-known standard 50-tone CTCSS list used industry-wide (see module
/// docs' "CN, the two lookup tables" section) — no entries approximated or
/// sampled.
#[rustfmt::skip]
pub const CTCSS_TONES_DECIHZ: [u16; 50] = [
    670, 693, 719, 744, 770, 797, 825, 854, 885, 915,
    948, 974, 1000, 1035, 1072, 1109, 1148, 1188, 1230, 1273,
    1318, 1365, 1413, 1462, 1514, 1567, 1598, 1622, 1655, 1679,
    1713, 1738, 1773, 1799, 1835, 1862, 1899, 1928, 1966, 1995,
    2035, 2065, 2107, 2181, 2257, 2291, 2336, 2418, 2503, 2541,
];

/// `CN`'s Table 2 (DCS Code Chart, manual printed p.6), indexed
/// `000`-`103` (index into this array), values are the standard 3-digit
/// (octal-style) DCS code numbers. Transcribed in full from the manual's
/// own table image and cross-checked against the well-known standard
/// 104-code DCS list used industry-wide (see module docs' "CN, the two
/// lookup tables" section) — one single-digit misread on the initial image
/// pass (index 078) was caught and corrected this way, documented there.
#[rustfmt::skip]
pub const DCS_CODES: [u16; 104] = [
    23, 25, 26, 31, 32, 36, 43, 47, 51, 53,
    54, 65, 71, 72, 73, 74, 114, 115, 116, 122,
    125, 131, 132, 134, 143, 145, 152, 155, 156, 162,
    165, 172, 174, 205, 212, 223, 225, 226, 243, 244,
    245, 246, 251, 252, 255, 261, 263, 265, 266, 271,
    274, 306, 311, 315, 325, 331, 332, 343, 346, 351,
    356, 364, 365, 371, 411, 412, 413, 423, 431, 432,
    445, 446, 452, 454, 455, 462, 464, 465, 466, 503,
    506, 516, 523, 526, 532, 546, 565, 606, 612, 624,
    627, 631, 632, 654, 662, 664, 703, 712, 723, 731,
    732, 734, 743, 754,
];

/// Look up a CTCSS tone's [`CTCSS_TONES_DECIHZ`] table index by its
/// frequency in Hz, rounding to the nearest tenth of a Hz before comparing
/// (see [`CTCSS_TONES_DECIHZ`]'s doc comment for why). Returns `None` if no
/// table entry matches.
pub fn ctcss_tone_index(hz: f32) -> Option<u8> {
    let decihz = (hz * 10.0).round() as i32;
    CTCSS_TONES_DECIHZ
        .iter()
        .position(|&v| i32::from(v) == decihz)
        .map(|i| i as u8)
}

/// Look up a CTCSS tone's frequency in Hz by its [`CTCSS_TONES_DECIHZ`]
/// table index. Returns `None` if `index` is out of range (`>= 50`).
pub fn ctcss_tone_hz(index: u8) -> Option<f32> {
    CTCSS_TONES_DECIHZ
        .get(index as usize)
        .map(|&decihz| f32::from(decihz) / 10.0)
}

/// Look up a DCS code's [`DCS_CODES`] table index by its 3-digit code
/// number. Returns `None` if no table entry matches.
pub fn dcs_code_index(code: u16) -> Option<u8> {
    DCS_CODES.iter().position(|&v| v == code).map(|i| i as u8)
}

/// Look up a DCS code number by its [`DCS_CODES`] table index. Returns
/// `None` if `index` is out of range (`>= 104`).
pub fn dcs_code_number(index: u8) -> Option<u16> {
    DCS_CODES.get(index as usize).copied()
}

/// One of the three mode families `SH`'s bandwidth table distinguishes
/// (manual p.16's six-column table header: SSB, CW, RTTY/PSK, each split
/// into Narrow/Wide). See [`mode_family_for`] for how a raw [`Mode`]
/// (`crate::radio_trait::Mode`) nibble maps onto this — including which
/// modes are deliberately left unmapped — and the module docs' "SH" section
/// for the full citation and reasoning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeFamily {
    Ssb,
    Cw,
    RttyPsk,
}

/// Map a raw mode nibble (manual p.11's `MD`/`IF` legend, `1`-`0xE`) onto
/// the [`ModeFamily`] `SH`'s bandwidth table names, or `None` if the mode
/// has no `SH`-adjustable bandwidth in this table at all.
///
/// **Documented judgment call, not manual-stated**: `SH`'s own table header
/// (manual p.16) names only three families — "SSB", "CW", "RTTY/PSK" —
/// literally matching `LSB`/`USB`, `CW`/`CW-R`, and `RTTY-LSB`/`RTTY-USB`
/// respectively. Nothing on `SH`'s own page states whether `DATA-LSB`/
/// `DATA-USB`/`DATA-FM` (digital modes that, on real hardware, typically
/// ride over an SSB or FM carrier) share the "SSB" or "RTTY/PSK" family, or
/// have their own unstated bandwidth behavior — this implementation
/// conservatively maps them (and `FM`/`FM-N`/`AM`/`AM-N`/`C4FM`) to `None`
/// rather than guessing, since no manual text disambiguates them. See the
/// module docs' "SH, the batch's highest-risk item" section.
pub fn mode_family_for(mode: u8) -> Option<ModeFamily> {
    match mode {
        0x1 | 0x2 => Some(ModeFamily::Ssb),     // LSB, USB
        0x3 | 0x7 => Some(ModeFamily::Cw),      // CW-U, CW-L
        0x6 | 0x9 => Some(ModeFamily::RttyPsk), // RTTY-LSB, RTTY-USB
        _ => None,
    }
}

/// One row of [`SH_BANDWIDTH_TABLE`]: the bandwidth in Hz for each of the
/// six (family, narrow/wide) columns at one `P2` index, or `None` for a `-`
/// cell (a `P2` value illegal for that particular column).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShBandwidthRow {
    pub ssb_narrow: Option<u16>,
    pub ssb_wide: Option<u16>,
    pub cw_narrow: Option<u16>,
    pub cw_wide: Option<u16>,
    pub rtty_psk_narrow: Option<u16>,
    pub rtty_psk_wide: Option<u16>,
}

/// `SH`'s full bandwidth table (manual p.16), transcribed row-for-row and
/// column-for-column from the manual's own table image, cross-checked
/// against `pdftotext -layout`'s independent extraction of the same page
/// (both agree exactly, no discrepancy found). Indexed by `P2` (`table[p2
/// as usize]`, `00`-`21`). `SH`'s own Set command validates only the raw
/// `0..=21` range — this table is consulted only by [`filter_bandwidth_hz`],
/// a pure convenience lookup, not by `Ft991aRadio::handle_command`'s `Sh`
/// write arm (see module docs' "SH" section for why cross-referencing
/// current mode/`NA` state at write time was deliberately not attempted).
#[rustfmt::skip]
pub const SH_BANDWIDTH_TABLE: [ShBandwidthRow; 22] = [
    // P2=00 (Default)
    ShBandwidthRow { ssb_narrow: Some(1500), ssb_wide: Some(2400), cw_narrow: Some(500), cw_wide: Some(2400), rtty_psk_narrow: Some(300), rtty_psk_wide: Some(500) },
    // P2=01
    ShBandwidthRow { ssb_narrow: Some(200), ssb_wide: None, cw_narrow: Some(50), cw_wide: None, rtty_psk_narrow: Some(50), rtty_psk_wide: None },
    // P2=02
    ShBandwidthRow { ssb_narrow: Some(400), ssb_wide: None, cw_narrow: Some(100), cw_wide: None, rtty_psk_narrow: Some(100), rtty_psk_wide: None },
    // P2=03
    ShBandwidthRow { ssb_narrow: Some(600), ssb_wide: None, cw_narrow: Some(150), cw_wide: None, rtty_psk_narrow: Some(150), rtty_psk_wide: None },
    // P2=04
    ShBandwidthRow { ssb_narrow: Some(850), ssb_wide: None, cw_narrow: Some(200), cw_wide: None, rtty_psk_narrow: Some(200), rtty_psk_wide: None },
    // P2=05
    ShBandwidthRow { ssb_narrow: Some(1100), ssb_wide: None, cw_narrow: Some(250), cw_wide: None, rtty_psk_narrow: Some(250), rtty_psk_wide: None },
    // P2=06
    ShBandwidthRow { ssb_narrow: Some(1350), ssb_wide: None, cw_narrow: Some(300), cw_wide: None, rtty_psk_narrow: Some(300), rtty_psk_wide: None },
    // P2=07
    ShBandwidthRow { ssb_narrow: Some(1500), ssb_wide: None, cw_narrow: Some(350), cw_wide: None, rtty_psk_narrow: Some(350), rtty_psk_wide: None },
    // P2=08
    ShBandwidthRow { ssb_narrow: Some(1650), ssb_wide: None, cw_narrow: Some(400), cw_wide: None, rtty_psk_narrow: Some(400), rtty_psk_wide: None },
    // P2=09
    ShBandwidthRow { ssb_narrow: Some(1800), ssb_wide: Some(1800), cw_narrow: Some(450), cw_wide: None, rtty_psk_narrow: Some(450), rtty_psk_wide: None },
    // P2=10
    ShBandwidthRow { ssb_narrow: None, ssb_wide: Some(1950), cw_narrow: Some(500), cw_wide: Some(500), rtty_psk_narrow: Some(500), rtty_psk_wide: Some(500) },
    // P2=11
    ShBandwidthRow { ssb_narrow: None, ssb_wide: Some(2100), cw_narrow: None, cw_wide: Some(800), rtty_psk_narrow: None, rtty_psk_wide: Some(800) },
    // P2=12
    ShBandwidthRow { ssb_narrow: None, ssb_wide: Some(2200), cw_narrow: None, cw_wide: Some(1200), rtty_psk_narrow: None, rtty_psk_wide: Some(1200) },
    // P2=13
    ShBandwidthRow { ssb_narrow: None, ssb_wide: Some(2300), cw_narrow: None, cw_wide: Some(1400), rtty_psk_narrow: None, rtty_psk_wide: Some(1400) },
    // P2=14
    ShBandwidthRow { ssb_narrow: None, ssb_wide: Some(2400), cw_narrow: None, cw_wide: Some(1700), rtty_psk_narrow: None, rtty_psk_wide: Some(1700) },
    // P2=15
    ShBandwidthRow { ssb_narrow: None, ssb_wide: Some(2500), cw_narrow: None, cw_wide: Some(2000), rtty_psk_narrow: None, rtty_psk_wide: Some(2000) },
    // P2=16
    ShBandwidthRow { ssb_narrow: None, ssb_wide: Some(2600), cw_narrow: None, cw_wide: Some(2400), rtty_psk_narrow: None, rtty_psk_wide: Some(2400) },
    // P2=17
    ShBandwidthRow { ssb_narrow: None, ssb_wide: Some(2700), cw_narrow: None, cw_wide: Some(3000), rtty_psk_narrow: None, rtty_psk_wide: Some(3000) },
    // P2=18
    ShBandwidthRow { ssb_narrow: None, ssb_wide: Some(2800), cw_narrow: None, cw_wide: None, rtty_psk_narrow: None, rtty_psk_wide: None },
    // P2=19
    ShBandwidthRow { ssb_narrow: None, ssb_wide: Some(2900), cw_narrow: None, cw_wide: None, rtty_psk_narrow: None, rtty_psk_wide: None },
    // P2=20
    ShBandwidthRow { ssb_narrow: None, ssb_wide: Some(3000), cw_narrow: None, cw_wide: None, rtty_psk_narrow: None, rtty_psk_wide: None },
    // P2=21
    ShBandwidthRow { ssb_narrow: None, ssb_wide: Some(3200), cw_narrow: None, cw_wide: None, rtty_psk_narrow: None, rtty_psk_wide: None },
];

/// Look up `SH`'s actual bandwidth in Hz for a given mode family,
/// narrow/wide selection (see module docs' "SH" section for why the
/// narrow/wide parameter — a well-evidenced but not literally-stated stand-
/// in for `NA`'s on/off state — is an explicit argument here rather than
/// read from state directly), and raw `P2` table index. Returns `None` if
/// `p2` is out of range (`>= 22`) or the `(family, narrow)` column has no
/// legal value at that row (a `-` cell in [`SH_BANDWIDTH_TABLE`]).
pub fn filter_bandwidth_hz(family: ModeFamily, narrow: bool, p2: u8) -> Option<u16> {
    let row = SH_BANDWIDTH_TABLE.get(p2 as usize)?;
    match (family, narrow) {
        (ModeFamily::Ssb, true) => row.ssb_narrow,
        (ModeFamily::Ssb, false) => row.ssb_wide,
        (ModeFamily::Cw, true) => row.cw_narrow,
        (ModeFamily::Cw, false) => row.cw_wide,
        (ModeFamily::RttyPsk, true) => row.rtty_psk_narrow,
        (ModeFamily::RttyPsk, false) => row.rtty_psk_wide,
    }
}

/// Convert `CO`'s `P2=3` (APF FREQ) raw wire value (`0000`-`0050`) into Hz
/// (`-250`..=`250`, 10 Hz steps) — manual p.5: `"0000-0050 (APF Frequency:
/// -250 - 250 Hz)"`, a documented linear-mapping judgment call (`raw=0` →
/// `-250`, `raw=25` → `0`, `raw=50` → `+250`), since the manual gives the
/// endpoints and step count but not an explicit formula.
pub fn apf_raw_to_hz(raw: u8) -> i16 {
    (i16::from(raw) - 25) * 10
}

/// Inverse of [`apf_raw_to_hz`]. Callers must have already validated `hz` is
/// in `-250..=250` and a multiple of 10.
pub fn apf_hz_to_raw(hz: i16) -> u16 {
    ((hz / 10) + 25) as u16
}

/// How an `EX` menu item's P2 value is encoded on the wire.
///
/// The first sub-batch's 9 items were all small named pick-lists
/// (`Enumerated`). This second sub-batch (menu numbers 001-046) adds
/// genuine continuous numeric ranges — e.g. 001 "AGC FAST DELAY", `0020 ~
/// 4000` msec in 20 msec steps — which the manual expresses as a
/// range/step formula rather than an exhaustive value legend, so a third
/// variant (`Range`) was added rather than enumerating hundreds of literal
/// wire strings per item.
#[derive(Debug, Clone, Copy)]
pub enum ExMenuValueKind {
    /// `(wire, label)` pairs from the manual's own P2 legend, e.g.
    /// `&[("0", "OFF"), ("1", "DAKY"), ("2", "RTS"), ("3", "DTR")]` for item
    /// 060 "PC KEYING". The wire value is what actually goes on the CAT
    /// wire (unchanged in meaning from this field's original bare
    /// `&'static [&'static str]` shape); the label is the manual's own
    /// human-readable name for that value, added so a `ListSelect`-driven
    /// UI can show named options instead of raw digit strings (Wave 4,
    /// `planning/architect/task_plan.md` §11.4's flagged UX gap). Every
    /// label below is transcribed from this file's own pre-existing
    /// [`EX_MENU_TABLE`] doc-comment legend tables (the manual citations
    /// already recorded there), not re-read from the manual PDF fresh.
    ///
    /// **Not necessarily zero-based or contiguous**: items 072/077 ("DATA
    /// PORT SELECT"/"FM PKT PORT SELECT") legally start at `"1"` (`1: DATA
    /// 2: USB`), while items 048/109 ("AM PORT SELECT"/"SSB PORT SELECT")
    /// start at `"0"` (`0: DATA 1: USB`) for the same DATA/USB concept — a
    /// genuine manual inconsistency, transcribed exactly rather than
    /// silently normalized to one convention. Item 028 "GPS/232C SELECT"
    /// (`0:GPS1 1:GPS2 3:RS232C`) has a documented gap at `2` — also
    /// transcribed exactly, same treatment `RI`'s selector gap (batch 9)
    /// already established. See [`EX_MENU_TABLE`]'s doc comment.
    Enumerated(&'static [(&'static str, &'static str)]),
    /// A numeric range `min..=max` at a fixed `step`, all already in the
    /// manual's own raw P2 wire units — any documented scale factor (e.g.
    /// item 014 "CW WEIGHT"'s "2.5 ~ 4.5" ratio is wire-encoded as `P2 =
    /// 25 ~ 45`) is pre-applied here so `min`/`max`/`step` are the literal
    /// wire integers, not the display units. `signed` marks whether the
    /// wire form's first character is an explicit `+`/`-` sign (consuming
    /// one of [`ExMenuItem::digits`] characters, magnitude zero-padded to
    /// the remaining `digits - 1` characters) rather than the value being
    /// an unsigned zero-padded integer across all `digits` characters —
    /// e.g. item 035 "QUICK SPLIT FREQ", digits=3, wire `"+00"`/`"-00"`/
    /// `"+20"`/`"-20"` for -20..=20 kHz (the manual explicitly allows both
    /// `"+00"` and `"-00"` for zero; parsing sign+magnitude handles both
    /// without a special case).
    Range {
        min: i32,
        max: i32,
        step: i32,
        signed: bool,
    },
}

impl ExMenuValueKind {
    /// Parse a wire-format P2 string — already confirmed by the caller to
    /// be exactly [`ExMenuItem::digits`] characters — into its integer
    /// value, or `None` if it is not legal for this kind (wrong sign
    /// character, non-digit content, out of range, or off the declared
    /// `step`).
    ///
    /// `pub(crate)` (not private) so `ft991a.rs`'s
    /// `Ft991a::get_ex_menu_item`/`set_ex_menu_item` (`Ft991aExtras`, Wave
    /// 4) can reuse the exact same parse logic `handle_command`'s `Ex` arm
    /// already uses, rather than reimplementing it client-side.
    pub(crate) fn parse(&self, wire: &str) -> Option<i32> {
        match self {
            ExMenuValueKind::Enumerated(values) => {
                if values.iter().any(|(w, _)| *w == wire) {
                    wire.parse().ok()
                } else {
                    None
                }
            }
            ExMenuValueKind::Range {
                min,
                max,
                step,
                signed,
            } => {
                let value = if *signed {
                    let mut chars = wire.chars();
                    let sign = match chars.next()? {
                        '+' => 1,
                        '-' => -1,
                        _ => return None,
                    };
                    let magnitude: i32 = chars.as_str().parse().ok()?;
                    sign * magnitude
                } else {
                    wire.parse().ok()?
                };
                if value < *min || value > *max || (value - min) % step != 0 {
                    return None;
                }
                Some(value)
            }
        }
    }

    /// Format an already-validated integer `value` back onto the wire at
    /// `digits` total width (zero-padded; signed ranges reserve the first
    /// character for an explicit `+`/`-`).
    ///
    /// `pub(crate)` for the same reason as [`Self::parse`] — reused by
    /// `ft991a.rs`'s `get_ex_menu_item`/`set_ex_menu_item`.
    pub(crate) fn format(&self, value: i32, digits: usize) -> String {
        match self {
            ExMenuValueKind::Range { signed: true, .. } => {
                let sign = if value < 0 { '-' } else { '+' };
                format!("{sign}{:0width$}", value.abs(), width = digits - 1)
            }
            _ => format!("{value:0digits$}"),
        }
    }

    /// Look up the manual's human-readable label for an already-validated
    /// integer `value` (as produced by [`Self::parse`]/read back from
    /// state) — `None` for [`ExMenuValueKind::Range`] (no discrete labels
    /// to show) or for a `value` that doesn't match any of this
    /// [`ExMenuValueKind::Enumerated`] variant's wire values (shouldn't
    /// happen for a value that already round-tripped through `parse`, but
    /// handled without panicking either way). Intended for a future
    /// `ListSelect`-driven UI (`planning/architect/task_plan.md` §11.4) —
    /// not consumed anywhere in this crate yet.
    pub fn label_for_value(&self, value: i32, digits: usize) -> Option<&'static str> {
        match self {
            ExMenuValueKind::Enumerated(values) => {
                let wire = self.format(value, digits);
                values
                    .iter()
                    .find(|(w, _)| *w == wire)
                    .map(|(_, label)| *label)
            }
            ExMenuValueKind::Range { .. } => None,
        }
    }
}

/// One `EX` menu item's static shape: its manual name, its P2 wire width
/// (the `"Digits"` column, manual p.7-9), and its value encoding/legal
/// range ([`ExMenuValueKind`], transcribed from the manual's own P2
/// legend).
///
/// Only the items landed so far have rows in [`EX_MENU_TABLE`] — see that
/// constant's doc comment for the full per-item transcription and
/// citations, and the module docs for why the remaining items are out of
/// scope here.
#[derive(Debug, Clone, Copy)]
pub struct ExMenuItem {
    /// P1: the 3-digit menu number, 001-153 (stored without leading
    /// zeros — the wire form always zero-pads to 3 digits on both read
    /// and write).
    pub p1: u16,
    /// Manual's own item name (e.g. `"PC KEYING"`).
    pub name: &'static str,
    /// P2's wire width in ASCII digits (this item's own "Digits" column
    /// value, manual p.7-9) — must equal the actual parameter's remaining
    /// length after the 3-digit P1, checked in `handle_command`'s `Ex`
    /// arm before `kind` is consulted.
    pub digits: usize,
    /// This item's value encoding — see [`ExMenuValueKind`].
    pub kind: ExMenuValueKind,
}

/// The `EX` menu items landed so far (manual p.7-9's full 153-row table;
/// see module docs for the sub-batching rationale).
///
/// # First sub-batch (9 items): 047, 048, 060, 071, 072, 076, 077, 108, 109
///
/// Transcribed and cross-checked via two independent extraction methods
/// against `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf` printed p.7-9 (PDF
/// pages 8-10, per the documented "+1" cover-page offset,
/// `planning/yaesu/task_plan.md`): the rendered page image (read directly,
/// column-by-column, per this crate's established `IF`-verification
/// practice) and `pdftotext -layout` (to independently confirm the
/// "Digits" column, which the image-render pass misread for one row — see
/// below).
///
/// | P1  | Name                | P2 legal values         | Digits |
/// |-----|---------------------|--------------------------|--------|
/// | 047 | AM PTT SELECT       | `0:DAKY 1:RTS 2:DTR`     | 1 |
/// | 048 | AM PORT SELECT      | `0:DATA 1:USB`           | 1 |
/// | 060 | PC KEYING           | `0:OFF 1:DAKY 2:RTS 3:DTR` | 1 |
/// | 071 | DATA PTT SELECT     | `0:DAKY 1:RTS 2:DTR`     | 1 |
/// | 072 | DATA PORT SELECT    | `1:DATA 2:USB`           | 1 |
/// | 076 | FM PKT PTT SELECT   | `0:DAKY 1:RTS 2:DTR`     | 1 |
/// | 077 | FM PKT PORT SELECT  | `1:DATA 2:USB`           | 1 |
/// | 108 | SSB PTT SELECT      | `0:DAKY 1:RTS 2:DTR`     | 1 |
/// | 109 | SSB PORT SELECT     | `0:DATA 1:USB`           | 1 |
///
/// **060 "PC KEYING" is the item this wave's marquee RTS/DTR CW-keying
/// feature (`planning/architect/task_plan.md` §10.2-10.4) actually
/// reads/writes** — `0`=OFF, `1`=DAKY (rear DAKY jack), `2`=RTS, `3`=DTR.
/// The other 8 items (047/048/071/072/076/077/108/109) are a *different*,
/// unrelated family — per-mode PTT/port selection for the rear DATA jack
/// vs. the USB audio/control virtual port, used for soundcard-driven
/// digital-mode/voice PTT, not CAT control (§10.2's correction) — included
/// here only because §10.6's priority carve-out front-loads them alongside
/// 060 as "cheap to complete in software," not because this app has any
/// other reason to use them yet.
///
/// **Digit-width cross-check, one discrepancy resolved**: the rendered
/// page image's OCR-like text extraction initially showed item 072
/// ("DATA PORT SELECT") with Digits=`3` — inconsistent with every other
/// `*PORT SELECT` item (048, 077, 109, all Digits=`1`) and with the fact
/// that a 2-value enumerated field (`1: DATA`, `2: USB`) cannot need 3
/// wire digits. Re-extracting the same page with `pdftotext -layout`
/// confirmed Digits=`1` for item 072 (matching the image-render's
/// immediately-following row 073 "DATA OUT LEVEL," Digits=`3` — the
/// image-OCR pass had almost certainly attributed 073's digit count to
/// 072). Resolved via cross-tool + sibling-row-pattern agreement, not
/// guessed; **not** a case that needed escalating per this task's "STOP if
/// ambiguous" instruction, since two independent extractions converged
/// with logic on the same answer.
///
/// **Genuine manual inconsistency, transcribed as-is (not resolved)**:
/// items 048/109 encode DATA/USB as `0`/`1`, while items 072/077 encode
/// the identical DATA/USB concept as `1`/`2`. Both pairs are independently
/// confirmed present verbatim on the manual page (not a typo introduced by
/// this transcription).
///
/// # Second sub-batch (45 items): 001-046, minus 027 (skipped, unresolved)
///
/// Manual numbers 001-046 (printed p.7, PDF page 8), re-verified the same
/// two-method way: `pdftotext -layout` first, then the rendered page image
/// read directly and compared column-by-column — the two extractions
/// agreed on every row in this range (no OCR misreads like the first
/// sub-batch's item 072, though one **was** caught immediately adjacent to
/// this range: `pdftotext -layout`'s first pass swapped items 068/069's
/// Digits column (`068 DATA HCUT FREQ` read back as Digits=1, `069 DATA
/// HCUT SLOPE` as Digits=2 — backwards vs. every sibling `*HCUT FREQ`/
/// `*HCUT SLOPE` pair, which are always 2/1). The page-image read confirmed
/// the correct 2/1 order; both items are in the *next* sub-batch's range
/// (049-079), not this one, but the misread is noted here since it was
/// caught during this task's verification pass).
///
/// | P1  | Name | P2 | Digits |
/// |-----|------|----|--------|
/// | 001 | AGC FAST DELAY | `0020~4000 msec, 20 msec/step` | 4 |
/// | 002 | AGC MID DELAY | same shape as 001 | 4 |
/// | 003 | AGC SLOW DELAY | same shape as 001 | 4 |
/// | 004 | HOME FUNCTION | `0:SCOPE 1:FUNCTION` | 1 |
/// | 005 | MY CALL INDICATION | `0~5 sec` | 1 |
/// | 006 | DISPLAY COLOR | `0:BLUE 1:GRAY 2:GREEN 3:ORANGE 4:PURPLE 5:RED 6:SKY BLUE` | 1 |
/// | 007 | DIMMER LED | `0:1 1:2` | 1 |
/// | 008 | DIMMER TFT | `00~15` | 2 |
/// | 009 | BAR MTR PEAK HOLD | `0:OFF 1:0.5sec 2:1.0sec 3:2.0sec` | 1 |
/// | 010 | DVS RX OUT LEVEL | `000~100` | 3 |
/// | 011 | DVS TX OUT LEVEL | `000~100` | 3 |
/// | 012 | KEYER TYPE | `0:OFF 1:BUG 2:ELEKEY-A 3:ELEKEY-B 4:ELEKEY-Y 5:ACS` | 1 |
/// | 013 | KEYER DOT/DASH | `0:NORMAL 1:REVERSE` | 1 |
/// | 014 | CW WEIGHT | `2.5~4.5 (P2=25~45)` | 2 |
/// | 015 | BEACON INTERVAL | `OFF/1~690 sec (P2=000~690, 000:OFF)` | 3 |
/// | 016 | NUMBER STYLE | `0:1290 1:AUNO 2:AUNT 3:A2NO 4:A2NT 5:12NO 6:12NT` | 1 |
/// | 017 | CONTEST NUMBER | `0000~9999` | 4 |
/// | 018-022 | CW MEMORY 1-5 | `0:TEXT 1:MESSAGE` | 1 |
/// | 023 | NB WIDTH | `0:1ms 1:3ms 2:10ms` | 1 |
/// | 024 | NB REJECTION | `0:10dB 1:30dB 2:50dB` | 1 |
/// | 025 | NB LEVEL | `00~10` | 2 |
/// | 026 | BEEP LEVEL | `000~100` | 3 |
/// | 027 | TIME ZONE | `UTC -12:00~+14:00` | 5 — **skipped, see below** |
/// | 028 | GPS/232C SELECT | `0:GPS1 1:GPS2 3:RS232C` (gap at 2) | 1 |
/// | 029 | 232C RATE | `0:4800bps 1:9600bps 2:19200bps 3:38400bps` | 1 |
/// | 030 | 232C TOT | `0:10msec 1:100msec 2:1000msec 3:3000msec` | 1 |
/// | 031 | CAT RATE | same shape as 029 | 1 |
/// | 032 | CAT TOT | same shape as 030 | 1 |
/// | 033 | CAT RTS | `0:DISABLE 1:ENABLE` | 1 |
/// | 034 | MEM GROUP | `0:DISABLE 1:ENABLE` | 1 |
/// | 035 | QUICK SPLIT FREQ | `-20~+00(or -00)~+20 kHz` | 3 |
/// | 036 | TX TOT | `0(OFF)~30 min` | 2 |
/// | 037 | MIC SCAN | `0:DISABLE 1:ENABLE` | 1 |
/// | 038 | MIC SCAN RESUME | `0:PAUSE 1:TIME` | 1 |
/// | 039 | REF FREQ ADJ | `-25~+00(or -00)~+25` | 3 |
/// | 040 | CLAR MODE SELECT | `0:RX 1:TX 2:TRX` | 1 |
/// | 041 | AM LCUT FREQ | `00:OFF 01:100Hz~19:1000Hz (50Hz steps)` | 2 |
/// | 042 | AM LCUT SLOPE | `0:6dB/oct 1:18dB/oct` | 1 |
/// | 043 | AM HCUT FREQ | `00:OFF 01:700Hz~67:4000Hz (50Hz steps)` | 2 |
/// | 044 | AM HCUT SLOPE | `0:6dB/oct 1:18dB/oct` | 1 |
/// | 045 | AM MIC SELECT | `0:MIC 1:REAR` | 1 |
/// | 046 | AM OUT LEVEL | `000~100` | 3 |
///
/// All widths in this sub-batch (1, 2, 3, 4) were already present in
/// [`EX_SET_FORMS`] (built ahead-of-need by the first sub-batch) — no new
/// `CommandForm` width was needed.
///
/// **027 "TIME ZONE" explicitly skipped, not guessed** — same treatment as
/// item 087 "RADIO ID". Unlike every other range-valued item in this
/// sub-batch, 027's P2 cell gives only the display-unit range (`UTC -12:00
/// ~ +14:00`) with **no** accompanying `"(P2 = ...)"` wire-encoding formula
/// — every sibling signed-range item (035, 039) *does* state its formula
/// explicitly. Real-world UTC offsets are not uniformly stepped across
/// this range either (e.g. `+05:30`, `+05:45`, `+12:45` are real zones at
/// non-15/30/60-minute-uniform points), so a guessed "sign + HHMM, N-minute
/// step" encoding risks being wrong in a way this manual alone cannot
/// resolve. Left for a future sub-batch/hardware verification.
///
/// **Value-encoding judgment calls, not manual-cited formulas**: items
/// without an explicit named `X: LABEL` legend for every value (005 "MY
/// CALL INDICATION", 008 "DIMMER TFT", 010/011 "DVS RX/TX OUT LEVEL", 014
/// "CW WEIGHT", 015 "BEACON INTERVAL", 017 "CONTEST NUMBER", 025 "NB
/// LEVEL", 026 "BEEP LEVEL", 035 "QUICK SPLIT FREQ", 036 "TX TOT", 039 "REF
/// FREQ ADJ", 041/043 "AM LCUT/HCUT FREQ", 046 "AM OUT LEVEL") are modeled
/// as [`ExMenuValueKind::Range`] rather than [`ExMenuValueKind::Enumerated`]
/// — a plain numeric quantity, not a named pick-list — with `step` taken
/// from the manual's own "N msec/step"/"N Hz steps" wording where stated,
/// else `1` where the manual gives only a bare `min ~ max` with no step
/// note (005, 008, 010, 011, 014, 015, 017, 025, 026, 035, 036, 039, 041,
/// 043, 046) — a documented assumption, not a manual-stated fact for those
/// specific items.
///
/// **Default-value policy, an explicit implementation choice — the manual
/// states no factory default for any item in this table**: `Enumerated`
/// items default to their legend's first listed value; unsigned `Range`
/// items default to `min`; signed `Range` items (035, 039) default to `0`
/// (itself a legal, manual-shown value — `"+00"`/`"-00"` — and the natural
/// "no adjustment" neutral point, not an arbitrary endpoint). Same category
/// of open item as `meter_select`'s default (batch 9) and the first `EX`
/// sub-batch's PTT-select defaults.
///
/// # Third sub-batch (26 items): 049-079, minus 060/071/072/076/077 (already landed)
///
/// Manual numbers 049-079 (printed p.7-8, PDF page 8-9). See module docs'
/// "EX menu, third sub-batch" section for the full 068/069 digit-width
/// resolution (definitively re-verified against a fresh 300 DPI page
/// render, superseding what the prior sub-batch's findings claimed).
///
/// | P1  | Name | P2 | Digits |
/// |-----|------|----|--------|
/// | 049 | AM DATA GAIN | `0~100 (P2=000~100)` | 3 |
/// | 050 | CW LCUT FREQ | `00:OFF 01:100Hz~19:1000Hz (50Hz steps)` | 2 |
/// | 051 | CW LCUT SLOPE | `0:6dB/oct 1:18dB/oct` | 1 |
/// | 052 | CW HCUT FREQ | `00:OFF 01:700Hz~67:4000Hz (50Hz steps)` | 2 |
/// | 053 | CW HCUT SLOPE | `0:6dB/oct 1:18dB/oct` | 1 |
/// | 054 | CW OUT LEVEL | `0~100 (P2=000~100)` | 3 |
/// | 055 | CW AUTO MODE | `0:OFF 1:50MHz 2:ON` | 1 |
/// | 056 | CW BK-IN TYPE | `0:SEMI BREAK-IN 1:FULL BREAK-IN` | 1 |
/// | 057 | CW BK-IN DELAY | `30~3000 msec (P2=0030~3000, 10 msec/step)` | 4 |
/// | 058 | CW WAVE SHAPE | `0:1msec 1:2msec 2:4msec 3:6msec` | 1 |
/// | 059 | CW FREQ DISPLAY | `0:DIRECT FREQ 1:PITCH OFFSET` | 1 |
/// | 061 | QSK DELAY TIME | `0:15msec 1:20msec 2:25msec 3:30msec` | 1 |
/// | 062 | DATA MODE | `0:PSK 1:OTHER` | 1 |
/// | 063 | PSK TONE | `0:1000Hz 1:1500Hz 2:2000Hz` | 1 |
/// | 064 | OTHER DISP (SSB) | `-3000~0~+3000 Hz (P2=-3000~-0000 or +0000~+3000, 10Hz steps)` | 5 |
/// | 065 | OTHER SHIFT (SSB) | same shape as 064 | 5 |
/// | 066 | DATA LCUT FREQ | `00:OFF 01:100Hz~19:1000Hz (50Hz steps)` | 2 |
/// | 067 | DATA LCUT SLOPE | `0:6dB/oct 1:18dB/oct` | 1 |
/// | 068 | DATA HCUT FREQ | `00:OFF 01:700Hz~67:4000Hz (50Hz steps)` | 2 — **see resolution note above** |
/// | 069 | DATA HCUT SLOPE | `0:6dB/oct 1:18dB/oct` | 1 — **see resolution note above** |
/// | 070 | DATA IN SELECT | `0:MIC 1:REAR` | 1 |
/// | 073 | DATA OUT LEVEL | `0~100 (P2=000~100)` | 3 |
/// | 074 | FM MIC SELECT | `0:MIC 1:REAR` | 1 |
/// | 075 | FM OUT LEVEL | `0~100 (P2=000~100)` | 3 |
/// | 078 | FM PKT TX GAIN | `0~100 (P2=000~100)` | 3 |
/// | 079 | FM PKT MODE | `0:1200 1:9600` | 1 |
///
/// `060` "PC KEYING", `071`/`072` "DATA PTT/PORT SELECT", `076`/`077`
/// "FM PKT PTT/PORT SELECT" fall inside 049-079 but were already landed by
/// the first sub-batch — not re-added, no duplicate `EX_MENU_TABLE` rows.
///
/// Default-value policy, same category of open item as the second
/// sub-batch's (manual states no factory default for any item here):
/// `Enumerated` items default to their legend's first listed value;
/// unsigned `Range` items default to `min` (057 "CW BK-IN DELAY" defaults
/// to `30`, its `min`, not `0` — `0` is not a legal value for that item);
/// signed `Range` items (064, 065) default to `0`, the neutral "no offset"
/// point, same treatment as 035/039.
///
/// # Fourth sub-batch (71 items): 080-153, minus 087 (skipped) and 108/109 (already landed)
///
/// Manual numbers 080-153 (printed p.8-9, PDF p.9-10). See module docs'
/// "EX menu, fourth sub-batch" section for the full 100/116/147 resolution
/// notes.
///
/// | P1  | Name | P2 | Digits |
/// |-----|------|----|--------|
/// | 080 | RPT SHIFT 28MHz | `0~1000 kHz (P2=0000~1000, 10kHz/step)` | 4 |
/// | 081 | RPT SHIFT 50MHz | `0~4000 kHz (P2=0000~4000, 10kHz/step)` | 4 |
/// | 082 | RPT SHIFT 144MHz | `0~4000 kHz (P2=0000~4000, 10kHz/step)` | 4 |
/// | 083 | RPT SHIFT 430MHz | `0~10000 kHz (P2=0000~10000, 10kHz/step)` | 5 |
/// | 084 | ARS 144MHz | `0:OFF 1:ON` | 1 |
/// | 085 | ARS 430MHz | `0:OFF 1:ON` | 1 |
/// | 086 | DCS POLARITY | `0:Tn-Rn 1:Tn-Riv 2:Tiv-Rn 3:Tiv-Riv` | 1 |
/// | 088 | GM DISPLY | `0:DISTANCE 1:STRENGTH` | 1 |
/// | 089 | DISTANCE | `0:km 1:mile` | 1 |
/// | 090 | AMS TX MODE | `0:AUTO 1:MANUAL 2:DN 3:VW 4:ANALOG` | 1 |
/// | 091 | STANDBY BEEP | `0:OFF 1:ON` | 1 |
/// | 092 | RTTY LCUT FREQ | `00:OFF 01:100Hz~19:1000Hz (50Hz steps)` | 2 |
/// | 093 | RTTY LCUT SLOPE | `0:6dB/oct 1:18dB/oct` | 1 |
/// | 094 | RTTY HCUT FREQ | `00:OFF 01:700Hz~67:4000Hz (50Hz steps)` | 2 |
/// | 095 | RTTY HCUT SLOPE | `0:6dB/oct 1:18dB/oct` | 1 |
/// | 096 | RTTY SHIFT PORT | `0:SHIFT 1:DTR 2:RTS` | 1 |
/// | 097 | RTTY POLARITY-RX | `0:NORMAL 1:REVERSE` | 1 |
/// | 098 | RTTY POLARITY-TX | `0:NORMAL 1:REVERSE` | 1 |
/// | 099 | RTTY OUT LEVEL | `0~100 (P2=000~100)` | 3 |
/// | 100 | RTTY SHIFT FREQ | `0:170Hz 1:200Hz 2:425Hz 3:850Hz` — **see resolution note above (manual's own duplicate "1:" typo)** | 1 |
/// | 101 | RTTY MARK FREQ | `1:1275Hz 2:2125Hz` | 1 |
/// | 102 | SSB LCUT FREQ | `00:OFF 01:100Hz~19:1000Hz (50Hz steps)` | 2 |
/// | 103 | SSB LCUT SLOPE | `0:6dB/oct 1:18dB/oct` | 1 |
/// | 104 | SSB HCUT FREQ | `00:OFF 01:700Hz~67:4000Hz (50Hz steps)` | 2 |
/// | 105 | SSB HCUT SLOPE | `0:6dB/oct 1:18dB/oct` | 1 |
/// | 106 | SSB MIC SELECT | `0:MIC 1:REAR` | 1 |
/// | 107 | SSB OUT LEVEL | `0~100 (P2=000~100)` | 3 |
/// | 110 | SSB TX BPF | `0:50~3000 1:100~2900 2:200~2800 3:300~2700 4:400~2600` | 1 |
/// | 111 | APF WIDTH | `0:NARROW 1:MEDIUM 2:WIDE` | 1 |
/// | 112 | CONTOUR LEVEL | `-40~0~+20 (P2=-40~-00 or +00~+20)` | 3 |
/// | 113 | CONTOUR WIDTH | `01~10` | 2 |
/// | 114 | IF NOTCH WIDTH | `0:NARROW 1:WIDE` | 1 |
/// | 115 | SCP DISPLAY MODE | `0:SPECTRUM 1:WATER FALL` | 1 |
/// | 116 | SCP SPAN FREQ | `03:50kHz 04:100kHz 05:200kHz 06:500kHz 07:1000kHz` — **documented gap at 00-02, see module docs** | 2 |
/// | 117 | SPECTRUM COLOR | `0:BLUE 1:GRAY 2:GREEN 3:ORANGE 4:PURPLE 5:RED 6:SKY BLUE` | 1 |
/// | 118 | WATER FALL COLOR | `0:BLUE 1:GRAY 2:GREEN 3:ORANGE 4:PURPLE 5:RED 6:SKY BLUE 7:MULTI` | 1 |
/// | 119 | PRMTRC EQ1 FREQ | `00:OFF 01:100~07:700 Hz` | 2 |
/// | 120 | PRMTRC EQ1 LEVEL | `-20~0~+10 (P2=-20~-00 or +00~+10)` | 3 |
/// | 121 | PRMTRC EQ1 BWTH | `01~10` | 2 |
/// | 122 | PRMTRC EQ2 FREQ | `00:OFF 01:700~09:1500 Hz` | 2 |
/// | 123 | PRMTRC EQ2 LEVEL | same shape as 120 | 3 |
/// | 124 | PRMTRC EQ2 BWTH | `01~10` | 2 |
/// | 125 | PRMTRC EQ3 FREQ | `00:OFF 01:1500~18:3200 Hz` | 2 |
/// | 126 | PRMTRC EQ3 LEVEL | same shape as 120 | 3 |
/// | 127 | PRMTRC EQ3 BWTH | `01~10` | 2 |
/// | 128 | P-PRMTRC EQ1 FREQ | same shape as 119 | 2 |
/// | 129 | P-PRMTRC EQ1 LEVEL | same shape as 120 | 3 |
/// | 130 | P-PRMTRC EQ1 BWTH | `01~10` | 2 |
/// | 131 | P-PRMTRC EQ2 FREQ | same shape as 122 | 2 |
/// | 132 | P-PRMTRC EQ2 LEVEL | same shape as 120 | 3 |
/// | 133 | P-PRMTRC EQ2 BWTH | `01~10` | 2 |
/// | 134 | P-PRMTRC EQ3 FREQ | same shape as 125 | 2 |
/// | 135 | P-PRMTRC EQ3 LEVEL | same shape as 120 | 3 |
/// | 136 | P-PRMTRC EQ3 BWTH | `01~10` | 2 |
/// | 137 | HF TX MAX POWER | `5~100 (P2=005~100)` | 3 |
/// | 138 | 50M TX MAX POWER | `5~100 (P2=005~100)` | 3 |
/// | 139 | 144M TX MAX POWER | `5~50 (P2=005~050)` | 3 |
/// | 140 | 430M TX MAX POWER | `5~50 (P2=005~050)` | 3 |
/// | 141 | TUNER SELECT | `0:OFF 1:INTERNAL 2:EXTERNAL 3:ATAS 4:LAMP` | 1 |
/// | 142 | VOX SELECT | `0:MIC 1:DATA` | 1 |
/// | 143 | VOX GAIN | `000~100` | 3 |
/// | 144 | VOX DELAY | `30~3000 msec (P2=0030~3000, 10msec/step)` | 4 |
/// | 145 | ANTI VOX GAIN | `000~100` | 3 |
/// | 146 | DATA VOX GAIN | `000~100` | 3 |
/// | 147 | DATA VOX DELAY | `30~3000 msec (P2=0030~3000)` — **step assumed 10msec by corroboration with sibling 144, see module docs** | 4 |
/// | 148 | ANTI DVOX GAIN | `000~100` | 3 |
/// | 149 | EMERGENCY FREQ TX | `0:DISABLE 1:ENABLE` | 1 |
/// | 150 | PRT/WIRES FREQ | `0:MANUAL 1:PRESET` | 1 |
/// | 151 | PRESET FREQUENCY | `00030000~47000000` | 8 |
/// | 152 | SEARCH SETUP | `0:HISTORY 1:ACTIVITY` | 1 |
/// | 153 | WIRES DG-ID | `00:AUTO 01:DG-ID01~99:DG-ID99` | 2 |
///
/// Default-value policy, same category of open item as prior sub-batches
/// (manual states no factory default for any item here): `Enumerated`
/// items default to their legend's first listed value (101 "RTTY MARK
/// FREQ" therefore defaults to `1`, its first listed value, not `0` — `0`
/// is not legal for this item, same treatment as 072/077/109's 1-based
/// lists); unsigned `Range` items default to `min` (113/121/124/127/130/
/// 133/136 "*BWTH" items default to `1`; 137-140 "*TX MAX POWER" items
/// default to `5`; 144/147 "*VOX DELAY" items default to `30`); signed
/// `Range` items (112, 120, 123, 126, 129, 132, 135) default to `0`, the
/// neutral "no adjustment" point, same treatment as 035/039/064/065.
pub static EX_MENU_TABLE: &[ExMenuItem] = &[
    ExMenuItem {
        p1: 1,
        name: "AGC FAST DELAY",
        digits: 4,
        kind: ExMenuValueKind::Range {
            min: 20,
            max: 4000,
            step: 20,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 2,
        name: "AGC MID DELAY",
        digits: 4,
        kind: ExMenuValueKind::Range {
            min: 20,
            max: 4000,
            step: 20,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 3,
        name: "AGC SLOW DELAY",
        digits: 4,
        kind: ExMenuValueKind::Range {
            min: 20,
            max: 4000,
            step: 20,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 4,
        name: "HOME FUNCTION",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "SCOPE"), ("1", "FUNCTION")]),
    },
    ExMenuItem {
        p1: 5,
        name: "MY CALL INDICATION",
        digits: 1,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 5,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 6,
        name: "DISPLAY COLOR",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "BLUE"),
            ("1", "GRAY"),
            ("2", "GREEN"),
            ("3", "ORANGE"),
            ("4", "PURPLE"),
            ("5", "RED"),
            ("6", "SKY BLUE"),
        ]),
    },
    ExMenuItem {
        p1: 7,
        name: "DIMMER LED",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "1"), ("1", "2")]),
    },
    ExMenuItem {
        p1: 8,
        name: "DIMMER TFT",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 15,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 9,
        name: "BAR MTR PEAK HOLD",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "OFF"),
            ("1", "0.5sec"),
            ("2", "1.0sec"),
            ("3", "2.0sec"),
        ]),
    },
    ExMenuItem {
        p1: 10,
        name: "DVS RX OUT LEVEL",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 100,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 11,
        name: "DVS TX OUT LEVEL",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 100,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 12,
        name: "KEYER TYPE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "OFF"),
            ("1", "BUG"),
            ("2", "ELEKEY-A"),
            ("3", "ELEKEY-B"),
            ("4", "ELEKEY-Y"),
            ("5", "ACS"),
        ]),
    },
    ExMenuItem {
        p1: 13,
        name: "KEYER DOT/DASH",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "NORMAL"), ("1", "REVERSE")]),
    },
    ExMenuItem {
        p1: 14,
        name: "CW WEIGHT",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 25,
            max: 45,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 15,
        name: "BEACON INTERVAL",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 690,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 16,
        name: "NUMBER STYLE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "1290"),
            ("1", "AUNO"),
            ("2", "AUNT"),
            ("3", "A2NO"),
            ("4", "A2NT"),
            ("5", "12NO"),
            ("6", "12NT"),
        ]),
    },
    ExMenuItem {
        p1: 17,
        name: "CONTEST NUMBER",
        digits: 4,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 9999,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 18,
        name: "CW MEMORY 1",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "TEXT"), ("1", "MESSAGE")]),
    },
    ExMenuItem {
        p1: 19,
        name: "CW MEMORY 2",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "TEXT"), ("1", "MESSAGE")]),
    },
    ExMenuItem {
        p1: 20,
        name: "CW MEMORY 3",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "TEXT"), ("1", "MESSAGE")]),
    },
    ExMenuItem {
        p1: 21,
        name: "CW MEMORY 4",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "TEXT"), ("1", "MESSAGE")]),
    },
    ExMenuItem {
        p1: 22,
        name: "CW MEMORY 5",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "TEXT"), ("1", "MESSAGE")]),
    },
    ExMenuItem {
        p1: 23,
        name: "NB WIDTH",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "1ms"), ("1", "3ms"), ("2", "10ms")]),
    },
    ExMenuItem {
        p1: 24,
        name: "NB REJECTION",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "10dB"), ("1", "30dB"), ("2", "50dB")]),
    },
    ExMenuItem {
        p1: 25,
        name: "NB LEVEL",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 10,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 26,
        name: "BEEP LEVEL",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 100,
            step: 1,
            signed: false,
        },
    },
    // 027 "TIME ZONE" deliberately absent — see doc comment above.
    ExMenuItem {
        p1: 28,
        name: "GPS/232C SELECT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "GPS1"), ("1", "GPS2"), ("3", "RS232C")]),
    },
    ExMenuItem {
        p1: 29,
        name: "232C RATE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "4800bps"),
            ("1", "9600bps"),
            ("2", "19200bps"),
            ("3", "38400bps"),
        ]),
    },
    ExMenuItem {
        p1: 30,
        name: "232C TOT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "10msec"),
            ("1", "100msec"),
            ("2", "1000msec"),
            ("3", "3000msec"),
        ]),
    },
    ExMenuItem {
        p1: 31,
        name: "CAT RATE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "4800bps"),
            ("1", "9600bps"),
            ("2", "19200bps"),
            ("3", "38400bps"),
        ]),
    },
    ExMenuItem {
        p1: 32,
        name: "CAT TOT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "10msec"),
            ("1", "100msec"),
            ("2", "1000msec"),
            ("3", "3000msec"),
        ]),
    },
    ExMenuItem {
        p1: 33,
        name: "CAT RTS",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "DISABLE"), ("1", "ENABLE")]),
    },
    ExMenuItem {
        p1: 34,
        name: "MEM GROUP",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "DISABLE"), ("1", "ENABLE")]),
    },
    ExMenuItem {
        p1: 35,
        name: "QUICK SPLIT FREQ",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: -20,
            max: 20,
            step: 1,
            signed: true,
        },
    },
    ExMenuItem {
        p1: 36,
        name: "TX TOT",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 30,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 37,
        name: "MIC SCAN",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "DISABLE"), ("1", "ENABLE")]),
    },
    ExMenuItem {
        p1: 38,
        name: "MIC SCAN RESUME",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "PAUSE"), ("1", "TIME")]),
    },
    ExMenuItem {
        p1: 39,
        name: "REF FREQ ADJ",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: -25,
            max: 25,
            step: 1,
            signed: true,
        },
    },
    ExMenuItem {
        p1: 40,
        name: "CLAR MODE SELECT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "RX"), ("1", "TX"), ("2", "TRX")]),
    },
    ExMenuItem {
        p1: 41,
        name: "AM LCUT FREQ",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 19,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 42,
        name: "AM LCUT SLOPE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "6dB/oct"), ("1", "18dB/oct")]),
    },
    ExMenuItem {
        p1: 43,
        name: "AM HCUT FREQ",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 67,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 44,
        name: "AM HCUT SLOPE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "6dB/oct"), ("1", "18dB/oct")]),
    },
    ExMenuItem {
        p1: 45,
        name: "AM MIC SELECT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "MIC"), ("1", "REAR")]),
    },
    ExMenuItem {
        p1: 46,
        name: "AM OUT LEVEL",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 100,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 47,
        name: "AM PTT SELECT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "DAKY"), ("1", "RTS"), ("2", "DTR")]),
    },
    ExMenuItem {
        p1: 48,
        name: "AM PORT SELECT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "DATA"), ("1", "USB")]),
    },
    ExMenuItem {
        p1: 49,
        name: "AM DATA GAIN",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 100,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 50,
        name: "CW LCUT FREQ",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 19,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 51,
        name: "CW LCUT SLOPE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "6dB/oct"), ("1", "18dB/oct")]),
    },
    ExMenuItem {
        p1: 52,
        name: "CW HCUT FREQ",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 67,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 53,
        name: "CW HCUT SLOPE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "6dB/oct"), ("1", "18dB/oct")]),
    },
    ExMenuItem {
        p1: 54,
        name: "CW OUT LEVEL",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 100,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 55,
        name: "CW AUTO MODE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "OFF"), ("1", "50MHz"), ("2", "ON")]),
    },
    ExMenuItem {
        p1: 56,
        name: "CW BK-IN TYPE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "SEMI BREAK-IN"), ("1", "FULL BREAK-IN")]),
    },
    ExMenuItem {
        p1: 57,
        name: "CW BK-IN DELAY",
        digits: 4,
        kind: ExMenuValueKind::Range {
            min: 30,
            max: 3000,
            step: 10,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 58,
        name: "CW WAVE SHAPE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "1msec"),
            ("1", "2msec"),
            ("2", "4msec"),
            ("3", "6msec"),
        ]),
    },
    ExMenuItem {
        p1: 59,
        name: "CW FREQ DISPLAY",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "DIRECT FREQ"), ("1", "PITCH OFFSET")]),
    },
    ExMenuItem {
        p1: 60,
        name: "PC KEYING",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "OFF"),
            ("1", "DAKY"),
            ("2", "RTS"),
            ("3", "DTR"),
        ]),
    },
    ExMenuItem {
        p1: 61,
        name: "QSK DELAY TIME",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "15msec"),
            ("1", "20msec"),
            ("2", "25msec"),
            ("3", "30msec"),
        ]),
    },
    ExMenuItem {
        p1: 62,
        name: "DATA MODE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "PSK"), ("1", "OTHER")]),
    },
    ExMenuItem {
        p1: 63,
        name: "PSK TONE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "1000Hz"), ("1", "1500Hz"), ("2", "2000Hz")]),
    },
    ExMenuItem {
        p1: 64,
        name: "OTHER DISP (SSB)",
        digits: 5,
        kind: ExMenuValueKind::Range {
            min: -3000,
            max: 3000,
            step: 10,
            signed: true,
        },
    },
    ExMenuItem {
        p1: 65,
        name: "OTHER SHIFT (SSB)",
        digits: 5,
        kind: ExMenuValueKind::Range {
            min: -3000,
            max: 3000,
            step: 10,
            signed: true,
        },
    },
    ExMenuItem {
        p1: 66,
        name: "DATA LCUT FREQ",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 19,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 67,
        name: "DATA LCUT SLOPE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "6dB/oct"), ("1", "18dB/oct")]),
    },
    // 068/069: manual's own printed Digits column literally shows 1/2, but
    // that is functionally impossible for 068 (legend needs values up to
    // 67, i.e. 2 digits) and contradicts every sibling *HCUT FREQ/SLOPE
    // pair on this page (always 2/1) — implemented as the
    // functionally-necessary, sibling-corroborated 2/1 order. See module
    // docs' "EX menu, third sub-batch" section and EX_MENU_TABLE's doc
    // comment for the full resolution.
    ExMenuItem {
        p1: 68,
        name: "DATA HCUT FREQ",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 67,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 69,
        name: "DATA HCUT SLOPE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "6dB/oct"), ("1", "18dB/oct")]),
    },
    ExMenuItem {
        p1: 70,
        name: "DATA IN SELECT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "MIC"), ("1", "REAR")]),
    },
    ExMenuItem {
        p1: 71,
        name: "DATA PTT SELECT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "DAKY"), ("1", "RTS"), ("2", "DTR")]),
    },
    ExMenuItem {
        p1: 72,
        name: "DATA PORT SELECT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("1", "DATA"), ("2", "USB")]),
    },
    ExMenuItem {
        p1: 73,
        name: "DATA OUT LEVEL",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 100,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 74,
        name: "FM MIC SELECT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "MIC"), ("1", "REAR")]),
    },
    ExMenuItem {
        p1: 75,
        name: "FM OUT LEVEL",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 100,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 76,
        name: "FM PKT PTT SELECT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "DAKY"), ("1", "RTS"), ("2", "DTR")]),
    },
    ExMenuItem {
        p1: 77,
        name: "FM PKT PORT SELECT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("1", "DATA"), ("2", "USB")]),
    },
    ExMenuItem {
        p1: 78,
        name: "FM PKT TX GAIN",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 100,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 79,
        name: "FM PKT MODE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "1200"), ("1", "9600")]),
    },
    // -- Fourth sub-batch: items 080-153, minus 087 (skipped) and 108/109
    // (already landed above) -----------------------------------------
    ExMenuItem {
        p1: 80,
        name: "RPT SHIFT 28MHz",
        digits: 4,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 1000,
            step: 10,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 81,
        name: "RPT SHIFT 50MHz",
        digits: 4,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 4000,
            step: 10,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 82,
        name: "RPT SHIFT 144MHz",
        digits: 4,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 4000,
            step: 10,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 83,
        name: "RPT SHIFT 430MHz",
        digits: 5,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 10000,
            step: 10,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 84,
        name: "ARS 144MHz",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "OFF"), ("1", "ON")]),
    },
    ExMenuItem {
        p1: 85,
        name: "ARS 430MHz",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "OFF"), ("1", "ON")]),
    },
    ExMenuItem {
        p1: 86,
        name: "DCS POLARITY",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "Tn-Rn"),
            ("1", "Tn-Riv"),
            ("2", "Tiv-Rn"),
            ("3", "Tiv-Riv"),
        ]),
    },
    // 087 "RADIO ID" permanently skipped — see module docs/findings.md.
    ExMenuItem {
        p1: 88,
        name: "GM DISPLY",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "DISTANCE"), ("1", "STRENGTH")]),
    },
    ExMenuItem {
        p1: 89,
        name: "DISTANCE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "km"), ("1", "mile")]),
    },
    ExMenuItem {
        p1: 90,
        name: "AMS TX MODE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "AUTO"),
            ("1", "MANUAL"),
            ("2", "DN"),
            ("3", "VW"),
            ("4", "ANALOG"),
        ]),
    },
    ExMenuItem {
        p1: 91,
        name: "STANDBY BEEP",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "OFF"), ("1", "ON")]),
    },
    ExMenuItem {
        p1: 92,
        name: "RTTY LCUT FREQ",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 19,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 93,
        name: "RTTY LCUT SLOPE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "6dB/oct"), ("1", "18dB/oct")]),
    },
    ExMenuItem {
        p1: 94,
        name: "RTTY HCUT FREQ",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 67,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 95,
        name: "RTTY HCUT SLOPE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "6dB/oct"), ("1", "18dB/oct")]),
    },
    ExMenuItem {
        p1: 96,
        name: "RTTY SHIFT PORT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "SHIFT"), ("1", "DTR"), ("2", "RTS")]),
    },
    ExMenuItem {
        p1: 97,
        name: "RTTY POLARITY-RX",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "NORMAL"), ("1", "REVERSE")]),
    },
    ExMenuItem {
        p1: 98,
        name: "RTTY POLARITY-TX",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "NORMAL"), ("1", "REVERSE")]),
    },
    ExMenuItem {
        p1: 99,
        name: "RTTY OUT LEVEL",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 100,
            step: 1,
            signed: false,
        },
    },
    // 100 "RTTY SHIFT FREQ": manual prints a duplicate "1:" label (see
    // module docs' resolution note) — resolved to 0-based via corroborating
    // evidence, not a guess.
    ExMenuItem {
        p1: 100,
        name: "RTTY SHIFT FREQ",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "170Hz"),
            ("1", "200Hz"),
            ("2", "425Hz"),
            ("3", "850Hz"),
        ]),
    },
    ExMenuItem {
        p1: 101,
        name: "RTTY MARK FREQ",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("1", "1275Hz"), ("2", "2125Hz")]),
    },
    ExMenuItem {
        p1: 102,
        name: "SSB LCUT FREQ",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 19,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 103,
        name: "SSB LCUT SLOPE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "6dB/oct"), ("1", "18dB/oct")]),
    },
    ExMenuItem {
        p1: 104,
        name: "SSB HCUT FREQ",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 67,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 105,
        name: "SSB HCUT SLOPE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "6dB/oct"), ("1", "18dB/oct")]),
    },
    ExMenuItem {
        p1: 106,
        name: "SSB MIC SELECT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "MIC"), ("1", "REAR")]),
    },
    ExMenuItem {
        p1: 107,
        name: "SSB OUT LEVEL",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 100,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 108,
        name: "SSB PTT SELECT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "DAKY"), ("1", "RTS"), ("2", "DTR")]),
    },
    ExMenuItem {
        p1: 109,
        name: "SSB PORT SELECT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "DATA"), ("1", "USB")]),
    },
    ExMenuItem {
        p1: 110,
        name: "SSB TX BPF",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "50~3000"),
            ("1", "100~2900"),
            ("2", "200~2800"),
            ("3", "300~2700"),
            ("4", "400~2600"),
        ]),
    },
    ExMenuItem {
        p1: 111,
        name: "APF WIDTH",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "NARROW"), ("1", "MEDIUM"), ("2", "WIDE")]),
    },
    ExMenuItem {
        p1: 112,
        name: "CONTOUR LEVEL",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: -40,
            max: 20,
            step: 1,
            signed: true,
        },
    },
    ExMenuItem {
        p1: 113,
        name: "CONTOUR WIDTH",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 1,
            max: 10,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 114,
        name: "IF NOTCH WIDTH",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "NARROW"), ("1", "WIDE")]),
    },
    ExMenuItem {
        p1: 115,
        name: "SCP DISPLAY MODE",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "SPECTRUM"), ("1", "WATER FALL")]),
    },
    // 116 "SCP SPAN FREQ" has a documented gap: legal values are 03-07
    // only (00-02 absent from the manual) — see module docs.
    ExMenuItem {
        p1: 116,
        name: "SCP SPAN FREQ",
        digits: 2,
        kind: ExMenuValueKind::Enumerated(&[
            ("03", "50kHz"),
            ("04", "100kHz"),
            ("05", "200kHz"),
            ("06", "500kHz"),
            ("07", "1000kHz"),
        ]),
    },
    ExMenuItem {
        p1: 117,
        name: "SPECTRUM COLOR",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "BLUE"),
            ("1", "GRAY"),
            ("2", "GREEN"),
            ("3", "ORANGE"),
            ("4", "PURPLE"),
            ("5", "RED"),
            ("6", "SKY BLUE"),
        ]),
    },
    ExMenuItem {
        p1: 118,
        name: "WATER FALL COLOR",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "BLUE"),
            ("1", "GRAY"),
            ("2", "GREEN"),
            ("3", "ORANGE"),
            ("4", "PURPLE"),
            ("5", "RED"),
            ("6", "SKY BLUE"),
            ("7", "MULTI"),
        ]),
    },
    ExMenuItem {
        p1: 119,
        name: "PRMTRC EQ1 FREQ",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 7,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 120,
        name: "PRMTRC EQ1 LEVEL",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: -20,
            max: 10,
            step: 1,
            signed: true,
        },
    },
    ExMenuItem {
        p1: 121,
        name: "PRMTRC EQ1 BWTH",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 1,
            max: 10,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 122,
        name: "PRMTRC EQ2 FREQ",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 9,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 123,
        name: "PRMTRC EQ2 LEVEL",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: -20,
            max: 10,
            step: 1,
            signed: true,
        },
    },
    ExMenuItem {
        p1: 124,
        name: "PRMTRC EQ2 BWTH",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 1,
            max: 10,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 125,
        name: "PRMTRC EQ3 FREQ",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 18,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 126,
        name: "PRMTRC EQ3 LEVEL",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: -20,
            max: 10,
            step: 1,
            signed: true,
        },
    },
    ExMenuItem {
        p1: 127,
        name: "PRMTRC EQ3 BWTH",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 1,
            max: 10,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 128,
        name: "P-PRMTRC EQ1 FREQ",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 7,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 129,
        name: "P-PRMTRC EQ1 LEVEL",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: -20,
            max: 10,
            step: 1,
            signed: true,
        },
    },
    ExMenuItem {
        p1: 130,
        name: "P-PRMTRC EQ1 BWTH",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 1,
            max: 10,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 131,
        name: "P-PRMTRC EQ2 FREQ",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 9,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 132,
        name: "P-PRMTRC EQ2 LEVEL",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: -20,
            max: 10,
            step: 1,
            signed: true,
        },
    },
    ExMenuItem {
        p1: 133,
        name: "P-PRMTRC EQ2 BWTH",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 1,
            max: 10,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 134,
        name: "P-PRMTRC EQ3 FREQ",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 18,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 135,
        name: "P-PRMTRC EQ3 LEVEL",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: -20,
            max: 10,
            step: 1,
            signed: true,
        },
    },
    ExMenuItem {
        p1: 136,
        name: "P-PRMTRC EQ3 BWTH",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 1,
            max: 10,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 137,
        name: "HF TX MAX POWER",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 5,
            max: 100,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 138,
        name: "50M TX MAX POWER",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 5,
            max: 100,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 139,
        name: "144M TX MAX POWER",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 5,
            max: 50,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 140,
        name: "430M TX MAX POWER",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 5,
            max: 50,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 141,
        name: "TUNER SELECT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[
            ("0", "OFF"),
            ("1", "INTERNAL"),
            ("2", "EXTERNAL"),
            ("3", "ATAS"),
            ("4", "LAMP"),
        ]),
    },
    ExMenuItem {
        p1: 142,
        name: "VOX SELECT",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "MIC"), ("1", "DATA")]),
    },
    ExMenuItem {
        p1: 143,
        name: "VOX GAIN",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 100,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 144,
        name: "VOX DELAY",
        digits: 4,
        kind: ExMenuValueKind::Range {
            min: 30,
            max: 3000,
            step: 10,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 145,
        name: "ANTI VOX GAIN",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 100,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 146,
        name: "DATA VOX GAIN",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 100,
            step: 1,
            signed: false,
        },
    },
    // 147 "DATA VOX DELAY": step=10 assumed by corroboration with sibling
    // 144 "VOX DELAY" — see module docs' resolution note.
    ExMenuItem {
        p1: 147,
        name: "DATA VOX DELAY",
        digits: 4,
        kind: ExMenuValueKind::Range {
            min: 30,
            max: 3000,
            step: 10,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 148,
        name: "ANTI DVOX GAIN",
        digits: 3,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 100,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 149,
        name: "EMERGENCY FREQ TX",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "DISABLE"), ("1", "ENABLE")]),
    },
    ExMenuItem {
        p1: 150,
        name: "PRT/WIRES FREQ",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "MANUAL"), ("1", "PRESET")]),
    },
    ExMenuItem {
        p1: 151,
        name: "PRESET FREQUENCY",
        digits: 8,
        kind: ExMenuValueKind::Range {
            min: 30_000,
            max: 47_000_000,
            step: 1,
            signed: false,
        },
    },
    ExMenuItem {
        p1: 152,
        name: "SEARCH SETUP",
        digits: 1,
        kind: ExMenuValueKind::Enumerated(&[("0", "HISTORY"), ("1", "ACTIVITY")]),
    },
    ExMenuItem {
        p1: 153,
        name: "WIRES DG-ID",
        digits: 2,
        kind: ExMenuValueKind::Range {
            min: 0,
            max: 99,
            step: 1,
            signed: false,
        },
    },
];

/// Look up an `EX` menu item by its `P1` menu number. Returns `None` for
/// any not-yet-implemented item (or any out-of-range number entirely,
/// e.g. `999`) — `handle_command`'s `Ex` arm turns that into a clean
/// `"?;"` response, never a panic.
pub fn ex_menu_item(p1: u16) -> Option<&'static ExMenuItem> {
    EX_MENU_TABLE.iter().find(|item| item.p1 == p1)
}

/// The composite VFO/memory-channel status payload used by `IF`'s answer.
///
/// Per the architect's cross-batch finding
/// (`planning/architect/task_plan.md` §10.5, "Cross-batch finding" above
/// the batch table), this exact field sequence is shared by `IF` (this
/// batch), and — in later batches — `MR`/`MT` (batch 2, manual p.12) and
/// `OI` (batch 10, manual p.13), which all carry the identical trailing
/// P1-P10 block after their own command-specific selector/prefix. Factored
/// out here so those batches can reuse [`Self::parse`]/[`Self::to_wire_string`]
/// instead of re-deriving the same column boundaries independently.
///
/// # Column-by-column re-verification (this is the highest-risk part of
/// this task — Wave 1 deferred `IF` for exactly this reason)
///
/// Verified directly against the manual's page image (not extracted text
/// alone): `IF`, printed manual p.10 (PDF page 11 in this repo's
/// `docs/manuals/FT-991A_CAT_OM_ENG_1711-D.pdf`, given the documented "+1"
/// cover-page offset — see `planning/yaesu/task_plan.md`). The Answer row
/// spans 3 printed sub-rows of 10 columns each (1-10, 11-20, 21-30); column
/// 28 is the terminator `;`, columns 29-30 are unused. Parameter body
/// (after the 2-character `IF` code, before `;`) is therefore a **fixed 25
/// characters**, columns 3-27:
///
/// | Field | Width | Columns | Meaning |
/// |-------|-------|---------|---------|
/// | P1  | 3 | 3-5   | Memory channel, `001`-`117`. What appears here in VFO mode (`P7`=0) is not stated by the manual — this implementation accepts `000`-`117` and treats `0` as the VFO-mode sentinel (documented judgment call, not manual-specified). |
/// | P2  | 9 | 6-14  | VFO-A frequency, Hz, zero-padded (same shape as `FA`'s answer). |
/// | P3  | 5 | 15-19 | Clarifier: 1-char sign (`+`=Plus Shift, `-`=Minus Shift) + 4-digit offset, `0000`-`9999` Hz. Same shape as `IS`'s (IF-shift) P2 field on manual p.2's own worked example (`IS0+1000;`). |
/// | P4  | 1 | 20    | RX CLAR: `0`=OFF `1`=ON |
/// | P5  | 1 | 21    | TX CLAR: `0`=OFF `1`=ON |
/// | P6  | 1 | 22    | MODE, hex nibble `1`-`E` — same numeric encoding as `MD`'s P2/`Ft991aState::mode` (`IF`'s own legend labels values 3/7 "CW"/"CW-R" rather than `MD`'s "CW-U"/"CW-L" — a manual labeling inconsistency between the two tables, flagged not silently resolved; the *numeric* encoding is identical either way). |
/// | P7  | 1 | 23    | 0=VFO 1=Memory 2=Memory Tune 3=QMB 4=QMB-MT 5=PMS 6=HOME |
/// | P8  | 1 | 24    | 0=CTCSS OFF 1=CTCSS ENC/DEC 2=CTCSS ENC 3=DCS ENC/DEC 4=DCS ENC |
/// | P9  | 2 | 25-26 | Fixed `"00"` per the manual ("00: (Fixed)") — not stored as radio state; always written as `"00"`, and accepted but not strictly validated on parse (an unused reserved field). |
/// | P10 | 1 | 27    | 0=Simplex 1=Plus Shift 2=Minus Shift |
///
/// Total body width: `3+9+5+1+1+1+1+1+2+1 = 25`, matching
/// [`Self::WIRE_WIDTH`]. This resolves the ambiguous spot Wave 1 flagged
/// (the P2/P3 boundary and P9's fixed segment) — the boundaries above are
/// unambiguous once read from the column-numbered table directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelStatusFields {
    /// P1: memory channel (0-117; `0` is this implementation's VFO-mode
    /// sentinel, see struct docs).
    pub channel: u8,
    /// P2: VFO-A frequency, Hz.
    pub frequency_hz: u64,
    /// P3: combined sign+offset, range -9999..=9999 Hz.
    pub clarifier_offset_hz: i16,
    /// P4: RX CLAR on/off.
    pub rx_clarifier_on: bool,
    /// P5: TX CLAR on/off.
    pub tx_clarifier_on: bool,
    /// P6: raw hex-nibble mode value, matches `Ft991aState::mode`.
    pub mode: u8,
    /// P7: VFO/memory/QMB select, 0-6.
    pub select: u8,
    /// P8: CTCSS/DCS status, 0-4.
    pub tone_status: u8,
    /// P10: offset type, 0-2 (Simplex/Plus/Minus).
    pub offset_type: u8,
}

impl ChannelStatusFields {
    /// Fixed wire body width (P1..P10, P9 not stored — see struct docs).
    pub const WIRE_WIDTH: usize = 25;

    /// Parse a [`Self::WIRE_WIDTH`]-character body (the P1..P10 block,
    /// without the leading command code/selector or the trailing `;`) into
    /// typed fields. Returns `None` on any structural or range violation.
    pub fn parse(body: &str) -> Option<Self> {
        if body.len() != Self::WIRE_WIDTH {
            return None;
        }
        let channel: u8 = body.get(0..3)?.parse().ok().filter(|v| *v <= 117)?;
        let frequency_hz: u64 = body.get(3..12)?.parse().ok()?;
        let sign = body.get(12..13)?;
        let offset_digits: i16 = body.get(13..17)?.parse().ok()?;
        let clarifier_offset_hz = match sign {
            "+" => offset_digits,
            "-" => -offset_digits,
            _ => return None,
        };
        let rx_clarifier_on = parse_bit(body.get(17..18)?)?;
        let tx_clarifier_on = parse_bit(body.get(18..19)?)?;
        let mode = body.get(19..20)?.chars().next()?.to_digit(16)? as u8;
        if !(1..=0xE).contains(&mode) {
            return None;
        }
        let select: u8 = body.get(20..21)?.parse().ok().filter(|v| *v <= 6)?;
        let tone_status: u8 = body.get(21..22)?.parse().ok().filter(|v| *v <= 4)?;
        // P9 (body[22..24]) is the fixed "00" reserved field — deliberately
        // not extracted or validated, see struct docs.
        let offset_type: u8 = body.get(24..25)?.parse().ok().filter(|v| *v <= 2)?;

        Some(Self {
            channel,
            frequency_hz,
            clarifier_offset_hz,
            rx_clarifier_on,
            tx_clarifier_on,
            mode,
            select,
            tone_status,
            offset_type,
        })
    }

    /// Format back into the fixed [`Self::WIRE_WIDTH`]-character wire body
    /// (P9 always written as `"00"`).
    pub fn to_wire_string(&self) -> String {
        let sign = if self.clarifier_offset_hz < 0 {
            '-'
        } else {
            '+'
        };
        format!(
            "{:03}{:09}{sign}{:04}{}{}{:X}{}{}00{}",
            self.channel,
            self.frequency_hz,
            self.clarifier_offset_hz.abs(),
            u8::from(self.rx_clarifier_on),
            u8::from(self.tx_clarifier_on),
            self.mode,
            self.select,
            self.tone_status,
            self.offset_type,
        )
    }
}

/// One stored memory channel's contents (manual p.11-12, `MC`/`MR`/`MW`/
/// `MT`), held in `Ft991aState::memory_channels`, indexed `channel - 1`
/// (channels are numbered 1-117, no `0`).
///
/// Deliberately does **not** reuse [`ChannelStatusFields`] as the storage
/// representation: [`ChannelStatusFields`] also carries `channel` (which is
/// the array index here, not per-record state) and `select` (contextual —
/// always fixed on write, always reported `1` on read, see below — not a
/// value worth persisting per channel). The P1-P10 fields this struct DOES
/// share with [`ChannelStatusFields`] are round-tripped through a
/// [`ChannelStatusFields`] value at the wire boundary in
/// `Ft991aRadio::handle_command`'s `Mr`/`Mw`/`Mt` arms, reusing
/// [`ChannelStatusFields::parse`]/[`ChannelStatusFields::to_wire_string`]
/// rather than re-deriving the column layout.
///
/// # Manual re-verification (`MR`/`MW`/`MT`, printed p.12, PDF page 13 —
/// per the documented "+1" cover-page offset)
///
/// `MR` ("MEMORY CHANNEL READ") and `MW` ("MEMORY CHANNEL WRITE") both
/// carry the **identical** P1-P10 field sequence [`ChannelStatusFields`]
/// already models for `IF` (25-byte body, terminator at column 28) —
/// confirmed column-by-column against the manual's own page image, not
/// assumed from the architect's cross-batch summary. Two narrow
/// differences from `IF`'s own use of the same shape:
///
/// - `MR`/`MW`'s P1 (channel) is documented `001-117` only — **no** `000`
///   VFO-mode sentinel (unlike `IF`, which can report VFO mode via
///   `channel=0`). [`ChannelStatusFields::parse`] itself still structurally
///   accepts `0..=117` (it's shared with `IF`), so `handle_command`'s
///   `Mr`/`Mw`/`Mt` arms add their own `channel >= 1` check on top.
/// - `MR`'s own P7 (select) legend is narrower than `IF`'s ("0: VFO
///   1: Memory" only, vs. `IF`'s full 7-value 0-6 legend) — since `MR`
///   only ever addresses an explicit memory channel number, this emulator
///   always reports `1` (Memory) in `MR`'s answer, a documented judgment
///   call (the manual doesn't explain when `MR` would ever report `0`).
///   `MW`'s P7 legend text is `"00: (Fixed)"`, but the column diagram
///   gives P7 only a single wire column (matching `IF`/`MR`'s P7 width) —
///   almost certainly a copy-paste artifact from the immediately-adjacent
///   P9 legend line (also `"00: (Fixed)"`, but genuinely 2 columns wide),
///   not a real 2-digit field; treated as fixed **`"0"`** (1 digit),
///   consistent with the column diagram. `handle_command`'s `Mw` arm
///   rejects any P7 other than `"0"`.
///
/// `MT` ("MEMORY CHANNEL WRITE/TAG") is a genuine superset: its Set/Answer
/// body is [`ChannelStatusFields::WIRE_WIDTH`] (25 bytes, P1-P10) + **P11**
/// (1 byte, `"0: (Fixed)"`, not stored — same "reserved, unvalidated"
/// treatment [`ChannelStatusFields`] already gives P9) + **P12** (up to 12
/// ASCII characters, the tag — genuinely new, not covered by
/// [`ChannelStatusFields`] at all), for a total 38-byte body (confirmed
/// against the manual's own column-numbered sub-rows, terminator at column
/// 41). `MT`'s P7 legend explicitly distinguishes direction — `"Set:
/// 0: (Fixed) / Read: 0: VFO 1: Memory"` — matching this crate's `Mr`/`Mw`
/// treatment above exactly (fixed `0` on write, always `1` on this
/// emulator's read/answer).
///
/// **P12 character-set/padding, a documented judgment call, not
/// manual-cited on `MT`'s own page**: `MT`'s own legend says only `"TAG
/// Characters (up to 12 characters) (ASCII)"`, with no explicit
/// character-set restriction or padding convention stated there. This
/// implementation applies the CAT Operation section's *general* parameter
/// rule (manual p.2: "the parameter digits should be filled using any
/// character except the ASCII control codes (00 to 1Fh) and the terminator
/// (;)") as the character-set restriction — printable ASCII space (0x20)
/// through tilde (0x7E), excluding `;` (structurally unreachable in
/// practice, since framing already splits a frame on its first `;`, but
/// checked for defensive symmetry with [`crate::MemoryTag::new`]'s
/// client-side constructor). Since the column diagram fixes P12 at exactly
/// 12 wire columns regardless of "up to 12" content length, shorter tags
/// are padded with trailing ASCII spaces on the wire and trimmed back off
/// on parse — this implementation's own convention (inspired by, but not
/// identical to, `ts570d`'s own fixed-width-field padding precedent for an
/// analogous concept), not itself manual-cited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryChannelRecord {
    /// P2: frequency, Hz. The manual states no factory default for an
    /// unprogrammed channel; `0` is this implementation's "vacant channel"
    /// sentinel default, inspired by (not identical to) `ts570d`'s own
    /// "freq_hz == 0 means vacant" convention for the analogous concept —
    /// a documented judgment call, not manual-specified.
    pub frequency_hz: u64,
    /// P3: combined sign+offset, range -9999..=9999 Hz. Default `0`.
    pub clarifier_offset_hz: i16,
    /// P4: RX CLAR on/off. Default `false`.
    pub rx_clarifier_on: bool,
    /// P5: TX CLAR on/off. Default `false`.
    pub tx_clarifier_on: bool,
    /// P6: raw hex-nibble mode value, matches `Ft991aState::mode`. Default
    /// `0x2` (USB) — a legal value is required (unlike `frequency_hz`,
    /// `Ft991aState::mode` and [`ChannelStatusFields::parse`] both reject
    /// `0`), matching `Ft991aState::mode`'s own default.
    pub mode: u8,
    /// P8: CTCSS/DCS status, 0-4. Default `0`.
    pub tone_status: u8,
    /// P10: offset type, 0-2 (Simplex/Plus/Minus). Default `0`.
    pub offset_type: u8,
    /// `MT`'s P12: up to 12 ASCII characters, stored **trimmed** of
    /// trailing padding spaces (padded back out to 12 columns only at the
    /// wire boundary — see struct docs). Default empty (blank tag, i.e.
    /// 12 wire spaces).
    pub tag: String,
}

impl Default for MemoryChannelRecord {
    fn default() -> Self {
        Self {
            frequency_hz: 0,
            clarifier_offset_hz: 0,
            rx_clarifier_on: false,
            tx_clarifier_on: false,
            mode: 0x2,
            tone_status: 0,
            offset_type: 0,
            tag: String::new(),
        }
    }
}

/// Validate a variable-length ASCII wire fragment against the general
/// parameter character-set rule (manual p.2: printable ASCII space (0x20)
/// through tilde (0x7E), excluding `;`). Originally written for `MT`'s
/// 12-character P12 tag ([`MemoryChannelRecord`]'s doc comment); reused
/// unmodified by batch 4's `KM` message content (module docs' "KM" section)
/// — both are variable-ASCII-content fields governed by the same manual
/// rule, not two separate conventions.
fn is_valid_ascii_wire_content(s: &str) -> bool {
    s.chars().all(|c| (' '..='~').contains(&c) && c != ';')
}

/// Parse a single `"0"`/`"1"` ASCII-digit wire flag into a `bool`.
fn parse_bit(s: &str) -> Option<bool> {
    match s {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

/// `KY`'s playback-family selector (manual p.11) — see the module docs'
/// "KY" section for why this is **not** a second 5-channel message store,
/// just a tag on which of two playback modes a `KY` trigger names for the
/// *same* [`Ft991aState::keyer_memories`] channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyerPlaybackMode {
    /// P1 `1`-`5`: "Keyer Memory 'N' Playback".
    KeyerMemory,
    /// P1 `6`-`9`,`A`: "Message Keyer 'N' Playback".
    MessageKeyer,
}

/// Map a `KY` P1 wire character (manual p.11: `1`-`9`,`A`) to its
/// `(channel 1-5, mode)` pair. Returns `None` for any other character.
pub fn ky_selector_from_wire(c: char) -> Option<(u8, KeyerPlaybackMode)> {
    match c {
        '1'..='5' => Some((
            c.to_digit(10).expect("'1'..='5' are ASCII digits") as u8,
            KeyerPlaybackMode::KeyerMemory,
        )),
        '6'..='9' => Some((
            c.to_digit(10).expect("'6'..='9' are ASCII digits") as u8 - 5,
            KeyerPlaybackMode::MessageKeyer,
        )),
        'A' => Some((5, KeyerPlaybackMode::MessageKeyer)),
        _ => None,
    }
}

/// Inverse of [`ky_selector_from_wire`]: format a `(channel 1-5, mode)`
/// pair back into its `KY` P1 wire character. Returns `None` if `channel`
/// is out of range.
pub fn ky_selector_to_wire(channel: u8, mode: KeyerPlaybackMode) -> Option<char> {
    if !(1..=5).contains(&channel) {
        return None;
    }
    Some(match mode {
        KeyerPlaybackMode::KeyerMemory => {
            char::from_digit(channel as u32, 10).expect("1..=5 are single ASCII digits")
        }
        KeyerPlaybackMode::MessageKeyer if channel == 5 => 'A',
        KeyerPlaybackMode::MessageKeyer => {
            char::from_digit((channel + 5) as u32, 10).expect("6..=9 are single ASCII digits")
        }
    })
}

/// `ED`/`EU`'s `P1` selector (manual p.7): which physical front-panel
/// encoder is being turned. `Ft991a`-inherent domain type — no
/// `Radio`-trait exposure, per module docs' "ED/EU" section (FT-991A-
/// specific, no generic concept to attach to).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderSelector {
    /// `0`: MAIN ENCODER.
    Main,
    /// `1`: SUB ENCODER.
    Sub,
    /// `8`: MULTI ENCODER.
    Multi,
}

impl EncoderSelector {
    /// Return the `ED`/`EU` `P1` wire digit character for this selector.
    pub fn as_wire_digit(self) -> char {
        match self {
            EncoderSelector::Main => '0',
            EncoderSelector::Sub => '1',
            EncoderSelector::Multi => '8',
        }
    }

    /// Parse an `ED`/`EU` `P1` wire digit character into a selector.
    /// Returns `None` for any digit other than `0`/`1`/`8`.
    pub fn from_wire_digit(c: char) -> Option<Self> {
        match c {
            '0' => Some(EncoderSelector::Main),
            '1' => Some(EncoderSelector::Sub),
            '8' => Some(EncoderSelector::Multi),
            _ => None,
        }
    }
}

/// `BS`'s full 16-band table (manual p.5), in wire order — see module docs'
/// "BS's full 16-band table" section for the complete transcription. `13`
/// is a genuine, documented gap (no band assigned) and is deliberately
/// **absent** from this array, not merely skipped by convention — consulted
/// only by [`next_band`]/[`prev_band`], which therefore never land on it.
const BAND_CODES: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14, 15, 16];

/// Step to the next `BS` band code up (`BU`), wrapping past the highest
/// band (`16`, 430 MHz) back to the lowest (`00`, 1.8 MHz) and skipping the
/// `13` gap. Manual-silent boundary behavior — documented judgment call,
/// same category as batch 1's `CH` wrap-around. `current` values outside
/// [`BAND_CODES`] (unreachable via `BS`'s own validation, defensive only)
/// are treated as if positioned just before the lowest band.
fn next_band(current: u8) -> u8 {
    let idx = BAND_CODES.iter().position(|&b| b == current);
    match idx {
        Some(i) => BAND_CODES[(i + 1) % BAND_CODES.len()],
        None => BAND_CODES[0],
    }
}

/// Step to the next `BS` band code down (`BD`), wrapping past the lowest
/// band back to the highest and skipping the `13` gap. See [`next_band`]'s
/// doc comment for the shared judgment-call/defensive-fallback notes.
fn prev_band(current: u8) -> u8 {
    let idx = BAND_CODES.iter().position(|&b| b == current);
    match idx {
        Some(i) => BAND_CODES[(i + BAND_CODES.len() - 1) % BAND_CODES.len()],
        None => BAND_CODES[0],
    }
}

/// `DN`/`UP`'s per-press VFO-A step, Hz — a documented, arbitrary
/// implementation choice (neither the FT-991A manual nor `ts570d`'s Kenwood
/// manual states an exact Hz-per-press value; `ts570d`'s own emulator picks
/// a fixed 100 Hz). See module docs' "DN/UP" section for the full
/// cross-radio corroboration behind modeling these as mic UP/DWN button
/// presses at all.
const MIC_STEP_HZ: u64 = 10;

/// Simulated FT-991A radio state (grows alongside command coverage).
#[derive(Debug, Clone)]
pub struct Ft991aState {
    pub vfo_a_hz: u64,
    pub vfo_b_hz: u64,
    /// Raw hex-nibble mode value, 1 (LSB) ..= 0xE (C4FM). See manual p.11.
    pub mode: u8,
    /// CAT-asserted PTT state (0/1). Distinct from any front-panel-asserted
    /// TX a later wave's emulator might model (the `TX;` answer's `2`
    /// value, "RADIO TX ON / CAT TX OFF") — this state machine only ever
    /// reports 0/1, never 2.
    pub cat_tx: u8,
    pub af_gain: u8,
    pub rf_gain: u8,
    pub squelch: u8,
    /// `PC` command value, watts (005-100).
    pub power_control: u8,
    pub smeter: u8,
    pub power_on: bool,

    // -- Batch 9: meters/status (`IF`/`RM`/`RI`/`RS`/`MS`/`UL`) --------
    /// `IF`'s P1 (memory channel, 0-117; `0` = VFO-mode sentinel, see
    /// [`ChannelStatusFields`]'s doc comment). No `Set` command in any
    /// landed batch changes this yet (memory channel commands are batch 2,
    /// out of this task's scope) — always `0` today.
    pub if_channel: u8,
    /// `IF`'s P3 clarifier offset, Hz, -9999..=9999. Written by batch 3's
    /// `RD`/`RU` (absolute set, sign per command identity) and cleared by
    /// `RC` — see the module docs' "RD/RU's direction encoding" and "RC"
    /// sections. Shared by RX and TX (no separate per-direction offset —
    /// see `rx_clarifier_on`/`tx_clarifier_on`).
    pub clarifier_offset_hz: i16,
    /// `IF`'s P4 (RX CLAR on/off) — whether `clarifier_offset_hz` is applied
    /// to RX. Written by batch 3's `RT`.
    pub rx_clarifier_on: bool,
    /// `IF`'s P5 (TX CLAR on/off) — whether `clarifier_offset_hz` is applied
    /// to TX (independently of `rx_clarifier_on`). Written by batch 3's
    /// `XT`.
    pub tx_clarifier_on: bool,
    /// `IF`'s P7 (VFO/Memory/QMB select, 0-6). Batch 1's `VM` command
    /// toggles this between `0` (VFO) and `1` (Memory) — see the module
    /// docs' "VM/AM manual heading inconsistency" section for the full
    /// citation and reasoning behind that judgment call.
    pub channel_select: u8,
    /// `IF`'s P8 (CTCSS/DCS status, 0-4). Written by batch 3's `CT`.
    pub tone_status: u8,
    /// `IF`'s P10 (offset type, 0-2: Simplex/Plus Shift/Minus Shift).
    /// Written by batch 10's `OS` (manual p.13) — reused directly, not a
    /// new field for `OS`; also read by batch 10's `OI` (see module docs'
    /// "OI" section).
    pub offset_type: u8,
    /// `MS`'s P1: which physical meter (COMP/ALC/PO/SWR/ID/VDD, 0-5) is
    /// selected on the front panel — determines `RM`'s `0`/`2` answer (see
    /// `Ft991aState::selected_meter_reading`). Manual p.12 gives no stated
    /// default; `0` (COMP) is this implementation's arbitrary choice,
    /// documented not manual-specified (same category of open item as
    /// `Ft991a::set_power_on`'s wake-sequence note).
    pub meter_select: u8,
    /// `RM` P1=3 direct-select COMP reading, 0-255.
    pub comp_meter: u8,
    /// `RM` P1=4 direct-select ALC reading, 0-255.
    pub alc_meter: u8,
    /// `RM` P1=5 direct-select PO (power output) reading, 0-255.
    pub po_meter: u8,
    /// `RM` P1=6 direct-select SWR reading, 0-255.
    pub swr_meter: u8,
    /// `RM` P1=7 direct-select Id (drain current) reading, 0-255. Distinct
    /// from [`FT991A_ID`]/the `ID` command's fixed model string — same
    /// two-letter mnemonic, unrelated meaning (manual p.15's meter-name
    /// list vs. p.10's model identification).
    pub id_meter: u8,
    /// `RM` P1=8 direct-select Vd (drain voltage) reading, 0-255.
    pub vdd_meter: u8,
    /// `RS`'s P1 (0=NORMAL MODE, 1=MENU MODE). No CAT command in any
    /// landed batch enters MENU MODE, so this is always `false` today —
    /// documented simplification, not a manual-specified default.
    pub menu_mode: bool,
    /// `UL`'s P1 (0=PLL Lock, 1=PLL Unlock). `UL` has no `Set` form on
    /// real hardware either (monitoring-only) — always `false` (Lock).
    pub pll_unlocked: bool,

    // -- EX menu, first sub-batch: the 9 PTT/keying-relevant items -----
    //
    // Each field stores the current raw P2 wire digit (as `u8`) for one
    // `EX_MENU_TABLE` row. The manual states no factory-default value for
    // any of these — each default below is this implementation's
    // documented, arbitrary choice (same category of open item as
    // `meter_select`'s default above), picked as the value that reads
    // "DAKY"/"OFF"/"DATA" in that item's own P2 legend (falling back to
    // `1` for items 072/077, whose legend has no `0` value at all — see
    // `EX_MENU_TABLE`'s doc comment on the 048/109-vs-072/077
    // inconsistency).
    /// EX 047 "AM PTT SELECT" (`0:DAKY 1:RTS 2:DTR`). Default `0` (DAKY).
    pub ex_am_ptt_select: u8,
    /// EX 048 "AM PORT SELECT" (`0:DATA 1:USB`). Default `0` (DATA).
    pub ex_am_port_select: u8,
    /// EX 060 "PC KEYING" (`0:OFF 1:DAKY 2:RTS 3:DTR`) — the item this
    /// wave's RTS/DTR CW-keying feature (§10.2-10.4) reads/writes.
    /// Default `0` (OFF).
    pub ex_pc_keying: u8,
    /// EX 071 "DATA PTT SELECT" (`0:DAKY 1:RTS 2:DTR`). Default `0` (DAKY).
    pub ex_data_ptt_select: u8,
    /// EX 072 "DATA PORT SELECT" (`1:DATA 2:USB` — legend starts at `1`,
    /// not `0`, see `EX_MENU_TABLE`'s doc comment). Default `1` (DATA).
    pub ex_data_port_select: u8,
    /// EX 076 "FM PKT PTT SELECT" (`0:DAKY 1:RTS 2:DTR`). Default `0`
    /// (DAKY).
    pub ex_fm_pkt_ptt_select: u8,
    /// EX 077 "FM PKT PORT SELECT" (`1:DATA 2:USB`, same one-based legend
    /// as 072). Default `1` (DATA).
    pub ex_fm_pkt_port_select: u8,
    /// EX 108 "SSB PTT SELECT" (`0:DAKY 1:RTS 2:DTR`). Default `0` (DAKY).
    pub ex_ssb_ptt_select: u8,
    /// EX 109 "SSB PORT SELECT" (`0:DATA 1:USB`). Default `0` (DATA).
    pub ex_ssb_port_select: u8,

    // -- EX menu, second sub-batch: items 001-046 (minus 027) -----------
    //
    // Each field stores the current raw P2 wire value (as `i32`, wide
    // enough for this sub-batch's signed and multi-digit-unsigned ranges,
    // unlike the first sub-batch's `u8` fields above) for one
    // `EX_MENU_TABLE` row. Default-value policy (manual states no factory
    // default for any of these): `Enumerated` items default to their
    // legend's first listed value; unsigned `Range` items default to
    // `min`; signed `Range` items (035, 039) default to `0`. See
    // `EX_MENU_TABLE`'s doc comment for the full per-item citation table.
    /// EX 001 "AGC FAST DELAY", 0020-4000 msec, 20 msec/step. Default `20`.
    pub ex_agc_fast_delay: i32,
    /// EX 002 "AGC MID DELAY", 0020-4000 msec, 20 msec/step. Default `20`.
    pub ex_agc_mid_delay: i32,
    /// EX 003 "AGC SLOW DELAY", 0020-4000 msec, 20 msec/step. Default `20`.
    pub ex_agc_slow_delay: i32,
    /// EX 004 "HOME FUNCTION" (`0:SCOPE 1:FUNCTION`). Default `0` (SCOPE).
    pub ex_home_function: i32,
    /// EX 005 "MY CALL INDICATION", 0-5 sec. Default `0`.
    pub ex_my_call_indication: i32,
    /// EX 006 "DISPLAY COLOR" (`0:BLUE 1:GRAY 2:GREEN 3:ORANGE 4:PURPLE
    /// 5:RED 6:SKY BLUE`). Default `0` (BLUE).
    pub ex_display_color: i32,
    /// EX 007 "DIMMER LED" (`0:1 1:2`). Default `0`.
    pub ex_dimmer_led: i32,
    /// EX 008 "DIMMER TFT", 00-15. Default `0`.
    pub ex_dimmer_tft: i32,
    /// EX 009 "BAR MTR PEAK HOLD" (`0:OFF 1:0.5sec 2:1.0sec 3:2.0sec`).
    /// Default `0` (OFF).
    pub ex_bar_mtr_peak_hold: i32,
    /// EX 010 "DVS RX OUT LEVEL", 000-100. Default `0`.
    pub ex_dvs_rx_out_level: i32,
    /// EX 011 "DVS TX OUT LEVEL", 000-100. Default `0`.
    pub ex_dvs_tx_out_level: i32,
    /// EX 012 "KEYER TYPE" (`0:OFF 1:BUG 2:ELEKEY-A 3:ELEKEY-B 4:ELEKEY-Y
    /// 5:ACS`). Default `0` (OFF).
    pub ex_keyer_type: i32,
    /// EX 013 "KEYER DOT/DASH" (`0:NORMAL 1:REVERSE`). Default `0`.
    pub ex_keyer_dot_dash: i32,
    /// EX 014 "CW WEIGHT", wire 25-45 (display 2.5-4.5). Default `25`.
    pub ex_cw_weight: i32,
    /// EX 015 "BEACON INTERVAL", 000-690 sec (000=OFF). Default `0` (OFF).
    pub ex_beacon_interval: i32,
    /// EX 016 "NUMBER STYLE" (`0:1290 1:AUNO 2:AUNT 3:A2NO 4:A2NT 5:12NO
    /// 6:12NT`). Default `0`.
    pub ex_number_style: i32,
    /// EX 017 "CONTEST NUMBER", 0000-9999. Default `0`.
    pub ex_contest_number: i32,
    /// EX 018 "CW MEMORY 1" (`0:TEXT 1:MESSAGE`). Default `0` (TEXT).
    pub ex_cw_memory_1: i32,
    /// EX 019 "CW MEMORY 2" (`0:TEXT 1:MESSAGE`). Default `0` (TEXT).
    pub ex_cw_memory_2: i32,
    /// EX 020 "CW MEMORY 3" (`0:TEXT 1:MESSAGE`). Default `0` (TEXT).
    pub ex_cw_memory_3: i32,
    /// EX 021 "CW MEMORY 4" (`0:TEXT 1:MESSAGE`). Default `0` (TEXT).
    pub ex_cw_memory_4: i32,
    /// EX 022 "CW MEMORY 5" (`0:TEXT 1:MESSAGE`). Default `0` (TEXT).
    pub ex_cw_memory_5: i32,
    /// EX 023 "NB WIDTH" (`0:1ms 1:3ms 2:10ms`). Default `0`.
    pub ex_nb_width: i32,
    /// EX 024 "NB REJECTION" (`0:10dB 1:30dB 2:50dB`). Default `0`.
    pub ex_nb_rejection: i32,
    /// EX 025 "NB LEVEL", 00-10. Default `0`.
    pub ex_nb_level: i32,
    /// EX 026 "BEEP LEVEL", 000-100. Default `0`.
    pub ex_beep_level: i32,
    // 027 "TIME ZONE" deliberately absent — see `EX_MENU_TABLE`'s doc
    // comment.
    /// EX 028 "GPS/232C SELECT" (`0:GPS1 1:GPS2 3:RS232C`, gap at 2).
    /// Default `0` (GPS1).
    pub ex_gps_232c_select: i32,
    /// EX 029 "232C RATE" (`0:4800bps 1:9600bps 2:19200bps 3:38400bps`).
    /// Default `0`.
    pub ex_rs232c_rate: i32,
    /// EX 030 "232C TOT" (`0:10msec 1:100msec 2:1000msec 3:3000msec`).
    /// Default `0`.
    pub ex_rs232c_tot: i32,
    /// EX 031 "CAT RATE" (`0:4800bps 1:9600bps 2:19200bps 3:38400bps`).
    /// Default `0`.
    pub ex_cat_rate: i32,
    /// EX 032 "CAT TOT" (`0:10msec 1:100msec 2:1000msec 3:3000msec`).
    /// Default `0`.
    pub ex_cat_tot: i32,
    /// EX 033 "CAT RTS" (`0:DISABLE 1:ENABLE`). Default `0` (DISABLE).
    pub ex_cat_rts: i32,
    /// EX 034 "MEM GROUP" (`0:DISABLE 1:ENABLE`). Default `0` (DISABLE).
    pub ex_mem_group: i32,
    /// EX 035 "QUICK SPLIT FREQ", signed -20..=+20 kHz. Default `0`.
    pub ex_quick_split_freq: i32,
    /// EX 036 "TX TOT", 00-30 min (00=OFF). Default `0` (OFF).
    pub ex_tx_tot: i32,
    /// EX 037 "MIC SCAN" (`0:DISABLE 1:ENABLE`). Default `0` (DISABLE).
    pub ex_mic_scan: i32,
    /// EX 038 "MIC SCAN RESUME" (`0:PAUSE 1:TIME`). Default `0` (PAUSE).
    pub ex_mic_scan_resume: i32,
    /// EX 039 "REF FREQ ADJ", signed -25..=+25. Default `0`.
    pub ex_ref_freq_adj: i32,
    /// EX 040 "CLAR MODE SELECT" (`0:RX 1:TX 2:TRX`). Default `0` (RX).
    pub ex_clar_mode_select: i32,
    /// EX 041 "AM LCUT FREQ", 00-19 (00=OFF). Default `0` (OFF).
    pub ex_am_lcut_freq: i32,
    /// EX 042 "AM LCUT SLOPE" (`0:6dB/oct 1:18dB/oct`). Default `0`.
    pub ex_am_lcut_slope: i32,
    /// EX 043 "AM HCUT FREQ", 00-67 (00=OFF). Default `0` (OFF).
    pub ex_am_hcut_freq: i32,
    /// EX 044 "AM HCUT SLOPE" (`0:6dB/oct 1:18dB/oct`). Default `0`.
    pub ex_am_hcut_slope: i32,
    /// EX 045 "AM MIC SELECT" (`0:MIC 1:REAR`). Default `0` (MIC).
    pub ex_am_mic_select: i32,
    /// EX 046 "AM OUT LEVEL", 000-100. Default `0`.
    pub ex_am_out_level: i32,

    // -- EX menu, third sub-batch: items 049-079 (minus 060/071/072/076/077,
    // already landed by the first sub-batch) -----------------------------
    //
    // Same storage/default-policy conventions as the second sub-batch above
    // (manual states no factory default for any of these): `Enumerated`
    // items default to their legend's first listed value; unsigned `Range`
    // items default to `min`; signed `Range` items (064, 065) default to
    // `0`. See `EX_MENU_TABLE`'s doc comment for the full per-item citation
    // table and the 068/069 digit-width resolution.
    /// EX 049 "AM DATA GAIN", 000-100. Default `0`.
    pub ex_am_data_gain: i32,
    /// EX 050 "CW LCUT FREQ", 00-19 (00=OFF). Default `0` (OFF).
    pub ex_cw_lcut_freq: i32,
    /// EX 051 "CW LCUT SLOPE" (`0:6dB/oct 1:18dB/oct`). Default `0`.
    pub ex_cw_lcut_slope: i32,
    /// EX 052 "CW HCUT FREQ", 00-67 (00=OFF). Default `0` (OFF).
    pub ex_cw_hcut_freq: i32,
    /// EX 053 "CW HCUT SLOPE" (`0:6dB/oct 1:18dB/oct`). Default `0`.
    pub ex_cw_hcut_slope: i32,
    /// EX 054 "CW OUT LEVEL", 000-100. Default `0`.
    pub ex_cw_out_level: i32,
    /// EX 055 "CW AUTO MODE" (`0:OFF 1:50MHz 2:ON`). Default `0` (OFF).
    pub ex_cw_auto_mode: i32,
    /// EX 056 "CW BK-IN TYPE" (`0:SEMI BREAK-IN 1:FULL BREAK-IN`). Default
    /// `0`.
    pub ex_cw_bk_in_type: i32,
    /// EX 057 "CW BK-IN DELAY", 0030-3000 msec, 10 msec/step. Default `30`
    /// (its `min` — `0` is not a legal value for this item).
    pub ex_cw_bk_in_delay: i32,
    /// EX 058 "CW WAVE SHAPE" (`0:1msec 1:2msec 2:4msec 3:6msec`). Default
    /// `0`.
    pub ex_cw_wave_shape: i32,
    /// EX 059 "CW FREQ DISPLAY" (`0:DIRECT FREQ 1:PITCH OFFSET`). Default
    /// `0`.
    pub ex_cw_freq_display: i32,
    // 060 "PC KEYING" already landed as `ex_pc_keying` (first sub-batch).
    /// EX 061 "QSK DELAY TIME" (`0:15msec 1:20msec 2:25msec 3:30msec`).
    /// Default `0`.
    pub ex_qsk_delay_time: i32,
    /// EX 062 "DATA MODE" (`0:PSK 1:OTHER`). Default `0` (PSK).
    pub ex_data_mode: i32,
    /// EX 063 "PSK TONE" (`0:1000Hz 1:1500Hz 2:2000Hz`). Default `0`.
    pub ex_psk_tone: i32,
    /// EX 064 "OTHER DISP (SSB)", signed -3000..=+3000 Hz, 10 Hz steps.
    /// Default `0`.
    pub ex_other_disp_ssb: i32,
    /// EX 065 "OTHER SHIFT (SSB)", signed -3000..=+3000 Hz, 10 Hz steps.
    /// Default `0`.
    pub ex_other_shift_ssb: i32,
    /// EX 066 "DATA LCUT FREQ", 00-19 (00=OFF). Default `0` (OFF).
    pub ex_data_lcut_freq: i32,
    /// EX 067 "DATA LCUT SLOPE" (`0:6dB/oct 1:18dB/oct`). Default `0`.
    pub ex_data_lcut_slope: i32,
    /// EX 068 "DATA HCUT FREQ", 00-67 (00=OFF). Default `0` (OFF). See
    /// `EX_MENU_TABLE`'s doc comment for this item's digit-width
    /// resolution (2, not the manual's literally-printed 1).
    pub ex_data_hcut_freq: i32,
    /// EX 069 "DATA HCUT SLOPE" (`0:6dB/oct 1:18dB/oct`). Default `0`. See
    /// `EX_MENU_TABLE`'s doc comment for this item's digit-width
    /// resolution (1, not the manual's literally-printed 2).
    pub ex_data_hcut_slope: i32,
    /// EX 070 "DATA IN SELECT" (`0:MIC 1:REAR`). Default `0` (MIC).
    pub ex_data_in_select: i32,
    // 071/072 "DATA PTT/PORT SELECT" already landed (first sub-batch).
    /// EX 073 "DATA OUT LEVEL", 000-100. Default `0`.
    pub ex_data_out_level: i32,
    /// EX 074 "FM MIC SELECT" (`0:MIC 1:REAR`). Default `0` (MIC).
    pub ex_fm_mic_select: i32,
    /// EX 075 "FM OUT LEVEL", 000-100. Default `0`.
    pub ex_fm_out_level: i32,
    // 076/077 "FM PKT PTT/PORT SELECT" already landed (first sub-batch).
    /// EX 078 "FM PKT TX GAIN", 000-100. Default `0`.
    pub ex_fm_pkt_tx_gain: i32,
    /// EX 079 "FM PKT MODE" (`0:1200 1:9600`). Default `0`.
    pub ex_fm_pkt_mode: i32,

    // -- EX menu, fourth sub-batch: items 080-153, minus 087 (skipped) and
    // 108/109 (already landed above) ---------------------------------
    //
    // Same storage/default-policy conventions as prior sub-batches (manual
    // states no factory default for any of these): `Enumerated` items
    // default to their legend's first listed value; unsigned `Range` items
    // default to `min`; signed `Range` items default to `0`. See
    // `EX_MENU_TABLE`'s doc comment for the full per-item citation table
    // and the 100/116/147 resolution notes.
    /// EX 080 "RPT SHIFT 28MHz", 0000-1000 kHz, 10 kHz/step. Default `0`.
    pub ex_rpt_shift_28mhz: i32,
    /// EX 081 "RPT SHIFT 50MHz", 0000-4000 kHz, 10 kHz/step. Default `0`.
    pub ex_rpt_shift_50mhz: i32,
    /// EX 082 "RPT SHIFT 144MHz", 0000-4000 kHz, 10 kHz/step. Default `0`.
    pub ex_rpt_shift_144mhz: i32,
    /// EX 083 "RPT SHIFT 430MHz", 00000-10000 kHz, 10 kHz/step. Default `0`.
    pub ex_rpt_shift_430mhz: i32,
    /// EX 084 "ARS 144MHz" (`0:OFF 1:ON`). Default `0`.
    pub ex_ars_144mhz: i32,
    /// EX 085 "ARS 430MHz" (`0:OFF 1:ON`). Default `0`.
    pub ex_ars_430mhz: i32,
    /// EX 086 "DCS POLARITY" (`0:Tn-Rn 1:Tn-Riv 2:Tiv-Rn 3:Tiv-Riv`).
    /// Default `0`.
    pub ex_dcs_polarity: i32,
    // 087 "RADIO ID" permanently skipped — see EX_MENU_TABLE's doc comment.
    /// EX 088 "GM DISPLY" (`0:DISTANCE 1:STRENGTH`). Default `0`.
    pub ex_gm_display: i32,
    /// EX 089 "DISTANCE" (`0:km 1:mile`). Default `0`.
    pub ex_distance: i32,
    /// EX 090 "AMS TX MODE" (`0:AUTO 1:MANUAL 2:DN 3:VW 4:ANALOG`). Default
    /// `0`.
    pub ex_ams_tx_mode: i32,
    /// EX 091 "STANDBY BEEP" (`0:OFF 1:ON`). Default `0`.
    pub ex_standby_beep: i32,
    /// EX 092 "RTTY LCUT FREQ", 00-19 (00=OFF). Default `0` (OFF).
    pub ex_rtty_lcut_freq: i32,
    /// EX 093 "RTTY LCUT SLOPE" (`0:6dB/oct 1:18dB/oct`). Default `0`.
    pub ex_rtty_lcut_slope: i32,
    /// EX 094 "RTTY HCUT FREQ", 00-67 (00=OFF). Default `0` (OFF).
    pub ex_rtty_hcut_freq: i32,
    /// EX 095 "RTTY HCUT SLOPE" (`0:6dB/oct 1:18dB/oct`). Default `0`.
    pub ex_rtty_hcut_slope: i32,
    /// EX 096 "RTTY SHIFT PORT" (`0:SHIFT 1:DTR 2:RTS`). Default `0`.
    pub ex_rtty_shift_port: i32,
    /// EX 097 "RTTY POLARITY-RX" (`0:NORMAL 1:REVERSE`). Default `0`.
    pub ex_rtty_polarity_rx: i32,
    /// EX 098 "RTTY POLARITY-TX" (`0:NORMAL 1:REVERSE`). Default `0`.
    pub ex_rtty_polarity_tx: i32,
    /// EX 099 "RTTY OUT LEVEL", 000-100. Default `0`.
    pub ex_rtty_out_level: i32,
    /// EX 100 "RTTY SHIFT FREQ" (`0:170Hz 1:200Hz 2:425Hz 3:850Hz`) — the
    /// manual's own printed legend has a duplicate `1:` label, resolved to
    /// 0-based via corroboration, see `EX_MENU_TABLE`'s doc comment.
    /// Default `0`.
    pub ex_rtty_shift_freq: i32,
    /// EX 101 "RTTY MARK FREQ" (`1:1275Hz 2:2125Hz`). Default `1` (its
    /// first listed value — `0` is not legal for this item).
    pub ex_rtty_mark_freq: i32,
    /// EX 102 "SSB LCUT FREQ", 00-19 (00=OFF). Default `0` (OFF).
    pub ex_ssb_lcut_freq: i32,
    /// EX 103 "SSB LCUT SLOPE" (`0:6dB/oct 1:18dB/oct`). Default `0`.
    pub ex_ssb_lcut_slope: i32,
    /// EX 104 "SSB HCUT FREQ", 00-67 (00=OFF). Default `0` (OFF).
    pub ex_ssb_hcut_freq: i32,
    /// EX 105 "SSB HCUT SLOPE" (`0:6dB/oct 1:18dB/oct`). Default `0`.
    pub ex_ssb_hcut_slope: i32,
    /// EX 106 "SSB MIC SELECT" (`0:MIC 1:REAR`). Default `0` (MIC).
    pub ex_ssb_mic_select: i32,
    /// EX 107 "SSB OUT LEVEL", 000-100. Default `0`.
    pub ex_ssb_out_level: i32,
    // 108/109 "SSB PTT/PORT SELECT" already landed (first sub-batch).
    /// EX 110 "SSB TX BPF" (5-way band-pass-filter preset selector).
    /// Default `0`.
    pub ex_ssb_tx_bpf: i32,
    /// EX 111 "APF WIDTH" (`0:NARROW 1:MEDIUM 2:WIDE`). Default `0`.
    pub ex_apf_width: i32,
    /// EX 112 "CONTOUR LEVEL", signed -40..=+20. Default `0`.
    pub ex_contour_level: i32,
    /// EX 113 "CONTOUR WIDTH", 01-10. Default `1` (its `min`).
    pub ex_contour_width: i32,
    /// EX 114 "IF NOTCH WIDTH" (`0:NARROW 1:WIDE`). Default `0`.
    pub ex_if_notch_width: i32,
    /// EX 115 "SCP DISPLAY MODE" (`0:SPECTRUM 1:WATER FALL`). Default `0`.
    pub ex_scp_display_mode: i32,
    /// EX 116 "SCP SPAN FREQ" — documented gap, legal values `03`-`07`
    /// only (`00`-`02` absent from the manual). Default `3` (its first
    /// listed legal value).
    pub ex_scp_span_freq: i32,
    /// EX 117 "SPECTRUM COLOR" (7-way color selector, same legend as item
    /// 006 "DISPLAY COLOR"). Default `0`.
    pub ex_spectrum_color: i32,
    /// EX 118 "WATER FALL COLOR" (8-way color selector — adds `7:MULTI` to
    /// 117's 7-way list). Default `0`.
    pub ex_water_fall_color: i32,
    /// EX 119 "PRMTRC EQ1 FREQ", 00-07 (00=OFF). Default `0` (OFF).
    pub ex_prmtrc_eq1_freq: i32,
    /// EX 120 "PRMTRC EQ1 LEVEL", signed -20..=+10. Default `0`.
    pub ex_prmtrc_eq1_level: i32,
    /// EX 121 "PRMTRC EQ1 BWTH", 01-10. Default `1` (its `min`).
    pub ex_prmtrc_eq1_bwth: i32,
    /// EX 122 "PRMTRC EQ2 FREQ", 00-09 (00=OFF). Default `0` (OFF).
    pub ex_prmtrc_eq2_freq: i32,
    /// EX 123 "PRMTRC EQ2 LEVEL", signed -20..=+10. Default `0`.
    pub ex_prmtrc_eq2_level: i32,
    /// EX 124 "PRMTRC EQ2 BWTH", 01-10. Default `1` (its `min`).
    pub ex_prmtrc_eq2_bwth: i32,
    /// EX 125 "PRMTRC EQ3 FREQ", 00-18 (00=OFF). Default `0` (OFF).
    pub ex_prmtrc_eq3_freq: i32,
    /// EX 126 "PRMTRC EQ3 LEVEL", signed -20..=+10. Default `0`.
    pub ex_prmtrc_eq3_level: i32,
    /// EX 127 "PRMTRC EQ3 BWTH", 01-10. Default `1` (its `min`).
    pub ex_prmtrc_eq3_bwth: i32,
    /// EX 128 "P-PRMTRC EQ1 FREQ", 00-07 (00=OFF). Default `0` (OFF).
    pub ex_p_prmtrc_eq1_freq: i32,
    /// EX 129 "P-PRMTRC EQ1 LEVEL", signed -20..=+10. Default `0`.
    pub ex_p_prmtrc_eq1_level: i32,
    /// EX 130 "P-PRMTRC EQ1 BWTH", 01-10. Default `1` (its `min`).
    pub ex_p_prmtrc_eq1_bwth: i32,
    /// EX 131 "P-PRMTRC EQ2 FREQ", 00-09 (00=OFF). Default `0` (OFF).
    pub ex_p_prmtrc_eq2_freq: i32,
    /// EX 132 "P-PRMTRC EQ2 LEVEL", signed -20..=+10. Default `0`.
    pub ex_p_prmtrc_eq2_level: i32,
    /// EX 133 "P-PRMTRC EQ2 BWTH", 01-10. Default `1` (its `min`).
    pub ex_p_prmtrc_eq2_bwth: i32,
    /// EX 134 "P-PRMTRC EQ3 FREQ", 00-18 (00=OFF). Default `0` (OFF).
    pub ex_p_prmtrc_eq3_freq: i32,
    /// EX 135 "P-PRMTRC EQ3 LEVEL", signed -20..=+10. Default `0`.
    pub ex_p_prmtrc_eq3_level: i32,
    /// EX 136 "P-PRMTRC EQ3 BWTH", 01-10. Default `1` (its `min`).
    pub ex_p_prmtrc_eq3_bwth: i32,
    /// EX 137 "HF TX MAX POWER", 005-100. Default `5` (its `min`).
    pub ex_tx_max_power_hf: i32,
    /// EX 138 "50M TX MAX POWER", 005-100. Default `5` (its `min`).
    pub ex_tx_max_power_50m: i32,
    /// EX 139 "144M TX MAX POWER", 005-050. Default `5` (its `min`).
    pub ex_tx_max_power_144m: i32,
    /// EX 140 "430M TX MAX POWER", 005-050. Default `5` (its `min`).
    pub ex_tx_max_power_430m: i32,
    /// EX 141 "TUNER SELECT" (`0:OFF 1:INTERNAL 2:EXTERNAL 3:ATAS
    /// 4:LAMP`). Default `0`.
    pub ex_tuner_select: i32,
    /// EX 142 "VOX SELECT" (`0:MIC 1:DATA`). Default `0`.
    pub ex_vox_select: i32,
    /// EX 143 "VOX GAIN", 000-100. Default `0`.
    pub ex_vox_gain: i32,
    /// EX 144 "VOX DELAY", 0030-3000 msec, 10 msec/step. Default `30`
    /// (its `min`).
    pub ex_vox_delay: i32,
    /// EX 145 "ANTI VOX GAIN", 000-100. Default `0`.
    pub ex_anti_vox_gain: i32,
    /// EX 146 "DATA VOX GAIN", 000-100. Default `0`.
    pub ex_data_vox_gain: i32,
    /// EX 147 "DATA VOX DELAY", 0030-3000 msec — step assumed `10`
    /// msec/step by corroboration with sibling 144, see `EX_MENU_TABLE`'s
    /// doc comment (this item's own manual row omits the step note).
    /// Default `30` (its `min`).
    pub ex_data_vox_delay: i32,
    /// EX 148 "ANTI DVOX GAIN", 000-100. Default `0`.
    pub ex_anti_dvox_gain: i32,
    /// EX 149 "EMERGENCY FREQ TX" (`0:DISABLE 1:ENABLE`). Default `0`.
    pub ex_emergency_freq_tx: i32,
    /// EX 150 "PRT/WIRES FREQ" (`0:MANUAL 1:PRESET`). Default `0`.
    pub ex_prt_wires_freq: i32,
    /// EX 151 "PRESET FREQUENCY", 00030000-47000000 (raw wire units, no
    /// separate scale factor stated by the manual). Default `30000` (its
    /// `min`).
    pub ex_preset_frequency: i32,
    /// EX 152 "SEARCH SETUP" (`0:HISTORY 1:ACTIVITY`). Default `0`.
    pub ex_search_setup: i32,
    /// EX 153 "WIRES DG-ID", 00-99 (00=AUTO, 01-99=DG-ID 01-99). Default
    /// `0` (AUTO).
    pub ex_wires_dg_id: i32,

    // -- Batch 2: memory channel records (`MC`/`MR`/`MW`/`MT`) ---------
    /// `MC`'s currently-selected memory channel, 1-117. Manual gives no
    /// stated factory default; `1` is this implementation's arbitrary
    /// choice, same category of open item as `meter_select`'s default
    /// above.
    pub selected_memory_channel: u8,
    /// The 117 memory channels' stored contents, indexed `channel - 1`
    /// (channels are numbered 1-117, no `0`). See
    /// [`MemoryChannelRecord`]'s doc comment for the full manual citation
    /// and the fields' per-channel defaults.
    pub memory_channels: Vec<MemoryChannelRecord>,

    // -- Batch 1: VFO/split/memory quick-ops (`AB BA AM VM MA CH QI QR QS
    // SV`) ---------------------------------------------------------------
    /// `QI`/`QR`'s dedicated Quick Memory Bank slot — a single storage
    /// location distinct from the 117 numbered `memory_channels` (see the
    /// module docs' "QI/QR" section). Reuses [`MemoryChannelRecord`] for
    /// convenient store/recall symmetry with `AM`/`MA`, even though `QI`/
    /// `QR` never touch its `tag` field.
    pub qmb: MemoryChannelRecord,
    /// `QS`'s toggled split state. No dedicated "split on"/"split off"
    /// command exists anywhere in the master table — see the module docs'
    /// "QS" section for why this is modeled as a plain toggle.
    pub split: bool,

    // -- Batch 3: clarifier/RIT-XIT + tone + IF-shift (`RT RC RD RU XT CN
    // CT IS`) --------------------------------------------------------------
    /// `CN`'s P3 when `P2=0` (CTCSS): raw [`CTCSS_TONES_DECIHZ`] table
    /// index, `0`-`49`. Manual gives no stated factory default; `0`
    /// (67.0 Hz) is this implementation's arbitrary choice, same category
    /// of open item as `meter_select`'s default.
    pub ctcss_tone_number: u8,
    /// `CN`'s P3 when `P2=1` (DCS): raw [`DCS_CODES`] table index,
    /// `0`-`103`. Same "arbitrary default" caveat as `ctcss_tone_number`.
    pub dcs_code_number: u8,
    /// `IS`'s IF-shift offset, Hz, -1200..=1200 in 20 Hz steps. Manual
    /// gives no stated factory default; `0` (no shift) is this
    /// implementation's choice — the one default in this batch that's not
    /// really a judgment call, since `0` is unambiguously "IF shift off".
    pub if_shift_hz: i16,

    // -- Batch 4: keyer/CW/break-in (KM KP KR KS KY CS ZI BI SD) -------
    /// `KM`'s 5 keyer memory channels, indexed `channel - 1` (channels are
    /// numbered 1-5, no `0`). Manual gives no factory default for an
    /// unprogrammed channel; empty string (`""`, "vacant") is this
    /// implementation's choice, same category of open item as
    /// `MemoryChannelRecord::tag`'s default — see module docs' "KM"
    /// section for the char-set/minimum-length rules a stored message must
    /// satisfy.
    pub keyer_memories: [String; 5],
    /// `KP`'s raw P1 value, `0`-`75` (300-1050 Hz, 10 Hz steps — manual
    /// p.10). Manual gives no stated factory default; `0` (300 Hz) is this
    /// implementation's arbitrary choice.
    pub key_pitch: u8,
    /// `KR`'s electronic-keyer on/off state. Manual gives no stated
    /// factory default; `false` (OFF) is this implementation's arbitrary
    /// choice, consistent with this crate's other undocumented-default
    /// booleans (`rx_clarifier_on`, `split`, etc.).
    pub keyer_on: bool,
    /// `KS`'s key (CW) speed, WPM, legal range `4`-`60` (manual p.11).
    /// Manual gives no stated factory default; `4` (the minimum legal
    /// value) is this implementation's arbitrary choice.
    pub key_speed: u8,
    /// `CS`'s CW spot on/off state. Manual gives no stated factory
    /// default; `false` is this implementation's arbitrary choice.
    pub cw_spot_on: bool,
    /// `BI`'s break-in on/off state. Manual gives no stated factory
    /// default; `false` is this implementation's arbitrary choice.
    pub break_in_on: bool,
    /// `SD`'s CW break-in delay time, msec, legal range `30`-`3000`
    /// (manual p.16). Manual gives no stated factory default; `30` (the
    /// minimum legal value) is this implementation's arbitrary choice.
    pub cw_break_in_delay_ms: u16,

    // -- Batch 5: scan/VOX/busy (SC VX VD VG BY) -----------------------
    /// `SC`'s raw P1 value, `0`-`2` (manual p.16): `0`=Scan OFF,
    /// `1`=Scan ON (UP ward), `2`=Scan ON (DOWN ward). Raw wire digit, not
    /// the domain [`ScanState`](crate::radio_trait::ScanState) type — same
    /// "raw nibble stored, domain type only at the client boundary" pattern
    /// `mode`/`cat_tx` already use. Manual gives no stated factory default;
    /// `0` (OFF) is this implementation's choice — the one default in this
    /// batch that isn't really a judgment call, since `0` is unambiguously
    /// "scan off".
    pub scan_state: u8,
    /// `VX`'s VOX on/off state (manual p.18). Manual gives no stated
    /// factory default; `false` is this implementation's arbitrary choice.
    pub vox_on: bool,
    /// `VG`'s VOX gain, `0`-`100` (manual p.18). Manual gives no stated
    /// factory default; `0` (the minimum legal value) is this
    /// implementation's arbitrary choice.
    pub vox_gain: u8,
    /// `VD`'s VOX delay time, msec, legal range `30`-`3000` in 10 msec
    /// steps (manual p.17). **Real-world meaning depends on `EX` menu item
    /// 142 "VOX SELECT" (`MIC` vs `DATA`), which is not implemented by this
    /// crate** — see the module docs' "VD, the batch's highest-risk item"
    /// section for the full manual citation and the related-but-unimplemented
    /// menu items (142/143/144/146/147) this finding uncovered. This field
    /// is addressed unconditionally by `VD` regardless of what menu 142
    /// would (if implemented) currently select; there is no separate
    /// "DATA VOX delay" value in this emulator. Manual gives no stated
    /// factory default; `30` (the minimum legal value) is this
    /// implementation's arbitrary choice.
    pub vox_delay_ms: u16,
    /// `BY`'s RX busy status (manual p.5). This emulator has no simulated
    /// received-signal/squelch-open condition anywhere (the same gap
    /// [`Self::ri_status`]'s doc comment notes for `RI`), so this always
    /// reports `false` — a documented simplification, not a manual-
    /// specified default. `BY` is read-only (no `Set` form exists), so
    /// nothing in this crate ever writes this field.
    pub rx_busy: bool,

    // -- Batch 6: attenuator/preamp/noise/AGC/notch/filter-width -------
    /// `RA`'s attenuator on/off state (manual p.15). Manual gives no stated
    /// factory default; `false` is this implementation's arbitrary choice.
    pub attenuator_on: bool,
    /// `PA`'s raw P2 value, `0`-`2` (`0`=IPO, `1`=AMP1, `2`=AMP2 — manual
    /// p.14). Manual gives no stated factory default; `0` (IPO, i.e.
    /// pre-amp bypassed) is this implementation's arbitrary choice.
    pub preamp_mode: u8,
    /// `NB`'s noise blanker on/off state (manual p.13). Manual gives no
    /// stated factory default; `false` is this implementation's arbitrary
    /// choice.
    pub noise_blanker_on: bool,
    /// `NL`'s noise blanker level, `0`-`10` (manual p.13). Manual gives no
    /// stated factory default; `0` (the minimum legal value) is this
    /// implementation's arbitrary choice.
    pub noise_blanker_level: u8,
    /// `NR`'s noise reduction on/off state (manual p.13). Manual gives no
    /// stated factory default; `false` is this implementation's arbitrary
    /// choice.
    pub noise_reduction_on: bool,
    /// `RL`'s noise reduction level, `1`-`15` (manual p.15, no `0` — `NR`
    /// is the separate on/off gate). Manual gives no stated factory
    /// default; `1` (the minimum legal value) is this implementation's
    /// arbitrary choice.
    pub noise_reduction_level: u8,
    /// `GT`'s raw P3 (reported) value, `0`-`6` — see module docs' "GT, AGC's
    /// write/report domain mismatch" section for why this stores the wider
    /// Answer domain rather than the narrower `0`-`4` Set domain, and for
    /// the documented `P2=4`→`P3=4` (AUTO→AUTO-FAST) resolution judgment
    /// call. Manual gives no stated factory default; `0` (OFF) is this
    /// implementation's arbitrary choice.
    pub agc_mode: u8,
    /// `CO`'s `P2=0` item (CONTOUR on/off, manual p.5). Manual gives no
    /// stated factory default; `false` is this implementation's arbitrary
    /// choice.
    pub contour_on: bool,
    /// `CO`'s `P2=1` item (CONTOUR frequency, Hz, `10`-`3200` — manual p.5).
    /// Manual gives no stated factory default; `10` (the minimum legal
    /// value) is this implementation's arbitrary choice.
    pub contour_freq_hz: u16,
    /// `CO`'s `P2=2` item (APF on/off, manual p.5). Manual gives no stated
    /// factory default; `false` is this implementation's arbitrary choice.
    pub apf_on: bool,
    /// `CO`'s `P2=3` item (APF frequency, Hz, `-250`..=`250` in 10 Hz steps
    /// — manual p.5, see [`apf_raw_to_hz`]/[`apf_hz_to_raw`] for the raw
    /// wire-value mapping). `0` (center) is this implementation's choice —
    /// the one default in this batch that isn't really a judgment call,
    /// since `0` is unambiguously "no APF shift".
    pub apf_freq_hz: i16,
    /// `BP`'s `P2=0` item (Manual NOTCH on/off, manual p.5). Manual gives
    /// no stated factory default; `false` is this implementation's
    /// arbitrary choice.
    pub manual_notch_on: bool,
    /// `BP`'s `P2=1` item (Manual NOTCH frequency, Hz, `10`-`3200` in 10 Hz
    /// steps — manual p.5, "NOTCH Frequency: x 10 Hz"). Manual gives no
    /// stated factory default; `10` (the minimum legal value) is this
    /// implementation's arbitrary choice.
    pub manual_notch_freq_hz: u16,
    /// `BC`'s auto notch on/off state (manual p.4). Manual gives no stated
    /// factory default; `false` is this implementation's arbitrary choice.
    pub auto_notch_on: bool,
    /// `NA`'s narrow on/off state (manual p.13 — see module docs' "NA, a
    /// genuine manual wire-diagram typo" section for the wire-code
    /// resolution). Manual gives no stated factory default; `false` is this
    /// implementation's arbitrary choice.
    pub narrow_on: bool,
    /// `SH`'s raw P2 table index, `0`-`21` (manual p.16). The manual's own
    /// table explicitly labels row `00` "(Default)" — the one default in
    /// this batch that's directly manual-stated, not an arbitrary choice.
    pub filter_width_index: u8,

    // -- Batch 7: speech processor/mic/monitor --------------------------
    /// `MG`'s mic gain level, `0`-`100` (manual p.11). Manual gives no
    /// stated factory default; `0` (the minimum legal value) is this
    /// implementation's arbitrary choice.
    pub mic_gain: u8,
    /// `PL`'s speech processor level, `0`-`100` (manual p.14). Manual gives
    /// no stated factory default; `0` (the minimum legal value) is this
    /// implementation's arbitrary choice.
    pub speech_processor_level: u8,
    /// `PR`'s `P1=0` item (Speech Processor on/off, manual p.14 — see
    /// module docs' "PR, a genuine manual heading typo" section). Manual
    /// gives no stated factory default; `false` is this implementation's
    /// arbitrary choice.
    pub speech_processor_on: bool,
    /// `PR`'s `P1=1` item (Parametric Microphone Equalizer on/off, manual
    /// p.14). Manual gives no stated factory default; `false` is this
    /// implementation's arbitrary choice.
    pub parametric_mic_eq_on: bool,
    /// `ML`'s `P1=0` item (MONI on/off, manual p.12). Manual gives no
    /// stated factory default; `false` is this implementation's arbitrary
    /// choice.
    pub monitor_on: bool,
    /// `ML`'s `P1=1` item (MONI level, `0`-`100`, manual p.12). Manual
    /// gives no stated factory default; `0` (the minimum legal value) is
    /// this implementation's arbitrary choice.
    pub monitor_level: u8,

    // -- Batch 8: band/step/encoder front-panel controls ----------------
    /// `BS`'s current band, a raw wire value from [`BAND_CODES`] (`00`-`16`,
    /// excluding the documented gap at `13` — manual p.5). Stepped by
    /// `BU`/`BD` via [`next_band`]/[`prev_band`]. `BS` has no `Read`/
    /// `Answer` form at all (manual p.3: Set-only), so this field is
    /// inspectable only via `Ft991aRadio::state()`/`from_state` in tests.
    /// Manual gives no stated factory default; `0` (`00`, 1.8 MHz) is this
    /// implementation's arbitrary choice.
    pub selected_band: u8,
    /// `FS`'s VFO-A "FAST" step key on/off state (manual p.9). Manual gives
    /// no stated factory default; `false` is this implementation's
    /// arbitrary choice.
    pub fast_step_on: bool,

    // -- Batch 10 (last of the 10 core batches): misc system/TX/tuner/DVS --
    /// `AC`'s P3 (`0`=Tuner OFF, `1`=Tuner ON, `2`=Tuning Start/Stop, manual
    /// p.4). Manual gives no stated factory default; `0` (OFF) is this
    /// implementation's arbitrary choice.
    pub antenna_tuner_state: u8,
    /// `AI`'s auto-information on/off state (manual p.4). The manual's own
    /// note that this resets to `0` when the transceiver powers off is
    /// **not** enforced by this emulator — see module docs' "AI" section.
    /// Manual gives no stated factory default; `false` is this
    /// implementation's arbitrary choice.
    pub auto_info_on: bool,
    /// `DA`'s P2 (LED Indicators Brightness Level, `1`-`2` — manual p.6, no
    /// `0` value). Manual gives no stated factory default; `1` (the minimum
    /// legal value) is this implementation's arbitrary choice.
    pub led_brightness: u8,
    /// `DA`'s P3 (TFT Display Brightness Level, `0`-`15`). Manual gives no
    /// stated factory default; `0` (the minimum legal value) is this
    /// implementation's arbitrary choice.
    pub tft_brightness: u8,
    /// `DT`'s P1=0 date: year (4 wire digits, manual states no explicit
    /// range beyond the digit count). Manual gives no stated factory
    /// default; `0` is this implementation's arbitrary choice.
    pub date_year: u16,
    /// `DT`'s P1=0 date: month, `1`-`12` (not calendar-validated against
    /// `date_day` — see module docs' "DT" section). Manual gives no stated
    /// factory default; `1` (the minimum legal value) is this
    /// implementation's arbitrary choice.
    pub date_month: u8,
    /// `DT`'s P1=0 date: day, `1`-`31` (not leap-year/month-length
    /// validated). Manual gives no stated factory default; `1` (the minimum
    /// legal value) is this implementation's arbitrary choice.
    pub date_day: u8,
    /// `DT`'s P1=1 time (UTC): hour, 24-hour, `0`-`23`. Manual gives no
    /// stated factory default; `0` is this implementation's arbitrary
    /// choice.
    pub time_hour: u8,
    /// `DT`'s P1=1 time: minute, `0`-`59`.
    pub time_minute: u8,
    /// `DT`'s P1=1 time: second, `0`-`59`.
    pub time_second: u8,
    /// `DT`'s P1=2 time differential (time zone), signed minutes,
    /// `-720..=840` (`-12:00`..`+14:00`), 30-minute increments (manual
    /// p.6). Manual gives no stated factory default; `0` is this
    /// implementation's choice — the one default in this batch that isn't
    /// really a judgment call, since `0` is unambiguously "no offset".
    pub time_zone_offset_min: i16,
    /// `LK`'s VFO-A dial lock on/off state (manual p.11). Manual gives no
    /// stated factory default; `false` is this implementation's arbitrary
    /// choice.
    pub lock_on: bool,
    /// `FT`'s Answer-domain value (`0`=VFO-A Band Transmitter:TX,
    /// `1`=VFO-B Band Transmitter:TX — manual p.9). `Set`'s own domain is
    /// `2`/`3` for the identical two states — see module docs' "FT" section
    /// for the write/report domain mismatch this stores the Answer side of.
    /// Manual gives no stated factory default; `0` is this implementation's
    /// arbitrary choice.
    pub tx_vfo_select: u8,
    /// `TS`'s "TXW" on/off state (manual p.17 — see module docs' "TS"
    /// section for why this is not modeled as "tuning step" despite the
    /// architect's dispatch-prompt guess). Manual gives no stated factory
    /// default; `false` is this implementation's arbitrary choice.
    pub txw_on: bool,
    /// `MX`'s MOX (manual transmit) on/off state (manual p.13). Manual gives
    /// no stated factory default; `false` is this implementation's
    /// arbitrary choice.
    pub mox_on: bool,
    /// `LM`'s DVS recording state: `0`=stopped, `1`-`5`=actively recording
    /// that channel (manual p.11 — see module docs' "LM/PB" section for the
    /// per-channel toggle semantics this field's `Set` arm implements).
    /// Manual gives no stated factory default; `0` (stopped) is this
    /// implementation's choice — the one default in this batch that isn't
    /// really a judgment call, since `0` is unambiguously "not recording".
    pub dvs_recording_channel: u8,
    /// `PB`'s DVS playback state: `0`=stopped, `1`-`5`=actively playing that
    /// channel (manual p.14 — unconditional start/stop, not a toggle like
    /// `dvs_recording_channel`, see module docs). `0` (stopped) is likewise
    /// unambiguous, not a judgment call.
    pub dvs_playback_channel: u8,
}

impl Default for Ft991aState {
    fn default() -> Self {
        Self {
            vfo_a_hz: 14_000_000,
            vfo_b_hz: 14_100_000,
            mode: 0x2, // USB
            cat_tx: 0,
            af_gain: 128,
            rf_gain: 255,
            squelch: 0,
            power_control: 100,
            smeter: 0,
            power_on: true,
            if_channel: 0,
            clarifier_offset_hz: 0,
            rx_clarifier_on: false,
            tx_clarifier_on: false,
            channel_select: 0,
            tone_status: 0,
            offset_type: 0,
            meter_select: 0,
            comp_meter: 0,
            alc_meter: 0,
            po_meter: 0,
            swr_meter: 0,
            id_meter: 0,
            vdd_meter: 0,
            menu_mode: false,
            pll_unlocked: false,
            ex_am_ptt_select: 0,
            ex_am_port_select: 0,
            ex_pc_keying: 0,
            ex_data_ptt_select: 0,
            ex_data_port_select: 1,
            ex_fm_pkt_ptt_select: 0,
            ex_fm_pkt_port_select: 1,
            ex_ssb_ptt_select: 0,
            ex_ssb_port_select: 0,
            ex_agc_fast_delay: 20,
            ex_agc_mid_delay: 20,
            ex_agc_slow_delay: 20,
            ex_home_function: 0,
            ex_my_call_indication: 0,
            ex_display_color: 0,
            ex_dimmer_led: 0,
            ex_dimmer_tft: 0,
            ex_bar_mtr_peak_hold: 0,
            ex_dvs_rx_out_level: 0,
            ex_dvs_tx_out_level: 0,
            ex_keyer_type: 0,
            ex_keyer_dot_dash: 0,
            ex_cw_weight: 25,
            ex_beacon_interval: 0,
            ex_number_style: 0,
            ex_contest_number: 0,
            ex_cw_memory_1: 0,
            ex_cw_memory_2: 0,
            ex_cw_memory_3: 0,
            ex_cw_memory_4: 0,
            ex_cw_memory_5: 0,
            ex_nb_width: 0,
            ex_nb_rejection: 0,
            ex_nb_level: 0,
            ex_beep_level: 0,
            ex_gps_232c_select: 0,
            ex_rs232c_rate: 0,
            ex_rs232c_tot: 0,
            ex_cat_rate: 0,
            ex_cat_tot: 0,
            ex_cat_rts: 0,
            ex_mem_group: 0,
            ex_quick_split_freq: 0,
            ex_tx_tot: 0,
            ex_mic_scan: 0,
            ex_mic_scan_resume: 0,
            ex_ref_freq_adj: 0,
            ex_clar_mode_select: 0,
            ex_am_lcut_freq: 0,
            ex_am_lcut_slope: 0,
            ex_am_hcut_freq: 0,
            ex_am_hcut_slope: 0,
            ex_am_mic_select: 0,
            ex_am_out_level: 0,
            ex_am_data_gain: 0,
            ex_cw_lcut_freq: 0,
            ex_cw_lcut_slope: 0,
            ex_cw_hcut_freq: 0,
            ex_cw_hcut_slope: 0,
            ex_cw_out_level: 0,
            ex_cw_auto_mode: 0,
            ex_cw_bk_in_type: 0,
            ex_cw_bk_in_delay: 30,
            ex_cw_wave_shape: 0,
            ex_cw_freq_display: 0,
            ex_qsk_delay_time: 0,
            ex_data_mode: 0,
            ex_psk_tone: 0,
            ex_other_disp_ssb: 0,
            ex_other_shift_ssb: 0,
            ex_data_lcut_freq: 0,
            ex_data_lcut_slope: 0,
            ex_data_hcut_freq: 0,
            ex_data_hcut_slope: 0,
            ex_data_in_select: 0,
            ex_data_out_level: 0,
            ex_fm_mic_select: 0,
            ex_fm_out_level: 0,
            ex_fm_pkt_tx_gain: 0,
            ex_fm_pkt_mode: 0,
            ex_rpt_shift_28mhz: 0,
            ex_rpt_shift_50mhz: 0,
            ex_rpt_shift_144mhz: 0,
            ex_rpt_shift_430mhz: 0,
            ex_ars_144mhz: 0,
            ex_ars_430mhz: 0,
            ex_dcs_polarity: 0,
            ex_gm_display: 0,
            ex_distance: 0,
            ex_ams_tx_mode: 0,
            ex_standby_beep: 0,
            ex_rtty_lcut_freq: 0,
            ex_rtty_lcut_slope: 0,
            ex_rtty_hcut_freq: 0,
            ex_rtty_hcut_slope: 0,
            ex_rtty_shift_port: 0,
            ex_rtty_polarity_rx: 0,
            ex_rtty_polarity_tx: 0,
            ex_rtty_out_level: 0,
            ex_rtty_shift_freq: 0,
            ex_rtty_mark_freq: 1,
            ex_ssb_lcut_freq: 0,
            ex_ssb_lcut_slope: 0,
            ex_ssb_hcut_freq: 0,
            ex_ssb_hcut_slope: 0,
            ex_ssb_mic_select: 0,
            ex_ssb_out_level: 0,
            ex_ssb_tx_bpf: 0,
            ex_apf_width: 0,
            ex_contour_level: 0,
            ex_contour_width: 1,
            ex_if_notch_width: 0,
            ex_scp_display_mode: 0,
            ex_scp_span_freq: 3,
            ex_spectrum_color: 0,
            ex_water_fall_color: 0,
            ex_prmtrc_eq1_freq: 0,
            ex_prmtrc_eq1_level: 0,
            ex_prmtrc_eq1_bwth: 1,
            ex_prmtrc_eq2_freq: 0,
            ex_prmtrc_eq2_level: 0,
            ex_prmtrc_eq2_bwth: 1,
            ex_prmtrc_eq3_freq: 0,
            ex_prmtrc_eq3_level: 0,
            ex_prmtrc_eq3_bwth: 1,
            ex_p_prmtrc_eq1_freq: 0,
            ex_p_prmtrc_eq1_level: 0,
            ex_p_prmtrc_eq1_bwth: 1,
            ex_p_prmtrc_eq2_freq: 0,
            ex_p_prmtrc_eq2_level: 0,
            ex_p_prmtrc_eq2_bwth: 1,
            ex_p_prmtrc_eq3_freq: 0,
            ex_p_prmtrc_eq3_level: 0,
            ex_p_prmtrc_eq3_bwth: 1,
            ex_tx_max_power_hf: 5,
            ex_tx_max_power_50m: 5,
            ex_tx_max_power_144m: 5,
            ex_tx_max_power_430m: 5,
            ex_tuner_select: 0,
            ex_vox_select: 0,
            ex_vox_gain: 0,
            ex_vox_delay: 30,
            ex_anti_vox_gain: 0,
            ex_data_vox_gain: 0,
            ex_data_vox_delay: 30,
            ex_anti_dvox_gain: 0,
            ex_emergency_freq_tx: 0,
            ex_prt_wires_freq: 0,
            ex_preset_frequency: 30_000,
            ex_search_setup: 0,
            ex_wires_dg_id: 0,
            selected_memory_channel: 1,
            memory_channels: vec![MemoryChannelRecord::default(); 117],
            qmb: MemoryChannelRecord::default(),
            split: false,
            ctcss_tone_number: 0,
            dcs_code_number: 0,
            if_shift_hz: 0,
            keyer_memories: [
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ],
            key_pitch: 0,
            keyer_on: false,
            key_speed: 4,
            cw_spot_on: false,
            break_in_on: false,
            cw_break_in_delay_ms: 30,
            scan_state: 0,
            vox_on: false,
            vox_gain: 0,
            vox_delay_ms: 30,
            rx_busy: false,
            attenuator_on: false,
            preamp_mode: 0,
            noise_blanker_on: false,
            noise_blanker_level: 0,
            noise_reduction_on: false,
            noise_reduction_level: 1,
            agc_mode: 0,
            contour_on: false,
            contour_freq_hz: 10,
            apf_on: false,
            apf_freq_hz: 0,
            manual_notch_on: false,
            manual_notch_freq_hz: 10,
            auto_notch_on: false,
            narrow_on: false,
            filter_width_index: 0,
            mic_gain: 0,
            speech_processor_level: 0,
            speech_processor_on: false,
            parametric_mic_eq_on: false,
            monitor_on: false,
            monitor_level: 0,
            selected_band: 0,
            fast_step_on: false,
            antenna_tuner_state: 0,
            auto_info_on: false,
            led_brightness: 1,
            tft_brightness: 0,
            date_year: 0,
            date_month: 1,
            date_day: 1,
            time_hour: 0,
            time_minute: 0,
            time_second: 0,
            time_zone_offset_min: 0,
            lock_on: false,
            tx_vfo_select: 0,
            txw_on: false,
            mox_on: false,
            dvs_recording_channel: 0,
            dvs_playback_channel: 0,
        }
    }
}

impl Ft991aState {
    /// Resolve `RM`'s meter reading for the given `P1` selector (manual
    /// p.15, `RM`). `0` and `2` both mean "whatever meter is currently
    /// shown on the front panel", which is determined by `MS`'s current
    /// selection — resolved via [`Self::selected_meter_reading`].
    ///
    /// Callers must have already validated `selector` is in `0..=8`; other
    /// values are not legal per the manual and are rejected by
    /// `Ft991aRadio::handle_command` before this is called.
    fn meter_reading(&self, selector: u8) -> u8 {
        match selector {
            0 | 2 => self.selected_meter_reading(),
            1 => self.smeter,
            3 => self.comp_meter,
            4 => self.alc_meter,
            5 => self.po_meter,
            6 => self.swr_meter,
            7 => self.id_meter,
            8 => self.vdd_meter,
            _ => unreachable!("caller validates selector range 0..=8"),
        }
    }

    /// The meter reading currently selected by `MS` (manual p.12's P1
    /// 0-5 maps 1:1 onto COMP/ALC/PO/SWR/Id/Vd).
    fn selected_meter_reading(&self) -> u8 {
        match self.meter_select {
            0 => self.comp_meter,
            1 => self.alc_meter,
            2 => self.po_meter,
            3 => self.swr_meter,
            4 => self.id_meter,
            5 => self.vdd_meter,
            _ => unreachable!("Ms write validates meter_select range 0..=5"),
        }
    }

    /// Resolve `RI`'s status bit for the given `P1` selector (manual
    /// p.15, `RI`). This emulator has no other CAT command in any landed
    /// batch that drives VFO-A/B TX/RX, DVS REC/PLAY, Hi-SWR, or TX-LED
    /// activity, so every selector currently reports OFF (`false`) — a
    /// documented simplification, not a manual-specified default,
    /// analogous to `cat_tx`'s doc comment above (the `TX;` answer's
    /// answer-only value `2` this state machine never produces).
    fn ri_status(&self, _selector: u8) -> bool {
        false
    }

    /// Read the current raw P2 value for one of the landed `EX` menu
    /// items. Returns `i32` (not the first sub-batch's original `u8`) to
    /// accommodate the second sub-batch's signed and multi-digit-unsigned
    /// ranges (e.g. 017 "CONTEST NUMBER", 0000-9999) — the first
    /// sub-batch's 9 fields stay `u8` (all their values fit in 0-3) and
    /// are simply widened at this boundary.
    ///
    /// Callers must have already validated `p1` via
    /// [`ex_menu_item`] — any other `p1` is not a landed item and is
    /// rejected by `Ft991aRadio::handle_command` before this is called.
    fn ex_menu_value(&self, p1: u16) -> i32 {
        match p1 {
            1 => self.ex_agc_fast_delay,
            2 => self.ex_agc_mid_delay,
            3 => self.ex_agc_slow_delay,
            4 => self.ex_home_function,
            5 => self.ex_my_call_indication,
            6 => self.ex_display_color,
            7 => self.ex_dimmer_led,
            8 => self.ex_dimmer_tft,
            9 => self.ex_bar_mtr_peak_hold,
            10 => self.ex_dvs_rx_out_level,
            11 => self.ex_dvs_tx_out_level,
            12 => self.ex_keyer_type,
            13 => self.ex_keyer_dot_dash,
            14 => self.ex_cw_weight,
            15 => self.ex_beacon_interval,
            16 => self.ex_number_style,
            17 => self.ex_contest_number,
            18 => self.ex_cw_memory_1,
            19 => self.ex_cw_memory_2,
            20 => self.ex_cw_memory_3,
            21 => self.ex_cw_memory_4,
            22 => self.ex_cw_memory_5,
            23 => self.ex_nb_width,
            24 => self.ex_nb_rejection,
            25 => self.ex_nb_level,
            26 => self.ex_beep_level,
            28 => self.ex_gps_232c_select,
            29 => self.ex_rs232c_rate,
            30 => self.ex_rs232c_tot,
            31 => self.ex_cat_rate,
            32 => self.ex_cat_tot,
            33 => self.ex_cat_rts,
            34 => self.ex_mem_group,
            35 => self.ex_quick_split_freq,
            36 => self.ex_tx_tot,
            37 => self.ex_mic_scan,
            38 => self.ex_mic_scan_resume,
            39 => self.ex_ref_freq_adj,
            40 => self.ex_clar_mode_select,
            41 => self.ex_am_lcut_freq,
            42 => self.ex_am_lcut_slope,
            43 => self.ex_am_hcut_freq,
            44 => self.ex_am_hcut_slope,
            45 => self.ex_am_mic_select,
            46 => self.ex_am_out_level,
            47 => i32::from(self.ex_am_ptt_select),
            48 => i32::from(self.ex_am_port_select),
            49 => self.ex_am_data_gain,
            50 => self.ex_cw_lcut_freq,
            51 => self.ex_cw_lcut_slope,
            52 => self.ex_cw_hcut_freq,
            53 => self.ex_cw_hcut_slope,
            54 => self.ex_cw_out_level,
            55 => self.ex_cw_auto_mode,
            56 => self.ex_cw_bk_in_type,
            57 => self.ex_cw_bk_in_delay,
            58 => self.ex_cw_wave_shape,
            59 => self.ex_cw_freq_display,
            60 => i32::from(self.ex_pc_keying),
            61 => self.ex_qsk_delay_time,
            62 => self.ex_data_mode,
            63 => self.ex_psk_tone,
            64 => self.ex_other_disp_ssb,
            65 => self.ex_other_shift_ssb,
            66 => self.ex_data_lcut_freq,
            67 => self.ex_data_lcut_slope,
            68 => self.ex_data_hcut_freq,
            69 => self.ex_data_hcut_slope,
            70 => self.ex_data_in_select,
            71 => i32::from(self.ex_data_ptt_select),
            72 => i32::from(self.ex_data_port_select),
            73 => self.ex_data_out_level,
            74 => self.ex_fm_mic_select,
            75 => self.ex_fm_out_level,
            76 => i32::from(self.ex_fm_pkt_ptt_select),
            77 => i32::from(self.ex_fm_pkt_port_select),
            78 => self.ex_fm_pkt_tx_gain,
            79 => self.ex_fm_pkt_mode,
            80 => self.ex_rpt_shift_28mhz,
            81 => self.ex_rpt_shift_50mhz,
            82 => self.ex_rpt_shift_144mhz,
            83 => self.ex_rpt_shift_430mhz,
            84 => self.ex_ars_144mhz,
            85 => self.ex_ars_430mhz,
            86 => self.ex_dcs_polarity,
            88 => self.ex_gm_display,
            89 => self.ex_distance,
            90 => self.ex_ams_tx_mode,
            91 => self.ex_standby_beep,
            92 => self.ex_rtty_lcut_freq,
            93 => self.ex_rtty_lcut_slope,
            94 => self.ex_rtty_hcut_freq,
            95 => self.ex_rtty_hcut_slope,
            96 => self.ex_rtty_shift_port,
            97 => self.ex_rtty_polarity_rx,
            98 => self.ex_rtty_polarity_tx,
            99 => self.ex_rtty_out_level,
            100 => self.ex_rtty_shift_freq,
            101 => self.ex_rtty_mark_freq,
            102 => self.ex_ssb_lcut_freq,
            103 => self.ex_ssb_lcut_slope,
            104 => self.ex_ssb_hcut_freq,
            105 => self.ex_ssb_hcut_slope,
            106 => self.ex_ssb_mic_select,
            107 => self.ex_ssb_out_level,
            108 => i32::from(self.ex_ssb_ptt_select),
            109 => i32::from(self.ex_ssb_port_select),
            110 => self.ex_ssb_tx_bpf,
            111 => self.ex_apf_width,
            112 => self.ex_contour_level,
            113 => self.ex_contour_width,
            114 => self.ex_if_notch_width,
            115 => self.ex_scp_display_mode,
            116 => self.ex_scp_span_freq,
            117 => self.ex_spectrum_color,
            118 => self.ex_water_fall_color,
            119 => self.ex_prmtrc_eq1_freq,
            120 => self.ex_prmtrc_eq1_level,
            121 => self.ex_prmtrc_eq1_bwth,
            122 => self.ex_prmtrc_eq2_freq,
            123 => self.ex_prmtrc_eq2_level,
            124 => self.ex_prmtrc_eq2_bwth,
            125 => self.ex_prmtrc_eq3_freq,
            126 => self.ex_prmtrc_eq3_level,
            127 => self.ex_prmtrc_eq3_bwth,
            128 => self.ex_p_prmtrc_eq1_freq,
            129 => self.ex_p_prmtrc_eq1_level,
            130 => self.ex_p_prmtrc_eq1_bwth,
            131 => self.ex_p_prmtrc_eq2_freq,
            132 => self.ex_p_prmtrc_eq2_level,
            133 => self.ex_p_prmtrc_eq2_bwth,
            134 => self.ex_p_prmtrc_eq3_freq,
            135 => self.ex_p_prmtrc_eq3_level,
            136 => self.ex_p_prmtrc_eq3_bwth,
            137 => self.ex_tx_max_power_hf,
            138 => self.ex_tx_max_power_50m,
            139 => self.ex_tx_max_power_144m,
            140 => self.ex_tx_max_power_430m,
            141 => self.ex_tuner_select,
            142 => self.ex_vox_select,
            143 => self.ex_vox_gain,
            144 => self.ex_vox_delay,
            145 => self.ex_anti_vox_gain,
            146 => self.ex_data_vox_gain,
            147 => self.ex_data_vox_delay,
            148 => self.ex_anti_dvox_gain,
            149 => self.ex_emergency_freq_tx,
            150 => self.ex_prt_wires_freq,
            151 => self.ex_preset_frequency,
            152 => self.ex_search_setup,
            153 => self.ex_wires_dg_id,
            _ => unreachable!("caller validates p1 via ex_menu_item first"),
        }
    }

    /// Write `value` for one of the landed `EX` menu items. Same
    /// caller-validates-`p1`-first precondition as
    /// [`Self::ex_menu_value`], and the same `i32`-widened-at-the-boundary
    /// treatment for the first sub-batch's `u8` fields.
    fn set_ex_menu_value(&mut self, p1: u16, value: i32) {
        match p1 {
            1 => self.ex_agc_fast_delay = value,
            2 => self.ex_agc_mid_delay = value,
            3 => self.ex_agc_slow_delay = value,
            4 => self.ex_home_function = value,
            5 => self.ex_my_call_indication = value,
            6 => self.ex_display_color = value,
            7 => self.ex_dimmer_led = value,
            8 => self.ex_dimmer_tft = value,
            9 => self.ex_bar_mtr_peak_hold = value,
            10 => self.ex_dvs_rx_out_level = value,
            11 => self.ex_dvs_tx_out_level = value,
            12 => self.ex_keyer_type = value,
            13 => self.ex_keyer_dot_dash = value,
            14 => self.ex_cw_weight = value,
            15 => self.ex_beacon_interval = value,
            16 => self.ex_number_style = value,
            17 => self.ex_contest_number = value,
            18 => self.ex_cw_memory_1 = value,
            19 => self.ex_cw_memory_2 = value,
            20 => self.ex_cw_memory_3 = value,
            21 => self.ex_cw_memory_4 = value,
            22 => self.ex_cw_memory_5 = value,
            23 => self.ex_nb_width = value,
            24 => self.ex_nb_rejection = value,
            25 => self.ex_nb_level = value,
            26 => self.ex_beep_level = value,
            28 => self.ex_gps_232c_select = value,
            29 => self.ex_rs232c_rate = value,
            30 => self.ex_rs232c_tot = value,
            31 => self.ex_cat_rate = value,
            32 => self.ex_cat_tot = value,
            33 => self.ex_cat_rts = value,
            34 => self.ex_mem_group = value,
            35 => self.ex_quick_split_freq = value,
            36 => self.ex_tx_tot = value,
            37 => self.ex_mic_scan = value,
            38 => self.ex_mic_scan_resume = value,
            39 => self.ex_ref_freq_adj = value,
            40 => self.ex_clar_mode_select = value,
            41 => self.ex_am_lcut_freq = value,
            42 => self.ex_am_lcut_slope = value,
            43 => self.ex_am_hcut_freq = value,
            44 => self.ex_am_hcut_slope = value,
            45 => self.ex_am_mic_select = value,
            46 => self.ex_am_out_level = value,
            47 => self.ex_am_ptt_select = value as u8,
            48 => self.ex_am_port_select = value as u8,
            49 => self.ex_am_data_gain = value,
            50 => self.ex_cw_lcut_freq = value,
            51 => self.ex_cw_lcut_slope = value,
            52 => self.ex_cw_hcut_freq = value,
            53 => self.ex_cw_hcut_slope = value,
            54 => self.ex_cw_out_level = value,
            55 => self.ex_cw_auto_mode = value,
            56 => self.ex_cw_bk_in_type = value,
            57 => self.ex_cw_bk_in_delay = value,
            58 => self.ex_cw_wave_shape = value,
            59 => self.ex_cw_freq_display = value,
            60 => self.ex_pc_keying = value as u8,
            61 => self.ex_qsk_delay_time = value,
            62 => self.ex_data_mode = value,
            63 => self.ex_psk_tone = value,
            64 => self.ex_other_disp_ssb = value,
            65 => self.ex_other_shift_ssb = value,
            66 => self.ex_data_lcut_freq = value,
            67 => self.ex_data_lcut_slope = value,
            68 => self.ex_data_hcut_freq = value,
            69 => self.ex_data_hcut_slope = value,
            70 => self.ex_data_in_select = value,
            71 => self.ex_data_ptt_select = value as u8,
            72 => self.ex_data_port_select = value as u8,
            73 => self.ex_data_out_level = value,
            74 => self.ex_fm_mic_select = value,
            75 => self.ex_fm_out_level = value,
            76 => self.ex_fm_pkt_ptt_select = value as u8,
            77 => self.ex_fm_pkt_port_select = value as u8,
            78 => self.ex_fm_pkt_tx_gain = value,
            79 => self.ex_fm_pkt_mode = value,
            80 => self.ex_rpt_shift_28mhz = value,
            81 => self.ex_rpt_shift_50mhz = value,
            82 => self.ex_rpt_shift_144mhz = value,
            83 => self.ex_rpt_shift_430mhz = value,
            84 => self.ex_ars_144mhz = value,
            85 => self.ex_ars_430mhz = value,
            86 => self.ex_dcs_polarity = value,
            88 => self.ex_gm_display = value,
            89 => self.ex_distance = value,
            90 => self.ex_ams_tx_mode = value,
            91 => self.ex_standby_beep = value,
            92 => self.ex_rtty_lcut_freq = value,
            93 => self.ex_rtty_lcut_slope = value,
            94 => self.ex_rtty_hcut_freq = value,
            95 => self.ex_rtty_hcut_slope = value,
            96 => self.ex_rtty_shift_port = value,
            97 => self.ex_rtty_polarity_rx = value,
            98 => self.ex_rtty_polarity_tx = value,
            99 => self.ex_rtty_out_level = value,
            100 => self.ex_rtty_shift_freq = value,
            101 => self.ex_rtty_mark_freq = value,
            102 => self.ex_ssb_lcut_freq = value,
            103 => self.ex_ssb_lcut_slope = value,
            104 => self.ex_ssb_hcut_freq = value,
            105 => self.ex_ssb_hcut_slope = value,
            106 => self.ex_ssb_mic_select = value,
            107 => self.ex_ssb_out_level = value,
            108 => self.ex_ssb_ptt_select = value as u8,
            109 => self.ex_ssb_port_select = value as u8,
            110 => self.ex_ssb_tx_bpf = value,
            111 => self.ex_apf_width = value,
            112 => self.ex_contour_level = value,
            113 => self.ex_contour_width = value,
            114 => self.ex_if_notch_width = value,
            115 => self.ex_scp_display_mode = value,
            116 => self.ex_scp_span_freq = value,
            117 => self.ex_spectrum_color = value,
            118 => self.ex_water_fall_color = value,
            119 => self.ex_prmtrc_eq1_freq = value,
            120 => self.ex_prmtrc_eq1_level = value,
            121 => self.ex_prmtrc_eq1_bwth = value,
            122 => self.ex_prmtrc_eq2_freq = value,
            123 => self.ex_prmtrc_eq2_level = value,
            124 => self.ex_prmtrc_eq2_bwth = value,
            125 => self.ex_prmtrc_eq3_freq = value,
            126 => self.ex_prmtrc_eq3_level = value,
            127 => self.ex_prmtrc_eq3_bwth = value,
            128 => self.ex_p_prmtrc_eq1_freq = value,
            129 => self.ex_p_prmtrc_eq1_level = value,
            130 => self.ex_p_prmtrc_eq1_bwth = value,
            131 => self.ex_p_prmtrc_eq2_freq = value,
            132 => self.ex_p_prmtrc_eq2_level = value,
            133 => self.ex_p_prmtrc_eq2_bwth = value,
            134 => self.ex_p_prmtrc_eq3_freq = value,
            135 => self.ex_p_prmtrc_eq3_level = value,
            136 => self.ex_p_prmtrc_eq3_bwth = value,
            137 => self.ex_tx_max_power_hf = value,
            138 => self.ex_tx_max_power_50m = value,
            139 => self.ex_tx_max_power_144m = value,
            140 => self.ex_tx_max_power_430m = value,
            141 => self.ex_tuner_select = value,
            142 => self.ex_vox_select = value,
            143 => self.ex_vox_gain = value,
            144 => self.ex_vox_delay = value,
            145 => self.ex_anti_vox_gain = value,
            146 => self.ex_data_vox_gain = value,
            147 => self.ex_data_vox_delay = value,
            148 => self.ex_anti_dvox_gain = value,
            149 => self.ex_emergency_freq_tx = value,
            150 => self.ex_prt_wires_freq = value,
            151 => self.ex_preset_frequency = value,
            152 => self.ex_search_setup = value,
            153 => self.ex_wires_dg_id = value,
            _ => unreachable!("caller validates p1 via ex_menu_item first"),
        }
    }

    /// Access one memory channel's stored record (manual p.11-12, `MC`/
    /// `MR`/`MW`/`MT`). Callers must have already validated `channel` is in
    /// `1..=117` — any other value indexes out of bounds.
    fn memory_channel(&self, channel: u8) -> &MemoryChannelRecord {
        &self.memory_channels[(channel - 1) as usize]
    }

    /// Mutable counterpart of [`Self::memory_channel`]. Same
    /// caller-validates-range precondition.
    fn memory_channel_mut(&mut self, channel: u8) -> &mut MemoryChannelRecord {
        &mut self.memory_channels[(channel - 1) as usize]
    }

    /// Read one `KM` keyer memory channel's stored message (manual p.10).
    /// Callers must have already validated `channel` is in `1..=5`.
    fn keyer_memory(&self, channel: u8) -> &str {
        &self.keyer_memories[(channel - 1) as usize]
    }

    /// Write one `KM` keyer memory channel's stored message. Same
    /// caller-validates-range precondition as [`Self::keyer_memory`].
    fn set_keyer_memory(&mut self, channel: u8, message: String) {
        self.keyer_memories[(channel - 1) as usize] = message;
    }
}

/// Radio-specific state change event used by emulator logging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ft991aEvent {
    pub field: &'static str,
    pub value: String,
}

/// FT-991A emulator radio implementation.
#[derive(Debug, Default)]
pub struct Ft991aRadio {
    state: Ft991aState,
}

impl Ft991aRadio {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a radio state machine starting from a caller-provided state,
    /// rather than [`Ft991aState::default`]. Used by this module's tests to
    /// exercise field combinations not yet reachable via CAT commands alone
    /// (e.g. `IF`'s clarifier/select/tone/offset-type fields, none of which
    /// have a landed `Set` command yet — see `Ft991aState`'s per-field doc
    /// comments), and available to a future `emulator` crate for scripted
    /// starting scenarios.
    pub fn from_state(state: Ft991aState) -> Self {
        Self { state }
    }

    pub fn state(&self) -> &Ft991aState {
        &self.state
    }
}

impl CatCommandCatalog for Ft991aRadio {
    type CommandId = Ft991aCommandId;

    fn command_table(&self) -> &'static CommandTable<Self::CommandId> {
        &FT991A_COMMAND_TABLE
    }
}

/// Write `text` (including the trailing `;`) as a complete response and
/// return the disposition for a successfully-written (possibly `"?;"`
/// content-error) response.
fn respond(response: &mut ResponseBuilder<'_>, text: &str) -> ResponseDisposition {
    response
        .write_complete(text)
        .expect("response write cannot fail before finish");
    ResponseDisposition::ResponseWritten
}

impl CatRadio for Ft991aRadio {
    type Event = Ft991aEvent;
    type Error = Infallible;

    fn handle_command(
        &mut self,
        request: CommandRequest<'_, Self::CommandId>,
        response: &mut ResponseBuilder<'_>,
    ) -> Result<CommandOutcome<Self::Event>, Self::Error> {
        use Ft991aCommandId::*;

        let mut events = Vec::new();
        let params = request.parameters.raw();

        let disposition = match request.id {
            Fa => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("FA{:09};", self.state.vfo_a_hz))
                }
                CommandOperation::Set => match params.parse::<u64>() {
                    Ok(hz) if (30_000..=470_000_000).contains(&hz) => {
                        self.state.vfo_a_hz = hz;
                        events.push(Ft991aEvent {
                            field: "vfo_a_hz",
                            value: hz.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },
            Fb => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("FB{:09};", self.state.vfo_b_hz))
                }
                CommandOperation::Set => match params.parse::<u64>() {
                    Ok(hz) if (30_000..=470_000_000).contains(&hz) => {
                        self.state.vfo_b_hz = hz;
                        events.push(Ft991aEvent {
                            field: "vfo_b_hz",
                            value: hz.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },
            Md => match params.len() {
                1 if params == "0" => respond(response, &format!("MD0{:X};", self.state.mode)),
                2 if params.starts_with('0') => {
                    match params[1..2].chars().next().and_then(|c| c.to_digit(16)) {
                        Some(v) if (1..=0xE).contains(&v) => {
                            self.state.mode = v as u8;
                            events.push(Ft991aEvent {
                                field: "mode",
                                value: format!("{:X}", v),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },
            Tx => match request.operation {
                CommandOperation::Query => respond(response, &format!("TX{};", self.state.cat_tx)),
                CommandOperation::Set => match params {
                    "0" | "1" => {
                        self.state.cat_tx = params.parse().expect("validated digit");
                        events.push(Ft991aEvent {
                            field: "cat_tx",
                            value: params.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },
            Sm => {
                if params == "0" {
                    respond(response, &format!("SM0{:03};", self.state.smeter))
                } else {
                    respond(response, "?;")
                }
            }
            Ps => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("PS{};", u8::from(self.state.power_on)))
                }
                CommandOperation::Set => match params {
                    "0" | "1" => {
                        self.state.power_on = params == "1";
                        events.push(Ft991aEvent {
                            field: "power_on",
                            value: params.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },
            Ag => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("AG0{:03};", self.state.af_gain))
                }
                CommandOperation::Set => match parse_selector_level(params, 255) {
                    Some(level) => {
                        self.state.af_gain = level;
                        events.push(Ft991aEvent {
                            field: "af_gain",
                            value: level.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    None => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },
            Rg => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("RG0{:03};", self.state.rf_gain))
                }
                CommandOperation::Set => match parse_selector_level(params, 255) {
                    Some(level) => {
                        self.state.rf_gain = level;
                        events.push(Ft991aEvent {
                            field: "rf_gain",
                            value: level.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    None => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },
            Sq => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("SQ0{:03};", self.state.squelch))
                }
                CommandOperation::Set => match parse_selector_level(params, 100) {
                    Some(level) => {
                        self.state.squelch = level;
                        events.push(Ft991aEvent {
                            field: "squelch",
                            value: level.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    None => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },
            Pc => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("PC{:03};", self.state.power_control))
                }
                CommandOperation::Set => match params.parse::<u16>() {
                    Ok(watts) if (5..=100).contains(&watts) => {
                        self.state.power_control = watts as u8;
                        events.push(Ft991aEvent {
                            field: "power_control",
                            value: watts.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },
            Id => respond(response, &format!("ID{};", FT991A_ID)),

            If => {
                let payload = ChannelStatusFields {
                    channel: self.state.if_channel,
                    frequency_hz: self.state.vfo_a_hz,
                    clarifier_offset_hz: self.state.clarifier_offset_hz,
                    rx_clarifier_on: self.state.rx_clarifier_on,
                    tx_clarifier_on: self.state.tx_clarifier_on,
                    mode: self.state.mode,
                    select: self.state.channel_select,
                    tone_status: self.state.tone_status,
                    offset_type: self.state.offset_type,
                };
                respond(response, &format!("IF{};", payload.to_wire_string()))
            }

            // Selector read (structurally a `Set` to the parser — see
            // `Sm`'s comment above): `params` is guaranteed exactly 1 char
            // by `SET_1`.
            Rm => match params.parse::<u8>() {
                Ok(selector) if selector <= 8 => {
                    let level = self.state.meter_reading(selector);
                    respond(response, &format!("RM{selector}{level:03};"))
                }
                _ => respond(response, "?;"),
            },

            // Selector read, but the legal selector set has a documented
            // gap (0, 3-7, A — 1/2/8/9/B-F are not listed in the manual's
            // P1 legend, transcribed exactly rather than guessed).
            Ri => match params.chars().next().and_then(|c| c.to_digit(16)) {
                Some(v) if matches!(v, 0 | 3 | 4 | 5 | 6 | 7 | 0xA) => {
                    let status = self.state.ri_status(v as u8);
                    respond(response, &format!("RI{:X}{};", v, u8::from(status)))
                }
                _ => respond(response, "?;"),
            },

            Rs => respond(response, &format!("RS{};", u8::from(self.state.menu_mode))),

            Ms => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("MS{};", self.state.meter_select))
                }
                CommandOperation::Set => match params.parse::<u8>() {
                    Ok(v) if v <= 5 => {
                        self.state.meter_select = v;
                        events.push(Ft991aEvent {
                            field: "meter_select",
                            value: v.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            Ul => respond(
                response,
                &format!("UL{};", u8::from(self.state.pll_unlocked)),
            ),

            // EX menu. `params` is guaranteed by EX_SET_FORMS to be one of
            // {3,4,5,6,7,8,11} chars: 3 is the selector-only read (P1
            // alone, same "selector read" shape as Md/Rm/Ri above); the
            // rest are writes, P1 (3 chars) + P2 (params.len()-3 chars).
            // Any P1 not in EX_MENU_TABLE, or any P2 whose width/value
            // doesn't match that item's own registered digits/kind,
            // cleanly falls through to "?;" — never a panic
            // (ex_menu_value/set_ex_menu_value are only called once
            // ex_menu_item(p1) has confirmed p1 is a landed item).
            Ex => {
                let p1_str = &params[0..3];
                match p1_str.parse::<u16>().ok().and_then(ex_menu_item) {
                    Some(item) if params.len() == 3 => {
                        let value = self.state.ex_menu_value(item.p1);
                        let wire = item.kind.format(value, item.digits);
                        respond(response, &format!("EX{p1_str}{wire};"))
                    }
                    Some(item) => {
                        let p2 = &params[3..];
                        let value = if p2.len() == item.digits {
                            item.kind.parse(p2)
                        } else {
                            None
                        };
                        match value {
                            Some(value) => {
                                self.state.set_ex_menu_value(item.p1, value);
                                events.push(Ft991aEvent {
                                    field: "ex_menu",
                                    value: format!("{}={}", item.p1, value),
                                });
                                ResponseDisposition::NoResponse
                            }
                            None => respond(response, "?;"),
                        }
                    }
                    None => respond(response, "?;"),
                }
            }

            // Batch 2: memory channel records.
            Mc => match request.operation {
                CommandOperation::Query => respond(
                    response,
                    &format!("MC{:03};", self.state.selected_memory_channel),
                ),
                CommandOperation::Set => match params.parse::<u8>() {
                    Ok(ch) if (1..=117).contains(&ch) => {
                        self.state.selected_memory_channel = ch;
                        events.push(Ft991aEvent {
                            field: "selected_memory_channel",
                            value: ch.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // `MR` is a selector read (structurally a `Set` to the
            // parser, same treatment as `Sm`/`Rm`/`Ri` above): `params` is
            // guaranteed exactly 3 chars by `SET_3`.
            Mr => match params.parse::<u8>() {
                Ok(ch) if (1..=117).contains(&ch) => {
                    let record = self.state.memory_channel(ch);
                    let payload = ChannelStatusFields {
                        channel: ch,
                        frequency_hz: record.frequency_hz,
                        clarifier_offset_hz: record.clarifier_offset_hz,
                        rx_clarifier_on: record.rx_clarifier_on,
                        tx_clarifier_on: record.tx_clarifier_on,
                        mode: record.mode,
                        // See MemoryChannelRecord's doc comment: MR only
                        // ever addresses a real memory channel, so this
                        // emulator always reports Memory (1) here.
                        select: 1,
                        tone_status: record.tone_status,
                        offset_type: record.offset_type,
                    };
                    respond(response, &format!("MR{};", payload.to_wire_string()))
                }
                _ => respond(response, "?;"),
            },

            // `MW` is write-only (manual p.3: Read=X, Ans=X) — a single
            // 25-byte `ChannelStatusFields`-shaped body, `params`
            // guaranteed that width by `SET_25`. `channel >= 1` and
            // `select == 0` are extra semantic checks on top of
            // `ChannelStatusFields::parse`'s own (wider, `IF`-shared)
            // structural validation — see MemoryChannelRecord's doc
            // comment.
            Mw => match ChannelStatusFields::parse(params) {
                Some(fields) if fields.channel >= 1 && fields.select == 0 => {
                    let record = self.state.memory_channel_mut(fields.channel);
                    record.frequency_hz = fields.frequency_hz;
                    record.clarifier_offset_hz = fields.clarifier_offset_hz;
                    record.rx_clarifier_on = fields.rx_clarifier_on;
                    record.tx_clarifier_on = fields.tx_clarifier_on;
                    record.mode = fields.mode;
                    record.tone_status = fields.tone_status;
                    record.offset_type = fields.offset_type;
                    events.push(Ft991aEvent {
                        field: "memory_channel_write",
                        value: fields.channel.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            // `MT`: selector read (3 chars, same shape as `Mr` above) or a
            // 38-byte write body (`ChannelStatusFields`'s 25 bytes + 1
            // reserved byte + 12-byte tag) — `params` is guaranteed one of
            // these two widths by `MT_SET_FORMS`.
            Mt => match params.len() {
                3 => match params.parse::<u8>() {
                    Ok(ch) if (1..=117).contains(&ch) => {
                        let record = self.state.memory_channel(ch);
                        let fields = ChannelStatusFields {
                            channel: ch,
                            frequency_hz: record.frequency_hz,
                            clarifier_offset_hz: record.clarifier_offset_hz,
                            rx_clarifier_on: record.rx_clarifier_on,
                            tx_clarifier_on: record.tx_clarifier_on,
                            mode: record.mode,
                            select: 1, // see Mr's comment above
                            tone_status: record.tone_status,
                            offset_type: record.offset_type,
                        };
                        respond(
                            response,
                            &format!("MT{}0{:<12};", fields.to_wire_string(), record.tag),
                        )
                    }
                    _ => respond(response, "?;"),
                },
                38 => {
                    // `params` is guaranteed exactly 38 *bytes* by
                    // `MT_SET_FORMS` (`CommandForm`'s width check is
                    // byte-length, not char count) — using `.get()` rather
                    // than direct byte-range indexing avoids a panic if
                    // stray multi-byte UTF-8 content ever lands off a char
                    // boundary (the same defensive style
                    // `ChannelStatusFields::parse` already uses). Layout:
                    // [0..25) the ChannelStatusFields body, [25..26) P11
                    // (reserved, unvalidated — see MemoryChannelRecord's
                    // doc comment), [26..38) the 12-byte P12 tag.
                    let fields_body = params.get(0..ChannelStatusFields::WIRE_WIDTH);
                    let raw_tag = params.get(ChannelStatusFields::WIRE_WIDTH + 1..38);
                    match (fields_body.and_then(ChannelStatusFields::parse), raw_tag) {
                        (Some(fields), Some(raw_tag))
                            if fields.channel >= 1
                                && fields.select == 0
                                && is_valid_ascii_wire_content(raw_tag) =>
                        {
                            let tag = raw_tag.trim_end_matches(' ').to_string();
                            let record = self.state.memory_channel_mut(fields.channel);
                            record.frequency_hz = fields.frequency_hz;
                            record.clarifier_offset_hz = fields.clarifier_offset_hz;
                            record.rx_clarifier_on = fields.rx_clarifier_on;
                            record.tx_clarifier_on = fields.tx_clarifier_on;
                            record.mode = fields.mode;
                            record.tone_status = fields.tone_status;
                            record.offset_type = fields.offset_type;
                            record.tag = tag;
                            events.push(Ft991aEvent {
                                field: "memory_channel_tag_write",
                                value: fields.channel.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },

            // Batch 1: VFO/split/memory quick-ops. See module docs for the
            // full manual citations, the VM/AM heading-inconsistency
            // resolution, and the judgment calls (CH wrap-around, QI/QR's
            // dedicated QMB slot, QS's toggle semantics).
            Ab => match request.operation {
                CommandOperation::Action => {
                    self.state.vfo_b_hz = self.state.vfo_a_hz;
                    events.push(Ft991aEvent {
                        field: "vfo_b_hz",
                        value: self.state.vfo_b_hz.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            Ba => match request.operation {
                CommandOperation::Action => {
                    self.state.vfo_a_hz = self.state.vfo_b_hz;
                    events.push(Ft991aEvent {
                        field: "vfo_a_hz",
                        value: self.state.vfo_a_hz.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            Am => match request.operation {
                CommandOperation::Action => {
                    let ch = self.state.selected_memory_channel;
                    let vfo_a_hz = self.state.vfo_a_hz;
                    let mode = self.state.mode;
                    let clarifier_offset_hz = self.state.clarifier_offset_hz;
                    let rx_clarifier_on = self.state.rx_clarifier_on;
                    let tx_clarifier_on = self.state.tx_clarifier_on;
                    let tone_status = self.state.tone_status;
                    let offset_type = self.state.offset_type;
                    let record = self.state.memory_channel_mut(ch);
                    record.frequency_hz = vfo_a_hz;
                    record.mode = mode;
                    record.clarifier_offset_hz = clarifier_offset_hz;
                    record.rx_clarifier_on = rx_clarifier_on;
                    record.tx_clarifier_on = tx_clarifier_on;
                    record.tone_status = tone_status;
                    record.offset_type = offset_type;
                    events.push(Ft991aEvent {
                        field: "memory_channel_store_from_vfo_a",
                        value: ch.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            // See module docs' "VM/AM manual heading inconsistency" section
            // for the full citation and reasoning. Toggles channel_select
            // between 0 (VFO) and 1 (Memory) rather than duplicating Am's
            // store behavior.
            Vm => match request.operation {
                CommandOperation::Action => {
                    self.state.channel_select = if self.state.channel_select == 0 { 1 } else { 0 };
                    events.push(Ft991aEvent {
                        field: "channel_select",
                        value: self.state.channel_select.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            Ma => match request.operation {
                CommandOperation::Action => {
                    let ch = self.state.selected_memory_channel;
                    let record = self.state.memory_channel(ch).clone();
                    self.state.vfo_a_hz = record.frequency_hz;
                    self.state.mode = record.mode;
                    self.state.clarifier_offset_hz = record.clarifier_offset_hz;
                    self.state.rx_clarifier_on = record.rx_clarifier_on;
                    self.state.tx_clarifier_on = record.tx_clarifier_on;
                    self.state.tone_status = record.tone_status;
                    self.state.offset_type = record.offset_type;
                    events.push(Ft991aEvent {
                        field: "vfo_a_recalled_from_memory_channel",
                        value: ch.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            // Steps MC's selected memory channel, wrapping at the 1..=117
            // boundary (manual gives no boundary behavior — documented
            // judgment call, see module docs).
            Ch => match params {
                "0" => {
                    // Memory Channel "UP"
                    self.state.selected_memory_channel =
                        if self.state.selected_memory_channel >= 117 {
                            1
                        } else {
                            self.state.selected_memory_channel + 1
                        };
                    events.push(Ft991aEvent {
                        field: "selected_memory_channel",
                        value: self.state.selected_memory_channel.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                "1" => {
                    // Memory Channel "DOWN"
                    self.state.selected_memory_channel = if self.state.selected_memory_channel <= 1
                    {
                        117
                    } else {
                        self.state.selected_memory_channel - 1
                    };
                    events.push(Ft991aEvent {
                        field: "selected_memory_channel",
                        value: self.state.selected_memory_channel.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            Qi => match request.operation {
                CommandOperation::Action => {
                    self.state.qmb = MemoryChannelRecord {
                        frequency_hz: self.state.vfo_a_hz,
                        clarifier_offset_hz: self.state.clarifier_offset_hz,
                        rx_clarifier_on: self.state.rx_clarifier_on,
                        tx_clarifier_on: self.state.tx_clarifier_on,
                        mode: self.state.mode,
                        tone_status: self.state.tone_status,
                        offset_type: self.state.offset_type,
                        tag: String::new(),
                    };
                    events.push(Ft991aEvent {
                        field: "qmb_store",
                        value: self.state.vfo_a_hz.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            Qr => match request.operation {
                CommandOperation::Action => {
                    let record = self.state.qmb.clone();
                    self.state.vfo_a_hz = record.frequency_hz;
                    self.state.mode = record.mode;
                    self.state.clarifier_offset_hz = record.clarifier_offset_hz;
                    self.state.rx_clarifier_on = record.rx_clarifier_on;
                    self.state.tx_clarifier_on = record.tx_clarifier_on;
                    self.state.tone_status = record.tone_status;
                    self.state.offset_type = record.offset_type;
                    events.push(Ft991aEvent {
                        field: "qmb_recall",
                        value: self.state.vfo_a_hz.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            Qs => match request.operation {
                CommandOperation::Action => {
                    self.state.split = !self.state.split;
                    events.push(Ft991aEvent {
                        field: "split",
                        value: self.state.split.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            Sv => match request.operation {
                CommandOperation::Action => {
                    std::mem::swap(&mut self.state.vfo_a_hz, &mut self.state.vfo_b_hz);
                    events.push(Ft991aEvent {
                        field: "vfo_swap",
                        value: format!("{}/{}", self.state.vfo_a_hz, self.state.vfo_b_hz),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            // -- Batch 3: clarifier/RIT-XIT + tone + IF-shift ---------------
            //
            // RT/XT gate the single shared `clarifier_offset_hz` value's
            // effect on RX/TX independently (see module docs' "RX/TX
            // clarifier relationship" section) — plain query/set, same shape
            // as `Tx`/`Ps`.
            Rt => match request.operation {
                CommandOperation::Query => respond(
                    response,
                    &format!("RT{};", u8::from(self.state.rx_clarifier_on)),
                ),
                CommandOperation::Set => match params {
                    "0" | "1" => {
                        self.state.rx_clarifier_on = params == "1";
                        events.push(Ft991aEvent {
                            field: "rx_clarifier_on",
                            value: params.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            Xt => match request.operation {
                CommandOperation::Query => respond(
                    response,
                    &format!("XT{};", u8::from(self.state.tx_clarifier_on)),
                ),
                CommandOperation::Set => match params {
                    "0" | "1" => {
                        self.state.tx_clarifier_on = params == "1";
                        events.push(Ft991aEvent {
                            field: "tx_clarifier_on",
                            value: params.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // Zeroes the shared clarifier offset only — RT/XT's on/off gates
            // are left untouched (module docs' "RC" section, a documented
            // judgment call).
            Rc => match request.operation {
                CommandOperation::Action => {
                    self.state.clarifier_offset_hz = 0;
                    events.push(Ft991aEvent {
                        field: "clarifier_offset_hz",
                        value: "0".to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            // Write-only, no `request.operation` match needed (same
            // established pattern as `Mw` above) — RD_SET_FORMS/RU's SET_4
            // guarantees `params` is exactly 4 digits. Absolute set, not an
            // incremental step — see module docs' "RD/RU's direction
            // encoding" section.
            Rd => match params.parse::<u16>() {
                Ok(mag) if mag <= 9999 => {
                    self.state.clarifier_offset_hz = -(mag as i16);
                    events.push(Ft991aEvent {
                        field: "clarifier_offset_hz",
                        value: self.state.clarifier_offset_hz.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            Ru => match params.parse::<u16>() {
                Ok(mag) if mag <= 9999 => {
                    self.state.clarifier_offset_hz = mag as i16;
                    events.push(Ft991aEvent {
                        field: "clarifier_offset_hz",
                        value: self.state.clarifier_offset_hz.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            // `CT`: selector read (1 char, same shape as `Md`) or a 2-char
            // write, reusing the already-landed `tone_status` field (see
            // module docs' "CT, a genuine selector-read" section).
            Ct => match params.len() {
                1 if params == "0" => respond(response, &format!("CT0{};", self.state.tone_status)),
                2 if params.starts_with('0') => match params[1..2].parse::<u8>() {
                    Ok(v) if v <= 4 => {
                        self.state.tone_status = v;
                        events.push(Ft991aEvent {
                            field: "tone_status",
                            value: v.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // `CN`: selector read (2 chars: fixed P1 + table selector P2) or
            // a 5-char write (+ 3-digit P3 table index) — see module docs'
            // "CN, the two lookup tables" section. `CN_SET_FORMS` guarantees
            // `params` is one of these two widths.
            Cn => match params.len() {
                2 if params.starts_with('0') => match params.get(1..2) {
                    Some("0") => respond(
                        response,
                        &format!("CN00{:03};", self.state.ctcss_tone_number),
                    ),
                    Some("1") => {
                        respond(response, &format!("CN01{:03};", self.state.dcs_code_number))
                    }
                    _ => respond(response, "?;"),
                },
                5 if params.starts_with('0') => {
                    let table = params.get(1..2);
                    let index: Option<u8> = params.get(2..5).and_then(|s| s.parse().ok());
                    match (table, index) {
                        (Some("0"), Some(v)) if v <= 49 => {
                            self.state.ctcss_tone_number = v;
                            events.push(Ft991aEvent {
                                field: "ctcss_tone_number",
                                value: v.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        (Some("1"), Some(v)) if v <= 103 => {
                            self.state.dcs_code_number = v;
                            events.push(Ft991aEvent {
                                field: "dcs_code_number",
                                value: v.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },

            // `IS`: selector read (1 char) or a 6-char write (fixed P1 +
            // sign + 4-digit magnitude — see module docs' "IS, a resolved
            // manual discrepancy" section for the 3-vs-4-digit resolution).
            // `-1200~+1200 Hz (20 Hz steps)` is enforced literally.
            Is => match params.len() {
                1 if params == "0" => {
                    let sign = if self.state.if_shift_hz < 0 { '-' } else { '+' };
                    respond(
                        response,
                        &format!("IS0{sign}{:04};", self.state.if_shift_hz.abs()),
                    )
                }
                6 if params.starts_with('0') => {
                    let sign = params.get(1..2);
                    let magnitude: Option<i16> = params.get(2..6).and_then(|s| s.parse().ok());
                    match (sign, magnitude) {
                        (Some("+"), Some(m)) if m <= 1200 && m % 20 == 0 => {
                            self.state.if_shift_hz = m;
                            events.push(Ft991aEvent {
                                field: "if_shift_hz",
                                value: m.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        (Some("-"), Some(m)) if m <= 1200 && m % 20 == 0 => {
                            self.state.if_shift_hz = -m;
                            events.push(Ft991aEvent {
                                field: "if_shift_hz",
                                value: (-m).to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },

            // -- Batch 4: keyer/CW/break-in ----------------------------------
            //
            // KM: selector read (1 char, P1 channel 1-5) or a variable-width
            // write (P1 + 1-50 message chars, total 2-51) — params is
            // guaranteed one of these two shapes by KM_SET_FORMS. See
            // module docs' "KM" section for the read-vs-write
            // disambiguation and the message character-set rule.
            Km => {
                if params.len() == 1 {
                    match params.parse::<u8>() {
                        Ok(ch) if (1..=5).contains(&ch) => {
                            let message = self.state.keyer_memory(ch);
                            respond(response, &format!("KM{ch}{message};"))
                        }
                        _ => respond(response, "?;"),
                    }
                } else {
                    let p1 = &params[0..1];
                    let p2 = &params[1..];
                    match p1.parse::<u8>() {
                        Ok(ch) if (1..=5).contains(&ch) && is_valid_ascii_wire_content(p2) => {
                            self.state.set_keyer_memory(ch, p2.to_string());
                            events.push(Ft991aEvent {
                                field: "keyer_memory",
                                value: format!("{ch}={p2}"),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
            }

            Kp => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("KP{:02};", self.state.key_pitch))
                }
                CommandOperation::Set => match params.parse::<u8>() {
                    Ok(v) if v <= 75 => {
                        self.state.key_pitch = v;
                        events.push(Ft991aEvent {
                            field: "key_pitch",
                            value: v.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            Kr => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("KR{};", u8::from(self.state.keyer_on)))
                }
                CommandOperation::Set => match params {
                    "0" | "1" => {
                        self.state.keyer_on = params == "1";
                        events.push(Ft991aEvent {
                            field: "keyer_on",
                            value: params.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            Ks => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("KS{:03};", self.state.key_speed))
                }
                CommandOperation::Set => match params.parse::<u8>() {
                    Ok(v) if (4..=60).contains(&v) => {
                        self.state.key_speed = v;
                        events.push(Ft991aEvent {
                            field: "key_speed",
                            value: v.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // Write-only trigger (manual p.3: Read=X Ans=X). Mutates no
            // `Ft991aState` field — see module docs' "KY" section for why
            // (no simulated audio/RF playback exists in this emulator to
            // represent); success is observable only via the pushed event.
            Ky => match request.operation {
                CommandOperation::Set => {
                    match params.chars().next().and_then(ky_selector_from_wire) {
                        Some((channel, mode)) => {
                            events.push(Ft991aEvent {
                                field: "keyer_playback",
                                value: format!("{channel}:{mode:?}"),
                            });
                            ResponseDisposition::NoResponse
                        }
                        None => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },

            Cs => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("CS{};", u8::from(self.state.cw_spot_on)))
                }
                CommandOperation::Set => match params {
                    "0" | "1" => {
                        self.state.cw_spot_on = params == "1";
                        events.push(Ft991aEvent {
                            field: "cw_spot_on",
                            value: params.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // Zero-width Action trigger, "(CW AUTO ZERO IN Function)"
            // (manual p.18). Mutates no `Ft991aState` field — see module
            // docs' "ZI" section (no simulated received-signal frequency
            // exists in this emulator to zero-beat against).
            Zi => match request.operation {
                CommandOperation::Action => {
                    events.push(Ft991aEvent {
                        field: "zero_in",
                        value: "triggered".to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            Bi => match request.operation {
                CommandOperation::Query => respond(
                    response,
                    &format!("BI{};", u8::from(self.state.break_in_on)),
                ),
                CommandOperation::Set => match params {
                    "0" | "1" => {
                        self.state.break_in_on = params == "1";
                        events.push(Ft991aEvent {
                            field: "break_in_on",
                            value: params.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            Sd => match request.operation {
                CommandOperation::Query => respond(
                    response,
                    &format!("SD{:04};", self.state.cw_break_in_delay_ms),
                ),
                CommandOperation::Set => match params.parse::<u16>() {
                    Ok(v) if (30..=3000).contains(&v) => {
                        self.state.cw_break_in_delay_ms = v;
                        events.push(Ft991aEvent {
                            field: "cw_break_in_delay_ms",
                            value: v.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // Batch 5: scan/VOX/busy. `Sc` is a plain three-valued Set/Query
            // pair (all three values legally settable, unlike `TX`'s
            // answer-only `2` — see module docs' "SC" section). `Vx`/`Vg`
            // are plain query/set pairs like `Bi`/`Ks` above. `Vd` shares
            // the same shape but its meaning depends on the not-yet-
            // implemented `EX` menu item 142 — see module docs and
            // `Ft991aState::vox_delay_ms`'s doc comment. `By` is read-only
            // (manual p.3: Set X Read O Ans O), always reporting `rx_busy`
            // (always `false` — no simulated received-signal condition
            // exists in this emulator) plus the fixed P2 byte.
            Sc => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("SC{};", self.state.scan_state))
                }
                CommandOperation::Set => match params {
                    "0" | "1" | "2" => {
                        self.state.scan_state = params.parse().expect("validated digit");
                        events.push(Ft991aEvent {
                            field: "scan_state",
                            value: params.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            Vx => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("VX{};", u8::from(self.state.vox_on)))
                }
                CommandOperation::Set => match params {
                    "0" | "1" => {
                        self.state.vox_on = params == "1";
                        events.push(Ft991aEvent {
                            field: "vox_on",
                            value: params.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // See module docs' "VD, the batch's highest-risk item" section:
            // this field's real-world meaning (VOX delay vs. DATA VOX
            // delay) depends on `EX` menu item 142, not implemented here.
            // This state machine addresses one shared value unconditionally.
            Vd => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("VD{:04};", self.state.vox_delay_ms))
                }
                CommandOperation::Set => match params.parse::<u16>() {
                    Ok(v) if (30..=3000).contains(&v) && v % 10 == 0 => {
                        self.state.vox_delay_ms = v;
                        events.push(Ft991aEvent {
                            field: "vox_delay_ms",
                            value: v.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            Vg => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("VG{:03};", self.state.vox_gain))
                }
                CommandOperation::Set => match params.parse::<u8>() {
                    Ok(v) if v <= 100 => {
                        self.state.vox_gain = v;
                        events.push(Ft991aEvent {
                            field: "vox_gain",
                            value: v.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // Read-only (manual p.3: Set X Read O Ans O) — no Set arm at
            // all. P2 is a fixed `0` byte (manual p.5), not a meaningful
            // reported value.
            By => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("BY{}0;", u8::from(self.state.rx_busy)))
                }
                _ => respond(response, "?;"),
            },

            // -- Batch 6: attenuator/preamp/noise/AGC/notch/filter-width ---
            //
            // `RA`/`NB`/`NR`/`BC`/`NA` are all the same "selector read"
            // shape as `CT` (1-char read carrying a fixed `P1=0`, 2-char
            // write) — manual p.4, p.13 (x2), p.15.
            Ra => match params.len() {
                1 if params == "0" => respond(
                    response,
                    &format!("RA0{};", u8::from(self.state.attenuator_on)),
                ),
                2 if params.starts_with('0') => match params[1..2].parse::<u8>() {
                    Ok(v) if v <= 1 => {
                        self.state.attenuator_on = v == 1;
                        events.push(Ft991aEvent {
                            field: "attenuator_on",
                            value: v.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // `PA`: same selector-read shape as `RA`, but P2 is 3-valued
            // (0=IPO, 1=AMP1, 2=AMP2 — manual p.14).
            Pa => match params.len() {
                1 if params == "0" => respond(response, &format!("PA0{};", self.state.preamp_mode)),
                2 if params.starts_with('0') => match params[1..2].parse::<u8>() {
                    Ok(v) if v <= 2 => {
                        self.state.preamp_mode = v;
                        events.push(Ft991aEvent {
                            field: "preamp_mode",
                            value: v.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            Nb => match params.len() {
                1 if params == "0" => respond(
                    response,
                    &format!("NB0{};", u8::from(self.state.noise_blanker_on)),
                ),
                2 if params.starts_with('0') => match params[1..2].parse::<u8>() {
                    Ok(v) if v <= 1 => {
                        self.state.noise_blanker_on = v == 1;
                        events.push(Ft991aEvent {
                            field: "noise_blanker_on",
                            value: v.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // `NL`: same selector-read shape, but P2 is a 3-digit level,
            // 000-010 (manual p.13).
            Nl => match params.len() {
                1 if params == "0" => respond(
                    response,
                    &format!("NL0{:03};", self.state.noise_blanker_level),
                ),
                4 if params.starts_with('0') => {
                    match params.get(1..4).and_then(|s| s.parse::<u8>().ok()) {
                        Some(v) if v <= 10 => {
                            self.state.noise_blanker_level = v;
                            events.push(Ft991aEvent {
                                field: "noise_blanker_level",
                                value: v.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },

            Nr => match params.len() {
                1 if params == "0" => respond(
                    response,
                    &format!("NR0{};", u8::from(self.state.noise_reduction_on)),
                ),
                2 if params.starts_with('0') => match params[1..2].parse::<u8>() {
                    Ok(v) if v <= 1 => {
                        self.state.noise_reduction_on = v == 1;
                        events.push(Ft991aEvent {
                            field: "noise_reduction_on",
                            value: v.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // `RL`: same selector-read shape as `NL`, but P2 is a 2-digit
            // level, 01-15 (manual p.15; `0` is not legal — `NR` is the
            // separate on/off gate).
            Rl => match params.len() {
                1 if params == "0" => respond(
                    response,
                    &format!("RL0{:02};", self.state.noise_reduction_level),
                ),
                3 if params.starts_with('0') => {
                    match params.get(1..3).and_then(|s| s.parse::<u8>().ok()) {
                        Some(v) if (1..=15).contains(&v) => {
                            self.state.noise_reduction_level = v;
                            events.push(Ft991aEvent {
                                field: "noise_reduction_level",
                                value: v.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },

            // `GT`: selector read (1 char) / 2-char write (P2, 0-4) — the
            // Answer domain (P3, 0-6) is wider than what Set can express
            // (module docs' "GT" section). `agc_mode` stores the full P3
            // domain directly; P2 0-3 map onto P3 0-3 verbatim, and P2=4
            // ("AUTO") stores as P3=4 ("AUTO-FAST") — a documented judgment
            // call, not a manual fact.
            Gt => match params.len() {
                1 if params == "0" => respond(response, &format!("GT0{};", self.state.agc_mode)),
                2 if params.starts_with('0') => match params[1..2].parse::<u8>() {
                    Ok(v) if v <= 4 => {
                        self.state.agc_mode = v;
                        events.push(Ft991aEvent {
                            field: "agc_mode",
                            value: v.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // `CO`: 4-item selector — CONTOUR ON/OFF (P2=0), CONTOUR FREQ
            // (P2=1), APF ON/OFF (P2=2), APF FREQ (P2=3). Read is 2 chars
            // (fixed P1 + P2 selector); write is 6 chars (+ 4-digit P3,
            // meaning depends on P2) — manual p.5, module docs' "CO"
            // section.
            Co => match params.len() {
                2 if params.starts_with('0') => match params.get(1..2) {
                    Some("0") => respond(
                        response,
                        &format!("CO00{:04};", u16::from(self.state.contour_on)),
                    ),
                    Some("1") => {
                        respond(response, &format!("CO01{:04};", self.state.contour_freq_hz))
                    }
                    Some("2") => respond(
                        response,
                        &format!("CO02{:04};", u16::from(self.state.apf_on)),
                    ),
                    Some("3") => respond(
                        response,
                        &format!("CO03{:04};", apf_hz_to_raw(self.state.apf_freq_hz)),
                    ),
                    _ => respond(response, "?;"),
                },
                6 if params.starts_with('0') => {
                    let item = params.get(1..2);
                    let p3: Option<u16> = params.get(2..6).and_then(|s| s.parse().ok());
                    match (item, p3) {
                        (Some("0"), Some(v)) if v <= 1 => {
                            self.state.contour_on = v == 1;
                            events.push(Ft991aEvent {
                                field: "contour_on",
                                value: v.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        (Some("1"), Some(v)) if (10..=3200).contains(&v) => {
                            self.state.contour_freq_hz = v;
                            events.push(Ft991aEvent {
                                field: "contour_freq_hz",
                                value: v.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        (Some("2"), Some(v)) if v <= 1 => {
                            self.state.apf_on = v == 1;
                            events.push(Ft991aEvent {
                                field: "apf_on",
                                value: v.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        (Some("3"), Some(v)) if v <= 50 => {
                            self.state.apf_freq_hz = apf_raw_to_hz(v as u8);
                            events.push(Ft991aEvent {
                                field: "apf_freq_hz",
                                value: self.state.apf_freq_hz.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },

            // `BP`: 2-item selector — MANUAL NOTCH ON/OFF (P2=0), MANUAL
            // NOTCH LEVEL i.e. frequency (P2=1). Read is 2 chars; write is 5
            // chars (+ 3-digit P3) — manual p.5, module docs' "BP" section.
            Bp => match params.len() {
                2 if params.starts_with('0') => match params.get(1..2) {
                    Some("0") => respond(
                        response,
                        &format!("BP00{:03};", u16::from(self.state.manual_notch_on)),
                    ),
                    Some("1") => respond(
                        response,
                        &format!("BP01{:03};", self.state.manual_notch_freq_hz / 10),
                    ),
                    _ => respond(response, "?;"),
                },
                5 if params.starts_with('0') => {
                    let item = params.get(1..2);
                    let p3: Option<u16> = params.get(2..5).and_then(|s| s.parse().ok());
                    match (item, p3) {
                        (Some("0"), Some(v)) if v <= 1 => {
                            self.state.manual_notch_on = v == 1;
                            events.push(Ft991aEvent {
                                field: "manual_notch_on",
                                value: v.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        (Some("1"), Some(v)) if (1..=320).contains(&v) => {
                            self.state.manual_notch_freq_hz = v * 10;
                            events.push(Ft991aEvent {
                                field: "manual_notch_freq_hz",
                                value: self.state.manual_notch_freq_hz.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },

            Bc => match params.len() {
                1 if params == "0" => respond(
                    response,
                    &format!("BC0{};", u8::from(self.state.auto_notch_on)),
                ),
                2 if params.starts_with('0') => match params[1..2].parse::<u8>() {
                    Ok(v) if v <= 1 => {
                        self.state.auto_notch_on = v == 1;
                        events.push(Ft991aEvent {
                            field: "auto_notch_on",
                            value: v.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // `NA`: same selector-read shape as `NB`/`NR`/`BC`. Wire code is
            // `NA`, per the master table and this batch's own assignment —
            // see module docs' "NA, a genuine manual wire-diagram typo"
            // section for why the per-command box's own diagram (`M A P1
            // P2 ;`) is not followed literally.
            Na => match params.len() {
                1 if params == "0" => {
                    respond(response, &format!("NA0{};", u8::from(self.state.narrow_on)))
                }
                2 if params.starts_with('0') => match params[1..2].parse::<u8>() {
                    Ok(v) if v <= 1 => {
                        self.state.narrow_on = v == 1;
                        events.push(Ft991aEvent {
                            field: "narrow_on",
                            value: v.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // `SH`: selector read (1 char) / 3-char write (2-digit P2,
            // table index 0-21 — manual p.16). See [`SH_BANDWIDTH_TABLE`]
            // for the full six-column bandwidth lookup; this arm only
            // stores/reports the raw index, with structural range
            // validation only (0-21) — see module docs' "SH" section for
            // why this doesn't cross-validate against the currently active
            // mode/`NA` state.
            Sh => match params.len() {
                1 if params == "0" => respond(
                    response,
                    &format!("SH0{:02};", self.state.filter_width_index),
                ),
                3 if params.starts_with('0') => {
                    match params.get(1..3).and_then(|s| s.parse::<u8>().ok()) {
                        Some(v) if v <= 21 => {
                            self.state.filter_width_index = v;
                            events.push(Ft991aEvent {
                                field: "filter_width_index",
                                value: v.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },

            // -- Batch 7: speech processor/mic/monitor ----------------------
            //
            // `MG`/`PL`: plain query/set, no selector byte at all (same
            // shape as `PC` — manual p.11, p.14).
            Mg => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("MG{:03};", self.state.mic_gain))
                }
                CommandOperation::Set => match params.parse::<u8>() {
                    Ok(v) if v <= 100 => {
                        self.state.mic_gain = v;
                        events.push(Ft991aEvent {
                            field: "mic_gain",
                            value: v.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            Pl => match request.operation {
                CommandOperation::Query => respond(
                    response,
                    &format!("PL{:03};", self.state.speech_processor_level),
                ),
                CommandOperation::Set => match params.parse::<u8>() {
                    Ok(v) if v <= 100 => {
                        self.state.speech_processor_level = v;
                        events.push(Ft991aEvent {
                            field: "speech_processor_level",
                            value: v.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // `PR`: selector read (1 char: `P1` alone) or a 2-char write
            // (`P1` + `P2`) — see module docs' "PR, a genuine manual
            // heading typo" section. `P2`'s on/off encoding is `1`=OFF,
            // `2`=ON (not the usual `0`/`1`), transcribed exactly.
            Pr => match params.len() {
                1 if matches!(params, "0" | "1") => {
                    let on = if params == "0" {
                        self.state.speech_processor_on
                    } else {
                        self.state.parametric_mic_eq_on
                    };
                    let p2: u8 = if on { 2 } else { 1 };
                    respond(response, &format!("PR{params}{p2};"))
                }
                2 => {
                    let p1 = params.get(0..1);
                    let p2 = params.get(1..2);
                    match (p1, p2) {
                        (Some("0"), Some("1")) => {
                            self.state.speech_processor_on = false;
                            events.push(Ft991aEvent {
                                field: "speech_processor_on",
                                value: "false".to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        (Some("0"), Some("2")) => {
                            self.state.speech_processor_on = true;
                            events.push(Ft991aEvent {
                                field: "speech_processor_on",
                                value: "true".to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        (Some("1"), Some("1")) => {
                            self.state.parametric_mic_eq_on = false;
                            events.push(Ft991aEvent {
                                field: "parametric_mic_eq_on",
                                value: "false".to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        (Some("1"), Some("2")) => {
                            self.state.parametric_mic_eq_on = true;
                            events.push(Ft991aEvent {
                                field: "parametric_mic_eq_on",
                                value: "true".to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },

            // `ML`: selector read (1 char) or a 4-char write (`P1` +
            // 3-digit `P2`) — see module docs' "ML, the batch's only
            // composite command" section. `P1=0` addresses MONI on/off
            // (`P2` is `000`/`001`); `P1=1` addresses MONI level (`P2` is
            // `000`-`100`).
            Ml => match params.len() {
                1 if params == "0" => respond(
                    response,
                    &format!("ML0{:03};", u8::from(self.state.monitor_on)),
                ),
                1 if params == "1" => {
                    respond(response, &format!("ML1{:03};", self.state.monitor_level))
                }
                4 => {
                    let p1 = params.get(0..1);
                    let p2: Option<u8> = params.get(1..4).and_then(|s| s.parse().ok());
                    match (p1, p2) {
                        (Some("0"), Some(0)) => {
                            self.state.monitor_on = false;
                            events.push(Ft991aEvent {
                                field: "monitor_on",
                                value: "false".to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        (Some("0"), Some(1)) => {
                            self.state.monitor_on = true;
                            events.push(Ft991aEvent {
                                field: "monitor_on",
                                value: "true".to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        (Some("1"), Some(v)) if v <= 100 => {
                            self.state.monitor_level = v;
                            events.push(Ft991aEvent {
                                field: "monitor_level",
                                value: v.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },

            // `BS`: Set-only, 2-digit band code — see module docs' "BS's
            // full 16-band table" section. No `Read`/`Answer` form exists
            // at all, so there's no `CommandOperation::Query` arm here.
            Bs => match params.parse::<u8>() {
                Ok(v) if v <= 16 && v != 13 => {
                    self.state.selected_band = v;
                    events.push(Ft991aEvent {
                        field: "selected_band",
                        value: v.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            // `BU`/`BD`: `P1` is a required literal `"0"` (manual: "0:
            // Fixed"), not real data. Steps `selected_band` via
            // `next_band`/`prev_band`, wrapping and skipping the `13` gap
            // (documented judgment call, see module docs).
            Bu => match params {
                "0" => {
                    self.state.selected_band = next_band(self.state.selected_band);
                    events.push(Ft991aEvent {
                        field: "selected_band",
                        value: self.state.selected_band.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },
            Bd => match params {
                "0" => {
                    self.state.selected_band = prev_band(self.state.selected_band);
                    events.push(Ft991aEvent {
                        field: "selected_band",
                        value: self.state.selected_band.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            // `FS`: plain bidirectional bool, same shape as `RT`/`XT`/`CS`.
            Fs => match request.operation {
                CommandOperation::Query => respond(
                    response,
                    &format!("FS{};", u8::from(self.state.fast_step_on)),
                ),
                CommandOperation::Set => match params {
                    "0" | "1" => {
                        self.state.fast_step_on = params == "1";
                        events.push(Ft991aEvent {
                            field: "fast_step_on",
                            value: params.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // `ED`/`EU`: structurally and semantically validated (`P1` in
            // {0,1,8}, `P2` in 1-99) but mutate no persisted `Ft991aState`
            // field — see module docs' "ED/EU" section for why (no
            // Hz-per-step mapping is knowable from this manual page alone).
            Ed => {
                let p1 = params.get(0..1).and_then(|s| s.chars().next());
                let p2: Option<u8> = params.get(1..3).and_then(|s| s.parse().ok());
                match (p1, p2) {
                    (Some(c), Some(steps))
                        if EncoderSelector::from_wire_digit(c).is_some()
                            && (1..=99).contains(&steps) =>
                    {
                        events.push(Ft991aEvent {
                            field: "encoder_down",
                            value: format!("{c}:{steps}"),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                }
            }
            Eu => {
                let p1 = params.get(0..1).and_then(|s| s.chars().next());
                let p2: Option<u8> = params.get(1..3).and_then(|s| s.parse().ok());
                match (p1, p2) {
                    (Some(c), Some(steps))
                        if EncoderSelector::from_wire_digit(c).is_some()
                            && (1..=99).contains(&steps) =>
                    {
                        events.push(Ft991aEvent {
                            field: "encoder_up",
                            value: format!("{c}:{steps}"),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                }
            }

            // `EK`: zero-width Action trigger, no persisted-state effect —
            // same category as `ZI`/`RC`.
            Ek => match request.operation {
                CommandOperation::Action => {
                    events.push(Ft991aEvent {
                        field: "ent_key",
                        value: "triggered".to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            // `DN`/`UP`: zero-width Action triggers modeled after
            // `ts570d::Radio::mic_down`/`mic_up`'s own precedent — see
            // module docs' "DN/UP" section for the full cross-radio
            // corroboration. Each press steps `vfo_a_hz` by a fixed
            // `MIC_STEP_HZ`, saturating at `FA`'s documented range.
            Dn => match request.operation {
                CommandOperation::Action => {
                    self.state.vfo_a_hz =
                        self.state.vfo_a_hz.saturating_sub(MIC_STEP_HZ).max(30_000);
                    events.push(Ft991aEvent {
                        field: "vfo_a_hz",
                        value: self.state.vfo_a_hz.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },
            Up => match request.operation {
                CommandOperation::Action => {
                    self.state.vfo_a_hz = self
                        .state
                        .vfo_a_hz
                        .saturating_add(MIC_STEP_HZ)
                        .min(470_000_000);
                    events.push(Ft991aEvent {
                        field: "vfo_a_hz",
                        value: self.state.vfo_a_hz.to_string(),
                    });
                    ResponseDisposition::NoResponse
                }
                _ => respond(response, "?;"),
            },

            // -- Batch 10 (last of the 10 core batches): misc
            // system/TX/tuner/DVS ------------------------------------------
            //
            // `AC`: Read is genuinely zero-width (`AC;`, not a selector
            // read) — confirmed via the page image, unlike most of this
            // crate's other fixed-`P1` commands. `P1`/`P2` are required
            // literal "0" bytes; `P3` (0-2) is the real data, stored and
            // echoed back verbatim (see module docs' "AC" section for why
            // no Set/Answer domain split is modeled here).
            Ac => match request.operation {
                CommandOperation::Query => respond(
                    response,
                    &format!("AC00{};", self.state.antenna_tuner_state),
                ),
                CommandOperation::Set => {
                    let p1 = params.get(0..1);
                    let p2 = params.get(1..2);
                    let p3: Option<u8> = params.get(2..3).and_then(|s| s.parse().ok());
                    match (p1, p2, p3) {
                        (Some("0"), Some("0"), Some(v)) if v <= 2 => {
                            self.state.antenna_tuner_state = v;
                            events.push(Ft991aEvent {
                                field: "antenna_tuner_state",
                                value: v.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },

            // `AI`: plain bidirectional bool. The manual's "resets to 0 on
            // power-off" note is not enforced — see module docs' "AI"
            // section.
            Ai => match request.operation {
                CommandOperation::Query => respond(
                    response,
                    &format!("AI{};", u8::from(self.state.auto_info_on)),
                ),
                CommandOperation::Set => match params {
                    "0" | "1" => {
                        self.state.auto_info_on = params == "1";
                        events.push(Ft991aEvent {
                            field: "auto_info_on",
                            value: params.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // `DA`: plain query/set, no selector byte (same shape as
            // `MG`/`PL`) — `P1` is a required literal "00", `P2`/`P3` are
            // the real 2-digit brightness levels.
            Da => match request.operation {
                CommandOperation::Query => respond(
                    response,
                    &format!(
                        "DA00{:02}{:02};",
                        self.state.led_brightness, self.state.tft_brightness
                    ),
                ),
                CommandOperation::Set => {
                    let p1 = params.get(0..2);
                    let p2: Option<u8> = params.get(2..4).and_then(|s| s.parse().ok());
                    let p3: Option<u8> = params.get(4..6).and_then(|s| s.parse().ok());
                    match (p1, p2, p3) {
                        (Some("00"), Some(led), Some(tft))
                            if (1..=2).contains(&led) && tft <= 15 =>
                        {
                            self.state.led_brightness = led;
                            self.state.tft_brightness = tft;
                            events.push(Ft991aEvent {
                                field: "dimmer",
                                value: format!("{led}:{tft}"),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },

            // `DT`: `P1` selects `P2`'s shape — see module docs' "DT"
            // section for the full 3-shape citation. `params.len()`
            // (guaranteed one of {1,6,7,9} by `DT_SET_FORMS`) disambiguates
            // read vs. write; a structurally-legal width whose `P1` doesn't
            // match that width's expected shape (e.g. a 6-char frame with
            // `P1="0"`) falls through to `"?;"`, the same "structural match
            // succeeded, semantic validation still per-item" pattern
            // `EX`/`FA` already established.
            Dt => match params.len() {
                1 => match params {
                    "0" => respond(
                        response,
                        &format!(
                            "DT0{:04}{:02}{:02};",
                            self.state.date_year, self.state.date_month, self.state.date_day
                        ),
                    ),
                    "1" => respond(
                        response,
                        &format!(
                            "DT1{:02}{:02}{:02};",
                            self.state.time_hour, self.state.time_minute, self.state.time_second
                        ),
                    ),
                    "2" => {
                        let sign = if self.state.time_zone_offset_min < 0 {
                            '-'
                        } else {
                            '+'
                        };
                        let mag = self.state.time_zone_offset_min.unsigned_abs();
                        respond(
                            response,
                            &format!("DT2{sign}{:02}{:02};", mag / 60, mag % 60),
                        )
                    }
                    _ => respond(response, "?;"),
                },
                9 => {
                    let p1 = params.get(0..1);
                    let year: Option<u16> = params.get(1..5).and_then(|s| s.parse().ok());
                    let month: Option<u8> = params.get(5..7).and_then(|s| s.parse().ok());
                    let day: Option<u8> = params.get(7..9).and_then(|s| s.parse().ok());
                    match (p1, year, month, day) {
                        (Some("0"), Some(y), Some(m), Some(d))
                            if (1..=12).contains(&m) && (1..=31).contains(&d) =>
                        {
                            self.state.date_year = y;
                            self.state.date_month = m;
                            self.state.date_day = d;
                            events.push(Ft991aEvent {
                                field: "date",
                                value: format!("{y:04}-{m:02}-{d:02}"),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                7 => {
                    let p1 = params.get(0..1);
                    let hour: Option<u8> = params.get(1..3).and_then(|s| s.parse().ok());
                    let minute: Option<u8> = params.get(3..5).and_then(|s| s.parse().ok());
                    let second: Option<u8> = params.get(5..7).and_then(|s| s.parse().ok());
                    match (p1, hour, minute, second) {
                        (Some("1"), Some(h), Some(mi), Some(se))
                            if h <= 23 && mi <= 59 && se <= 59 =>
                        {
                            self.state.time_hour = h;
                            self.state.time_minute = mi;
                            self.state.time_second = se;
                            events.push(Ft991aEvent {
                                field: "time",
                                value: format!("{h:02}:{mi:02}:{se:02}"),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                6 => {
                    let p1 = params.get(0..1);
                    let sign_char = params.get(1..2);
                    let hh: Option<i16> = params.get(2..4).and_then(|s| s.parse().ok());
                    let mm: Option<i16> = params.get(4..6).and_then(|s| s.parse().ok());
                    match (p1, sign_char, hh, mm) {
                        (Some("2"), Some(sign), Some(h), Some(m))
                            if matches!(sign, "+" | "-") && (m == 0 || m == 30) =>
                        {
                            let magnitude = h * 60 + m;
                            let signed = if sign == "-" { -magnitude } else { magnitude };
                            if (-720..=840).contains(&signed) {
                                self.state.time_zone_offset_min = signed;
                                events.push(Ft991aEvent {
                                    field: "time_zone_offset_min",
                                    value: signed.to_string(),
                                });
                                ResponseDisposition::NoResponse
                            } else {
                                respond(response, "?;")
                            }
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },

            // `LK`: plain bidirectional bool.
            Lk => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("LK{};", u8::from(self.state.lock_on)))
                }
                CommandOperation::Set => match params {
                    "0" | "1" => {
                        self.state.lock_on = params == "1";
                        events.push(Ft991aEvent {
                            field: "lock_on",
                            value: params.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // `OI`: read-only, zero-width query (same treatment as `IF`/
            // `BY`, so no `request.operation` match is needed — `QUERY0`/
            // `NONE` means the parser only ever produces `Query` here).
            // Confirmed to share `IF`'s `ChannelStatusFields` shape exactly
            // — see module docs' "OI" section, including the documented
            // judgment call that `P1`/`P3`-`P10` reuse the exact same
            // shared (non-per-VFO) state `IF` already uses; only the
            // frequency genuinely differs (`vfo_b_hz`, not `vfo_a_hz`).
            Oi => {
                let payload = ChannelStatusFields {
                    channel: self.state.if_channel,
                    frequency_hz: self.state.vfo_b_hz,
                    clarifier_offset_hz: self.state.clarifier_offset_hz,
                    rx_clarifier_on: self.state.rx_clarifier_on,
                    tx_clarifier_on: self.state.tx_clarifier_on,
                    mode: self.state.mode,
                    select: self.state.channel_select,
                    tone_status: self.state.tone_status,
                    offset_type: self.state.offset_type,
                };
                respond(response, &format!("OI{};", payload.to_wire_string()))
            }

            // `OS`: selector read (fixed `P1="0"`), writes the same
            // `offset_type` field `IF`/`OI` already report — see module
            // docs' "OS" section.
            Os => match params.len() {
                1 if params == "0" => respond(response, &format!("OS0{};", self.state.offset_type)),
                2 => {
                    let p1 = params.get(0..1);
                    let p2: Option<u8> = params.get(1..2).and_then(|s| s.parse().ok());
                    match (p1, p2) {
                        (Some("0"), Some(v)) if v <= 2 => {
                            self.state.offset_type = v;
                            events.push(Ft991aEvent {
                                field: "offset_type",
                                value: v.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },

            // `FT`: genuine write/report domain mismatch — Set uses `2`/`3`,
            // Answer uses `0`/`1` for the identical two states. See module
            // docs' "FT" section.
            Ft => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("FT{};", self.state.tx_vfo_select))
                }
                CommandOperation::Set => match params {
                    "2" => {
                        self.state.tx_vfo_select = 0;
                        events.push(Ft991aEvent {
                            field: "tx_vfo_select",
                            value: "0".to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    "3" => {
                        self.state.tx_vfo_select = 1;
                        events.push(Ft991aEvent {
                            field: "tx_vfo_select",
                            value: "1".to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // `TS` ("TXW" — see module docs' "TS" section for why this is
            // not "tuning step" despite the architect's dispatch-prompt
            // guess): plain bidirectional bool.
            Ts => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("TS{};", u8::from(self.state.txw_on)))
                }
                CommandOperation::Set => match params {
                    "0" | "1" => {
                        self.state.txw_on = params == "1";
                        events.push(Ft991aEvent {
                            field: "txw_on",
                            value: params.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // `MX`: plain bidirectional bool.
            Mx => match request.operation {
                CommandOperation::Query => {
                    respond(response, &format!("MX{};", u8::from(self.state.mox_on)))
                }
                CommandOperation::Set => match params {
                    "0" | "1" => {
                        self.state.mox_on = params == "1";
                        events.push(Ft991aEvent {
                            field: "mox_on",
                            value: params.to_string(),
                        });
                        ResponseDisposition::NoResponse
                    }
                    _ => respond(response, "?;"),
                },
                _ => respond(response, "?;"),
            },

            // `LM`: selector read, per-channel Start/Stop **toggle** — see
            // module docs' "LM/PB" section for why this differs from `PB`'s
            // unconditional start/stop below.
            Lm => match params.len() {
                1 if params == "0" => respond(
                    response,
                    &format!("LM0{};", self.state.dvs_recording_channel),
                ),
                2 => {
                    let p1 = params.get(0..1);
                    let p2: Option<u8> = params.get(1..2).and_then(|s| s.parse().ok());
                    match (p1, p2) {
                        (Some("0"), Some(v)) if v <= 5 => {
                            // `v == 0` (explicit stop) and "same channel
                            // already recording" (toggle-stop) both resolve
                            // to `0`; any other non-zero `v` starts that
                            // channel — see module docs' "LM/PB" section.
                            self.state.dvs_recording_channel =
                                if v == 0 || self.state.dvs_recording_channel == v {
                                    0
                                } else {
                                    v
                                };
                            events.push(Ft991aEvent {
                                field: "dvs_recording_channel",
                                value: self.state.dvs_recording_channel.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },

            // `PB`: selector read, unconditional start/stop (not a toggle
            // like `LM` above — see module docs' "LM/PB" section).
            Pb => match params.len() {
                1 if params == "0" => respond(
                    response,
                    &format!("PB0{};", self.state.dvs_playback_channel),
                ),
                2 => {
                    let p1 = params.get(0..1);
                    let p2: Option<u8> = params.get(1..2).and_then(|s| s.parse().ok());
                    match (p1, p2) {
                        (Some("0"), Some(v)) if v <= 5 => {
                            self.state.dvs_playback_channel = v;
                            events.push(Ft991aEvent {
                                field: "dvs_playback_channel",
                                value: v.to_string(),
                            });
                            ResponseDisposition::NoResponse
                        }
                        _ => respond(response, "?;"),
                    }
                }
                _ => respond(response, "?;"),
            },
        };

        Ok(CommandOutcome {
            response: disposition,
            events,
        })
    }

    /// Write the FT-991A's protocol error response.
    ///
    /// **Assumption, not manual-cited**: the FT-991A CAT manual's 20 pages
    /// never state a protocol-error response format anywhere (unlike
    /// `ts570d`'s Kenwood manual, which documents `?;` directly). `"?;"` is
    /// widely known outside this manual to be the general Yaesu CAT
    /// convention, and matches `ts570d`'s own convention, but this has NOT
    /// been verified against real FT-991A hardware or an official errata —
    /// open item, see `planning/yaesu/task_plan.md`.
    fn write_protocol_error(
        &mut self,
        kind: ProtocolErrorKind,
        response: &mut ResponseBuilder<'_>,
    ) -> Result<CommandOutcome<Self::Event>, Self::Error> {
        response
            .write_complete("?;")
            .expect("response write cannot fail before finish");
        Ok(CommandOutcome {
            response: ResponseDisposition::ProtocolError(kind),
            events: Vec::new(),
        })
    }
}

/// Parse an `AG`/`RG`/`SQ`-shaped 4-character set parameter (`"0" +
/// 3-digit level`), returning the level if the selector is `0` and the
/// level is within `0..=max`.
fn parse_selector_level(params: &str, max: u16) -> Option<u8> {
    if !params.starts_with('0') {
        return None;
    }
    let level: u16 = params.get(1..4)?.parse().ok()?;
    if level <= max {
        Some(level as u8)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use cat_framework::CatFramework;

    use super::*;

    #[test]
    fn table_has_unique_codes_and_ids_and_legal_operations() {
        let mut codes = HashSet::new();
        let mut ids = HashSet::new();
        for definition in FT991A_COMMAND_TABLE.definitions() {
            assert!(
                codes.insert(definition.code),
                "duplicate code {}",
                definition.code
            );
            assert!(
                ids.insert(definition.id),
                "duplicate id {:?}",
                definition.id
            );
            assert!(
                !definition.query_forms.is_empty()
                    || !definition.set_forms.is_empty()
                    || !definition.action_forms.is_empty(),
                "{} has no legal operation",
                definition.code
            );
        }
        assert_eq!(
            codes.len(),
            91,
            "expected exactly 91 commands (11 first-slice + 6 batch-9 meters/status + EX + 4 batch-2 memory channel records + 10 batch-1 VFO/split/memory quick-ops + 8 batch-3 clarifier/tone/IF-shift + 9 batch-4 keyer/CW/break-in + 5 batch-5 scan/VOX/busy + 12 batch-6 attenuator/preamp/noise/AGC/notch/filter-width + 4 batch-7 speech processor/mic/monitor + 9 batch-8 band/step/encoder front-panel controls + 12 batch-10 misc system/TX/tuner/DVS — all 91 top-level (non-EX) CAT commands, the last of the 10 core batches)"
        );
    }

    #[test]
    fn ex_menu_table_has_exactly_151_entries() {
        // First sub-batch's 9 PTT/keying items + second sub-batch's 45
        // items (001-046 minus 027, explicitly skipped) + third sub-batch's
        // 26 new items (049-079, minus 060/071/072/076/077 already counted
        // in the first sub-batch's 9) + fourth sub-batch's 71 new items
        // (080-153, minus 087 skipped and 108/109 already counted in the
        // first sub-batch's 9) = 151, not one more. Every one of the 153
        // manual menu numbers is now covered except 027 and 087.
        assert_eq!(EX_MENU_TABLE.len(), 151);
        let mut p1s: Vec<u16> = EX_MENU_TABLE.iter().map(|item| item.p1).collect();
        p1s.sort_unstable();
        let mut expected: Vec<u16> = (1..=46).filter(|&p1| p1 != 27).collect();
        expected.extend([47, 48]);
        expected.extend((49..=79).filter(|p1| ![60, 71, 72, 76, 77].contains(p1)));
        expected.extend([60, 71, 72, 76, 77]);
        expected.extend([108, 109]);
        expected.extend((80..=153).filter(|p1| ![87, 108, 109].contains(p1)));
        expected.sort_unstable();
        assert_eq!(p1s, expected);
    }

    #[test]
    fn table_master_flags_match_manual_p3() {
        // Cross-check against the manual's p.3 master Set/Read/Ans/AI table.
        let fa = FT991A_COMMAND_TABLE.find("FA").unwrap();
        assert!(fa.is_readable() && fa.is_writable());
        let sm = FT991A_COMMAND_TABLE.find("SM").unwrap();
        assert!(sm.is_readable() && !sm.is_writable(), "SM is read-only");
        let id = FT991A_COMMAND_TABLE.find("ID").unwrap();
        assert!(id.is_readable() && !id.is_writable(), "ID is read-only");
        let ps = FT991A_COMMAND_TABLE.find("PS").unwrap();
        assert!(ps.is_readable() && ps.is_writable());

        // Batch 9 (meters/status): manual p.3 rows are
        // "IF X O O X", "RM X O O O", "RI X O O X", "RS X O O X",
        // "MS O O O O", "UL X O O O" (Set/Read/Ans/AI). This crate's table
        // doesn't model AI, but Set/Read map directly onto writable/readable.
        let iff = FT991A_COMMAND_TABLE.find("IF").unwrap();
        assert!(iff.is_readable() && !iff.is_writable(), "IF is read-only");
        let rm = FT991A_COMMAND_TABLE.find("RM").unwrap();
        assert!(rm.is_readable() && !rm.is_writable(), "RM is read-only");
        let ri = FT991A_COMMAND_TABLE.find("RI").unwrap();
        assert!(ri.is_readable() && !ri.is_writable(), "RI is read-only");
        let rs = FT991A_COMMAND_TABLE.find("RS").unwrap();
        assert!(rs.is_readable() && !rs.is_writable(), "RS is read-only");
        let ms = FT991A_COMMAND_TABLE.find("MS").unwrap();
        assert!(ms.is_readable() && ms.is_writable(), "MS is read/write");
        let ul = FT991A_COMMAND_TABLE.find("UL").unwrap();
        assert!(ul.is_readable() && !ul.is_writable(), "UL is read-only");

        // Batch 2 (memory channel records): manual p.3 rows are
        // "MC O O O X", "MR X O O X", "MT O O O X", "MW O X X X".
        let mc = FT991A_COMMAND_TABLE.find("MC").unwrap();
        assert!(mc.is_readable() && mc.is_writable(), "MC is read/write");
        let mr = FT991A_COMMAND_TABLE.find("MR").unwrap();
        assert!(mr.is_readable() && !mr.is_writable(), "MR is read-only");
        let mt = FT991A_COMMAND_TABLE.find("MT").unwrap();
        assert!(mt.is_readable() && mt.is_writable(), "MT is read/write");
        let mw = FT991A_COMMAND_TABLE.find("MW").unwrap();
        assert!(!mw.is_readable() && mw.is_writable(), "MW is write-only");

        // Batch 1 (VFO/split/memory quick-ops): manual p.3 rows are all
        // "Set O Read X Ans X" — write-only, no read at all.
        for code in ["AB", "BA", "AM", "VM", "MA", "CH", "QI", "QR", "QS", "SV"] {
            let def = FT991A_COMMAND_TABLE.find(code).unwrap();
            assert!(
                !def.is_readable() && def.is_writable(),
                "{code} should be write-only per manual p.3"
            );
        }

        // Batch 3 (clarifier/RIT-XIT + tone + IF-shift): manual p.3 rows are
        // "RT O O O O", "RC O X X X", "RD O X X X", "RU O X X X",
        // "XT O O O O", "CN O O O O", "CT O O O O", "IS O O O O".
        for code in ["RT", "XT", "CN", "CT", "IS"] {
            let def = FT991A_COMMAND_TABLE.find(code).unwrap();
            assert!(
                def.is_readable() && def.is_writable(),
                "{code} should be read/write per manual p.3"
            );
        }
        for code in ["RC", "RD", "RU"] {
            let def = FT991A_COMMAND_TABLE.find(code).unwrap();
            assert!(
                !def.is_readable() && def.is_writable(),
                "{code} should be write-only per manual p.3"
            );
        }

        // Batch 4 (keyer/CW/break-in): manual p.3 rows are
        // "KM O O O X", "KP O O O O", "KR O O O O", "KS O O O O",
        // "KY O X X X", "CS O O O O", "ZI O X X X", "BI O O O O",
        // "SD O O O O".
        for code in ["KM", "KP", "KR", "KS", "CS", "BI", "SD"] {
            let def = FT991A_COMMAND_TABLE.find(code).unwrap();
            assert!(
                def.is_readable() && def.is_writable(),
                "{code} should be read/write per manual p.3"
            );
        }
        for code in ["KY", "ZI"] {
            let def = FT991A_COMMAND_TABLE.find(code).unwrap();
            assert!(
                !def.is_readable() && def.is_writable(),
                "{code} should be write-only per manual p.3"
            );
        }

        // Batch 5 (scan/VOX/busy): manual p.3 rows are
        // "SC O O O O", "VX O O O O", "VD O O O O", "VG O O O O",
        // "BY X O O O".
        for code in ["SC", "VX", "VD", "VG"] {
            let def = FT991A_COMMAND_TABLE.find(code).unwrap();
            assert!(
                def.is_readable() && def.is_writable(),
                "{code} should be read/write per manual p.3"
            );
        }
        let by = FT991A_COMMAND_TABLE.find("BY").unwrap();
        assert!(by.is_readable() && !by.is_writable(), "BY is read-only");

        // Batch 6 (attenuator/preamp/noise/AGC/notch/filter-width): manual
        // p.3 rows are all "Set O Read O Ans O" — every one of the twelve
        // is read/write.
        for code in [
            "RA", "PA", "NB", "NL", "NR", "RL", "GT", "CO", "BP", "BC", "NA", "SH",
        ] {
            let def = FT991A_COMMAND_TABLE.find(code).unwrap();
            assert!(
                def.is_readable() && def.is_writable(),
                "{code} should be read/write per manual p.3"
            );
        }

        // Batch 7 (speech processor/mic/monitor): manual p.3 rows are all
        // "Set O Read O Ans O" — every one of the four is read/write.
        for code in ["MG", "PL", "PR", "ML"] {
            let def = FT991A_COMMAND_TABLE.find(code).unwrap();
            assert!(
                def.is_readable() && def.is_writable(),
                "{code} should be read/write per manual p.3"
            );
        }

        // Batch 8 (band/step/encoder front-panel controls): manual p.3 rows
        // are all "Set O Read X Ans X" except FS ("Set O Read O Ans O").
        for code in ["BS", "BU", "BD", "ED", "EU", "EK", "DN", "UP"] {
            let def = FT991A_COMMAND_TABLE.find(code).unwrap();
            assert!(
                !def.is_readable() && def.is_writable(),
                "{code} should be write-only per manual p.3"
            );
        }
        let fs = FT991A_COMMAND_TABLE.find("FS").unwrap();
        assert!(fs.is_readable() && fs.is_writable(), "FS is read/write");

        // Batch 10 (misc system/TX/tuner/DVS): manual p.3 rows are
        // "AC O O O O", "AI O O O X", "DA O O O X", "DT O O O X",
        // "LK O O O O", "OI X O O O", "OS O O O O", "FT O O O O",
        // "TS O O O O", "MX O O O O", "LM O O O X", "PB O O O X".
        for code in [
            "AC", "AI", "DA", "DT", "LK", "OS", "FT", "TS", "MX", "LM", "PB",
        ] {
            let def = FT991A_COMMAND_TABLE.find(code).unwrap();
            assert!(
                def.is_readable() && def.is_writable(),
                "{code} should be read/write per manual p.3"
            );
        }
        let oi = FT991A_COMMAND_TABLE.find("OI").unwrap();
        assert!(oi.is_readable() && !oi.is_writable(), "OI is read-only");
    }

    #[test]
    fn framework_fa_query_returns_wire_response() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("FA;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "FA014000000;");
    }

    #[test]
    fn framework_fa_set_then_query_preserves_state() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework
            .process_frame("FA014250000;", &mut output)
            .unwrap();
        assert!(output.is_empty());

        framework.process_frame("FA;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "FA014250000;");
    }

    #[test]
    fn framework_fa_out_of_range_frequency_is_rejected() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        // 480,000,000 Hz exceeds the 470,000,000 Hz maximum.
        framework
            .process_frame("FA480000000;", &mut output)
            .unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");
    }

    #[test]
    fn framework_tx_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("TX;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "TX0;");

        output.clear();
        framework.process_frame("TX1;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("TX;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "TX1;");
    }

    #[test]
    fn framework_tx_never_reports_the_answer_only_value_two() {
        // The emulator's state machine only models CAT-driven TX (0/1);
        // `2` ("radio TX on via a non-CAT cause") is answer-only per the
        // manual and is never produced by this first-slice state machine.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("TX;", &mut output).unwrap();
        assert_ne!(String::from_utf8(output.clone()).unwrap(), "TX2;");
    }

    // -----------------------------------------------------------------
    // The highest-risk part of this design: MD/SM's selector-read shape.
    // -----------------------------------------------------------------

    #[test]
    fn framework_md_selector_read_parses_as_a_read_not_a_write() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // "MD0;" is structurally a Set (1-byte parameter matches
        // MD_SET_FORMS's width-1 form) but semantically a read — the
        // default mode (USB = 0x2) must come back unchanged.
        framework.process_frame("MD0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MD02;");
    }

    #[test]
    fn framework_md_two_char_parameter_parses_as_a_write() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // "MD01;" (2-byte parameter, matches the width-2 form) sets mode to
        // LSB (1) and produces no response, unlike the 1-byte selector read.
        framework.process_frame("MD01;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("MD0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MD01;");
    }

    #[test]
    fn framework_md_hex_mode_c4fm_round_trips() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("MD0E;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("MD0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MD0E;");
    }

    #[test]
    fn framework_sm_selector_read_returns_level_and_has_no_write() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("SM0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "SM0000;");

        // SM has no set form at all — any 4-char attempt is an unknown
        // parameter width for the SM command.
        output.clear();
        framework.process_frame("SM0123;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");
    }

    // -----------------------------------------------------------------
    // Corrected (non-selector-read) AG/RG/SQ shape — plain zero-width
    // query, single 4-char set form.
    // -----------------------------------------------------------------

    #[test]
    fn framework_ag_zero_width_query_and_selector_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("AG;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "AG0128;");

        output.clear();
        framework.process_frame("AG0200;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("AG;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "AG0200;");
    }

    #[test]
    fn framework_sq_range_is_0_to_100_not_255() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // 150 is valid for AG/RG (max 255) but not SQ (max 100).
        framework.process_frame("SQ0150;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        output.clear();
        framework.process_frame("SQ0075;", &mut output).unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn framework_pc_zero_width_query_and_plain_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("PC;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "PC100;");

        output.clear();
        framework.process_frame("PC050;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("PC;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "PC050;");
    }

    #[test]
    fn framework_id_is_read_only_fixed_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("ID;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "ID0670;");
    }

    #[test]
    fn framework_unknown_command_uses_protocol_error_response() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        let outcome = framework.process_frame("ZZ;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");
        assert!(matches!(
            outcome.response,
            ResponseDisposition::ProtocolError(ProtocolErrorKind::UnknownCommand)
        ));
    }

    #[test]
    fn framework_ps_wake_sequence_quirk_field_documented_not_tested_here() {
        // PS itself is a plain zero-width query / 1-char set, tested like
        // TX above. The "dummy data, then 1-2s delay" wake sequence (p.14)
        // is a caller-side sequencing concern (see `Ft991a` in ft991a.rs),
        // not something the emulator's state machine or command table can
        // express — this test only documents where that behavior lives.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("PS;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "PS1;");
    }

    // -----------------------------------------------------------------
    // Batch 9: meters/status (`IF RM RI RS MS UL`).
    // -----------------------------------------------------------------

    // -- ChannelStatusFields: struct-level round trips -----------------
    //
    // These exercise field combinations `IF` itself cannot yet reach via
    // CAT commands alone (clarifier/select/tone/offset-type have no `Set`
    // command in any landed batch — see `Ft991aState`'s per-field doc
    // comments), which is why they're tested at the struct level rather
    // than only through `process_frame`. See below for the
    // `process_frame`-level tests covering what *is* reachable (FA/MD).

    #[test]
    fn channel_status_fields_round_trips_all_zero() {
        let fields = ChannelStatusFields {
            channel: 0,
            frequency_hz: 0,
            clarifier_offset_hz: 0,
            rx_clarifier_on: false,
            tx_clarifier_on: false,
            mode: 1, // LSB — 0 is not a legal MODE value
            select: 0,
            tone_status: 0,
            offset_type: 0,
        };
        let wire = fields.to_wire_string();
        assert_eq!(wire.len(), ChannelStatusFields::WIRE_WIDTH);
        assert_eq!(wire, "000000000000+000000100000");
        assert_eq!(ChannelStatusFields::parse(&wire), Some(fields));
    }

    #[test]
    fn channel_status_fields_round_trips_realistic_combination_one() {
        // Memory channel 42, 14.250 MHz, +1200 Hz RX clarifier on (TX off),
        // CW-U mode, QMB select, CTCSS ENC, plus-shift offset.
        let fields = ChannelStatusFields {
            channel: 42,
            frequency_hz: 14_250_000,
            clarifier_offset_hz: 1200,
            rx_clarifier_on: true,
            tx_clarifier_on: false,
            mode: 0x3,      // CW-U
            select: 3,      // QMB
            tone_status: 2, // CTCSS ENC
            offset_type: 1, // Plus Shift
        };
        let wire = fields.to_wire_string();
        assert_eq!(wire, "042014250000+120010332001");
        assert_eq!(ChannelStatusFields::parse(&wire), Some(fields));
    }

    #[test]
    fn channel_status_fields_round_trips_negative_offset_and_high_mode() {
        // Memory channel 117 (max), -9999 Hz TX clarifier on (RX off),
        // C4FM mode, HOME select, DCS ENC, minus-shift offset.
        let fields = ChannelStatusFields {
            channel: 117,
            frequency_hz: 470_000_000,
            clarifier_offset_hz: -9999,
            rx_clarifier_on: false,
            tx_clarifier_on: true,
            mode: 0xE,      // C4FM
            select: 6,      // HOME
            tone_status: 4, // DCS ENC
            offset_type: 2, // Minus Shift
        };
        let wire = fields.to_wire_string();
        assert_eq!(ChannelStatusFields::parse(&wire), Some(fields));
        assert!(wire.starts_with("117470000000-9999"));
        assert!(wire.ends_with("01E64002"));
    }

    #[test]
    fn channel_status_fields_parse_rejects_wrong_width() {
        assert_eq!(
            ChannelStatusFields::parse("000000000000+000000100000EXTRA"),
            None
        );
        assert_eq!(ChannelStatusFields::parse("short"), None);
    }

    #[test]
    fn channel_status_fields_parse_rejects_invalid_mode() {
        // Mode 0 and 0xF are not legal (manual p.11: modes are 1..=E).
        // Index 19 (0-based) is P6/mode — see ChannelStatusFields's column
        // table (body columns 3-27 map to indices 0-24; P6 is column 22,
        // body index 19).
        let mut body = "000000000000+000000100000".to_string();
        body.replace_range(19..20, "0");
        assert_eq!(ChannelStatusFields::parse(&body), None);
    }

    #[test]
    fn channel_status_fields_parse_rejects_bad_sign_char() {
        // Index 12 is P3's sign character.
        let mut body = "000000000000+000000100000".to_string();
        body.replace_range(12..13, "*");
        assert_eq!(ChannelStatusFields::parse(&body), None);
    }

    #[test]
    fn channel_status_fields_parse_rejects_out_of_range_select() {
        // select (P7) legal range is 0-6; 7 is out of range. Index 20 is
        // P7/select.
        let mut body = "000000000000+000000100000".to_string();
        body.replace_range(20..21, "7");
        assert_eq!(ChannelStatusFields::parse(&body), None);
    }

    // -- IF: process_frame round trips (default state + FA/MD-reachable
    // combinations) --------------------------------------------------

    #[test]
    fn framework_if_query_default_state() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("IF;", &mut output).unwrap();
        // Default state: channel 0, VFO-A 14,000,000 Hz, no clarifier,
        // mode USB (0x2), select 0 (VFO), tone 0 (off), offset 0 (simplex).
        assert_eq!(
            String::from_utf8(output.clone()).unwrap(),
            "IF000014000000+000000200000;"
        );
    }

    #[test]
    fn framework_if_reflects_fa_and_md_after_set() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework
            .process_frame("FA014250000;", &mut output)
            .unwrap();
        output.clear();
        framework.process_frame("MD0E;", &mut output).unwrap(); // C4FM
        output.clear();

        framework.process_frame("IF;", &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output.clone()).unwrap(),
            "IF000014250000+000000E00000;"
        );
    }

    #[test]
    fn framework_if_reflects_non_default_channel_status_state() {
        // Exercises fields IF cannot reach via any landed batch's CAT
        // commands yet (clarifier/select/tone/offset-type) — constructed
        // directly via `Ft991aRadio::from_state`, per that method's doc
        // comment.
        let state = Ft991aState {
            if_channel: 5,
            clarifier_offset_hz: -250,
            rx_clarifier_on: true,
            tx_clarifier_on: true,
            channel_select: 1, // Memory
            tone_status: 3,    // DCS ENC/DEC
            offset_type: 2,    // Minus Shift
            ..Ft991aState::default()
        };
        let mut framework = CatFramework::new(Ft991aRadio::from_state(state));
        let mut output = Vec::new();
        framework.process_frame("IF;", &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output.clone()).unwrap(),
            "IF005014000000-025011213002;"
        );
    }

    // -- RS ------------------------------------------------------------

    #[test]
    fn framework_rs_query_default_reports_normal_mode() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("RS;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "RS0;");
    }

    #[test]
    fn framework_rs_reports_menu_mode_when_state_says_so() {
        let state = Ft991aState {
            menu_mode: true,
            ..Ft991aState::default()
        };
        let mut framework = CatFramework::new(Ft991aRadio::from_state(state));
        let mut output = Vec::new();
        framework.process_frame("RS;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "RS1;");
    }

    // -- UL ------------------------------------------------------------

    #[test]
    fn framework_ul_query_default_reports_pll_lock() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("UL;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "UL0;");
    }

    #[test]
    fn framework_ul_reports_unlock_when_state_says_so() {
        let state = Ft991aState {
            pll_unlocked: true,
            ..Ft991aState::default()
        };
        let mut framework = CatFramework::new(Ft991aRadio::from_state(state));
        let mut output = Vec::new();
        framework.process_frame("UL;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "UL1;");
    }

    // -- RI (selector read with a documented gap: 0,3-7,A only) --------

    #[test]
    fn framework_ri_valid_selectors_report_off_in_this_emulator() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        for (selector, expected) in [
            ("0", "RI00;"),
            ("3", "RI30;"),
            ("4", "RI40;"),
            ("5", "RI50;"),
            ("6", "RI60;"),
            ("7", "RI70;"),
            ("A", "RIA0;"),
        ] {
            let mut output = Vec::new();
            framework
                .process_frame(&format!("RI{selector};"), &mut output)
                .unwrap();
            assert_eq!(String::from_utf8(output).unwrap(), expected);
        }
    }

    #[test]
    fn framework_ri_rejects_selectors_in_the_documented_gap() {
        // 1, 2, 8, 9, B-F are not listed in the manual's P1 legend.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        for selector in ["1", "2", "8", "9", "B", "F"] {
            let mut output = Vec::new();
            framework
                .process_frame(&format!("RI{selector};"), &mut output)
                .unwrap();
            assert_eq!(
                String::from_utf8(output).unwrap(),
                "?;",
                "selector {selector} should be rejected"
            );
        }
    }

    // -- MS: read/write --------------------------------------------------

    #[test]
    fn framework_ms_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("MS;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MS0;"); // default COMP

        output.clear();
        framework.process_frame("MS3;", &mut output).unwrap(); // SWR
        assert!(output.is_empty());

        framework.process_frame("MS;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MS3;");
    }

    #[test]
    fn framework_ms_rejects_out_of_range_selector() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        // 6-9 are not legal MS selectors (only 0-5: COMP/ALC/PO/SWR/ID/VDD).
        framework.process_frame("MS6;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");
    }

    // -- RM: selector read, including the RM/MS interaction ------------

    #[test]
    fn framework_rm_direct_selectors_read_independent_meter_fields() {
        let state = Ft991aState {
            smeter: 10,
            comp_meter: 20,
            alc_meter: 30,
            po_meter: 40,
            swr_meter: 50,
            id_meter: 60,
            vdd_meter: 70,
            ..Ft991aState::default()
        };
        let mut framework = CatFramework::new(Ft991aRadio::from_state(state));

        for (selector, expected) in [
            ("1", "RM1010;"), // S-meter (shared with SM's field)
            ("3", "RM3020;"), // COMP
            ("4", "RM4030;"), // ALC
            ("5", "RM5040;"), // PO
            ("6", "RM6050;"), // SWR
            ("7", "RM7060;"), // Id
            ("8", "RM8070;"), // Vd
        ] {
            let mut output = Vec::new();
            framework
                .process_frame(&format!("RM{selector};"), &mut output)
                .unwrap();
            assert_eq!(String::from_utf8(output).unwrap(), expected);
        }
    }

    #[test]
    fn framework_rm_selectors_0_and_2_depend_on_current_ms_selection() {
        // This is the confirmed RM/MS relationship (manual p.15's RM P1
        // legend: "0: Depends on the front panel METER", "2: Depends on
        // the front panel METER (PO / COMP / ALC / SWR / ID / VDD)") —
        // both route through whatever `MS` currently has selected, per
        // `Ft991aState::selected_meter_reading`.
        let state = Ft991aState {
            comp_meter: 111,
            alc_meter: 222,
            swr_meter: 33,
            ..Ft991aState::default()
        };
        let mut framework = CatFramework::new(Ft991aRadio::from_state(state));
        let mut output = Vec::new();

        // meter_select defaults to 0 (COMP).
        framework.process_frame("RM0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "RM0111;");
        output.clear();
        framework.process_frame("RM2;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "RM2111;");

        // Switch MS to ALC (1) — RM0/RM2 must now report the ALC reading.
        output.clear();
        framework.process_frame("MS1;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("RM0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "RM0222;");

        // Switch MS to SWR (3) — RM0/RM2 must now report the SWR reading,
        // while RM6 (direct SWR select) agrees.
        output.clear();
        framework.process_frame("MS3;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("RM2;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "RM2033;");
        output.clear();
        framework.process_frame("RM6;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "RM6033;");
    }

    #[test]
    fn framework_rm_rejects_out_of_range_selector() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        // 9 is out of RM's legal 0-8 selector range.
        framework.process_frame("RM9;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");
    }

    // -----------------------------------------------------------------
    // EX menu: first sub-batch (shared plumbing + the 9 PTT/keying items).
    // -----------------------------------------------------------------

    #[test]
    fn ex_menu_item_finds_all_nine_landed_items() {
        for p1 in [47u16, 48, 60, 71, 72, 76, 77, 108, 109] {
            assert!(ex_menu_item(p1).is_some(), "expected item {p1} to exist");
        }
    }

    #[test]
    fn ex_menu_item_returns_none_for_any_unlanded_p1() {
        // 027 (TIME ZONE) and 087 (RADIO ID) are the only two of the 153
        // manual menu numbers explicitly skipped as unresolvable (see
        // EX_MENU_TABLE's doc comment) — every other number 001-153 is now
        // landed. 999 isn't a real menu number at all. None of these may
        // panic.
        for p1 in [27u16, 87, 999] {
            assert!(
                ex_menu_item(p1).is_none(),
                "expected item {p1} to be absent from the landed table"
            );
        }
    }

    #[test]
    fn framework_ex_read_returns_default_values_for_all_nine_items() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        for (frame, expected) in [
            ("EX047;", "EX0470;"), // AM PTT SELECT, default DAKY (0)
            ("EX048;", "EX0480;"), // AM PORT SELECT, default DATA (0)
            ("EX060;", "EX0600;"), // PC KEYING, default OFF (0)
            ("EX071;", "EX0710;"), // DATA PTT SELECT, default DAKY (0)
            ("EX072;", "EX0721;"), // DATA PORT SELECT, default DATA (1)
            ("EX076;", "EX0760;"), // FM PKT PTT SELECT, default DAKY (0)
            ("EX077;", "EX0771;"), // FM PKT PORT SELECT, default DATA (1)
            ("EX108;", "EX1080;"), // SSB PTT SELECT, default DAKY (0)
            ("EX109;", "EX1090;"), // SSB PORT SELECT, default DATA (0)
        ] {
            let mut output = Vec::new();
            framework.process_frame(frame, &mut output).unwrap();
            assert_eq!(String::from_utf8(output).unwrap(), expected, "{frame}");
        }
    }

    #[test]
    fn framework_ex_pc_keying_write_then_read_round_trips_all_legal_values() {
        // EX 060 "PC KEYING" — the item this wave's RTS/DTR CW-keying
        // feature actually reads/writes: 0:OFF 1:DAKY 2:RTS 3:DTR.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        for value in ["0", "1", "2", "3"] {
            let mut output = Vec::new();
            framework
                .process_frame(&format!("EX060{value};"), &mut output)
                .unwrap();
            assert!(output.is_empty(), "write should produce no response");

            output.clear();
            framework.process_frame("EX060;", &mut output).unwrap();
            assert_eq!(String::from_utf8(output).unwrap(), format!("EX060{value};"));
        }
    }

    #[test]
    fn framework_ex_pc_keying_rejects_out_of_range_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        // Legal values are 0-3; 4 is not in PC KEYING's legend.
        framework.process_frame("EX0604;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        // State must be unchanged after the rejected write.
        output.clear();
        framework.process_frame("EX060;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "EX0600;");
    }

    #[test]
    fn framework_ex_write_round_trips_for_every_landed_item_valid_and_invalid_values() {
        let cases: &[(u16, &[&str], &[&str])] = &[
            (47, &["0", "1", "2"], &["3", "9"]),
            (48, &["0", "1"], &["2", "9"]),
            (60, &["0", "1", "2", "3"], &["4", "9"]),
            (71, &["0", "1", "2"], &["3", "9"]),
            (72, &["1", "2"], &["0", "3"]), // legend starts at 1, not 0
            (76, &["0", "1", "2"], &["3", "9"]),
            (77, &["1", "2"], &["0", "3"]), // legend starts at 1, not 0
            (108, &["0", "1", "2"], &["3", "9"]),
            (109, &["0", "1"], &["2", "9"]),
        ];

        for (p1, valid, invalid) in cases {
            let mut framework = CatFramework::new(Ft991aRadio::new());

            for value in *valid {
                let mut output = Vec::new();
                framework
                    .process_frame(&format!("EX{p1:03}{value};"), &mut output)
                    .unwrap();
                assert!(output.is_empty(), "EX{p1:03}{value}; should write silently");

                output.clear();
                framework
                    .process_frame(&format!("EX{p1:03};"), &mut output)
                    .unwrap();
                assert_eq!(
                    String::from_utf8(output).unwrap(),
                    format!("EX{p1:03}{value};"),
                    "item {p1} did not read back {value}"
                );
            }

            for value in *invalid {
                let mut output = Vec::new();
                framework
                    .process_frame(&format!("EX{p1:03}{value};"), &mut output)
                    .unwrap();
                assert_eq!(
                    String::from_utf8(output).unwrap(),
                    "?;",
                    "item {p1} should reject value {value}"
                );
            }
        }
    }

    #[test]
    fn framework_ex_rejects_wrong_digit_width_for_a_landed_item() {
        // EX060 (PC KEYING) is a 1-digit item; a structurally-legal
        // 2-digit write (matches EX_SET_FORMS' width-5 form, used by other
        // menu items) must still be rejected once the per-item digits
        // check runs.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("EX06012;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        // State unchanged.
        output.clear();
        framework.process_frame("EX060;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "EX0600;");
    }

    #[test]
    fn framework_ex_out_of_table_p1_fails_cleanly_not_a_panic() {
        // EX027 (TIME ZONE) is a real manual menu item explicitly skipped
        // as unresolvable (see EX_MENU_TABLE's doc comment) — both the read
        // and a structurally-legal write must cleanly resolve to "?;".
        let mut framework = CatFramework::new(Ft991aRadio::new());

        let mut output = Vec::new();
        framework.process_frame("EX027;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        // EX027's own real shape is a 5-digit write (total width 8); use
        // that width here to confirm even a structurally-plausible write
        // to an unlanded item resolves cleanly.
        output.clear();
        framework.process_frame("EX02700000;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        // A completely bogus menu number (not even a real manual item).
        output.clear();
        framework.process_frame("EX999;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_ex_independent_items_do_not_clobber_each_others_state() {
        // Writing one item must not affect another item's stored value,
        // even when both are 1-digit "PTT SELECT"-shaped items.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("EX0472;", &mut output).unwrap(); // AM PTT -> DTR
        output.clear();
        framework.process_frame("EX1082;", &mut output).unwrap(); // SSB PTT -> DTR
        output.clear();

        framework.process_frame("EX047;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "EX0472;");
        output.clear();
        framework.process_frame("EX108;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "EX1082;");
        // A third, untouched item stays at its default.
        output.clear();
        framework.process_frame("EX071;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "EX0710;");
    }

    // -----------------------------------------------------------------
    // EX menu: second sub-batch (items 001-046, minus 027).
    // -----------------------------------------------------------------

    #[test]
    fn ex_menu_item_finds_all_second_sub_batch_items() {
        for p1 in (1u16..=46).filter(|&p1| p1 != 27) {
            assert!(ex_menu_item(p1).is_some(), "expected item {p1} to exist");
        }
    }

    #[test]
    fn ex_menu_item_027_time_zone_is_explicitly_absent() {
        // See EX_MENU_TABLE's doc comment: no wire-encoding formula is
        // given for this item anywhere in the manual, unlike its
        // signed-range siblings 035/039 — deliberately skipped, same
        // treatment as item 087 "RADIO ID".
        assert!(ex_menu_item(27).is_none());
    }

    #[test]
    fn framework_ex_read_returns_default_values_for_second_sub_batch_items() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        for (frame, expected) in [
            ("EX001;", "EX0010020;"), // AGC FAST DELAY, default 20 (min)
            ("EX002;", "EX0020020;"), // AGC MID DELAY, default 20 (min)
            ("EX003;", "EX0030020;"), // AGC SLOW DELAY, default 20 (min)
            ("EX004;", "EX0040;"),    // HOME FUNCTION, default SCOPE (0)
            ("EX005;", "EX0050;"),    // MY CALL INDICATION, default 0
            ("EX006;", "EX0060;"),    // DISPLAY COLOR, default BLUE (0)
            ("EX007;", "EX0070;"),    // DIMMER LED, default 0
            ("EX008;", "EX00800;"),   // DIMMER TFT, default 00 (min)
            ("EX009;", "EX0090;"),    // BAR MTR PEAK HOLD, default OFF (0)
            ("EX010;", "EX010000;"),  // DVS RX OUT LEVEL, default 000 (min)
            ("EX011;", "EX011000;"),  // DVS TX OUT LEVEL, default 000 (min)
            ("EX012;", "EX0120;"),    // KEYER TYPE, default OFF (0)
            ("EX013;", "EX0130;"),    // KEYER DOT/DASH, default NORMAL (0)
            ("EX014;", "EX01425;"),   // CW WEIGHT, default 25 (min)
            ("EX015;", "EX015000;"),  // BEACON INTERVAL, default 000 (OFF)
            ("EX016;", "EX0160;"),    // NUMBER STYLE, default 0
            ("EX017;", "EX0170000;"), // CONTEST NUMBER, default 0000 (min)
            ("EX018;", "EX0180;"),    // CW MEMORY 1, default TEXT (0)
            ("EX019;", "EX0190;"),    // CW MEMORY 2, default TEXT (0)
            ("EX020;", "EX0200;"),    // CW MEMORY 3, default TEXT (0)
            ("EX021;", "EX0210;"),    // CW MEMORY 4, default TEXT (0)
            ("EX022;", "EX0220;"),    // CW MEMORY 5, default TEXT (0)
            ("EX023;", "EX0230;"),    // NB WIDTH, default 0
            ("EX024;", "EX0240;"),    // NB REJECTION, default 0
            ("EX025;", "EX02500;"),   // NB LEVEL, default 00 (min)
            ("EX026;", "EX026000;"),  // BEEP LEVEL, default 000 (min)
            ("EX028;", "EX0280;"),    // GPS/232C SELECT, default GPS1 (0)
            ("EX029;", "EX0290;"),    // 232C RATE, default 0
            ("EX030;", "EX0300;"),    // 232C TOT, default 0
            ("EX031;", "EX0310;"),    // CAT RATE, default 0
            ("EX032;", "EX0320;"),    // CAT TOT, default 0
            ("EX033;", "EX0330;"),    // CAT RTS, default DISABLE (0)
            ("EX034;", "EX0340;"),    // MEM GROUP, default DISABLE (0)
            ("EX035;", "EX035+00;"),  // QUICK SPLIT FREQ, default 0 (signed)
            ("EX036;", "EX03600;"),   // TX TOT, default 00 (OFF)
            ("EX037;", "EX0370;"),    // MIC SCAN, default DISABLE (0)
            ("EX038;", "EX0380;"),    // MIC SCAN RESUME, default PAUSE (0)
            ("EX039;", "EX039+00;"),  // REF FREQ ADJ, default 0 (signed)
            ("EX040;", "EX0400;"),    // CLAR MODE SELECT, default RX (0)
            ("EX041;", "EX04100;"),   // AM LCUT FREQ, default 00 (OFF)
            ("EX042;", "EX0420;"),    // AM LCUT SLOPE, default 0
            ("EX043;", "EX04300;"),   // AM HCUT FREQ, default 00 (OFF)
            ("EX044;", "EX0440;"),    // AM HCUT SLOPE, default 0
            ("EX045;", "EX0450;"),    // AM MIC SELECT, default MIC (0)
            ("EX046;", "EX046000;"),  // AM OUT LEVEL, default 000 (min)
        ] {
            let mut output = Vec::new();
            framework.process_frame(frame, &mut output).unwrap();
            assert_eq!(String::from_utf8(output).unwrap(), expected, "{frame}");
        }
    }

    #[test]
    fn framework_ex_write_round_trips_for_second_sub_batch_valid_and_invalid_values() {
        // (p1, valid wire values, invalid wire values). Enumerated items
        // use one value just past their legend; Range items use the
        // boundary values (min/max) as valid and one value just outside
        // the range as invalid. Item 017 (CONTEST NUMBER, 0000-9999) has
        // no in-width invalid numeric value — its full 4-digit space is
        // legal — so its invalid case exercises non-numeric rejection
        // instead.
        let cases: &[(u16, &[&str], &[&str])] = &[
            (1, &["0020", "4000", "0040"], &["0000", "4020", "0021"]),
            (2, &["0020", "4000", "0040"], &["0000", "4020", "0021"]),
            (3, &["0020", "4000", "0040"], &["0000", "4020", "0021"]),
            (4, &["0", "1"], &["2"]),
            (5, &["0", "5", "3"], &["6"]),
            (6, &["0", "1", "2", "3", "4", "5", "6"], &["7"]),
            (7, &["0", "1"], &["2"]),
            (8, &["00", "15", "08"], &["16"]),
            (9, &["0", "1", "2", "3"], &["4"]),
            (10, &["000", "100", "050"], &["101"]),
            (11, &["000", "100", "050"], &["101"]),
            (12, &["0", "1", "2", "3", "4", "5"], &["6"]),
            (13, &["0", "1"], &["2"]),
            (14, &["25", "45", "30"], &["24", "46"]),
            (15, &["000", "690", "345"], &["691"]),
            (16, &["0", "1", "2", "3", "4", "5", "6"], &["7"]),
            (17, &["0000", "9999", "1234"], &["abcd"]),
            (18, &["0", "1"], &["2"]),
            (19, &["0", "1"], &["2"]),
            (20, &["0", "1"], &["2"]),
            (21, &["0", "1"], &["2"]),
            (22, &["0", "1"], &["2"]),
            (23, &["0", "1", "2"], &["3"]),
            (24, &["0", "1", "2"], &["3"]),
            (25, &["00", "10", "05"], &["11"]),
            (26, &["000", "100", "050"], &["101"]),
            (28, &["0", "1", "3"], &["2"]), // documented gap at 2
            (29, &["0", "1", "2", "3"], &["4"]),
            (30, &["0", "1", "2", "3"], &["4"]),
            (31, &["0", "1", "2", "3"], &["4"]),
            (32, &["0", "1", "2", "3"], &["4"]),
            (33, &["0", "1"], &["2"]),
            (34, &["0", "1"], &["2"]),
            // "-00" is deliberately excluded from this exact-echo list:
            // it's a legal write (see the dedicated zero-collapse test
            // below) but reads back canonically as "+00", not "-00".
            (35, &["+00", "+20", "-20"], &["+21", "-21"]),
            (36, &["00", "30", "15"], &["31"]),
            (37, &["0", "1"], &["2"]),
            (38, &["0", "1"], &["2"]),
            (39, &["+00", "+25", "-25"], &["+26", "-26"]),
            (40, &["0", "1", "2"], &["3"]),
            (41, &["00", "19", "10"], &["20"]),
            (42, &["0", "1"], &["2"]),
            (43, &["00", "67", "30"], &["68"]),
            (44, &["0", "1"], &["2"]),
            (45, &["0", "1"], &["2"]),
            (46, &["000", "100", "050"], &["101"]),
        ];

        for (p1, valid, invalid) in cases {
            let mut framework = CatFramework::new(Ft991aRadio::new());

            for value in *valid {
                let mut output = Vec::new();
                framework
                    .process_frame(&format!("EX{p1:03}{value};"), &mut output)
                    .unwrap();
                assert!(output.is_empty(), "EX{p1:03}{value}; should write silently");

                output.clear();
                framework
                    .process_frame(&format!("EX{p1:03};"), &mut output)
                    .unwrap();
                assert_eq!(
                    String::from_utf8(output).unwrap(),
                    format!("EX{p1:03}{value};"),
                    "item {p1} did not read back {value}"
                );
            }

            for value in *invalid {
                let mut output = Vec::new();
                framework
                    .process_frame(&format!("EX{p1:03}{value};"), &mut output)
                    .unwrap();
                assert_eq!(
                    String::from_utf8(output).unwrap(),
                    "?;",
                    "item {p1} should reject value {value}"
                );
            }
        }
    }

    #[test]
    fn framework_ex_signed_zero_collapses_to_canonical_plus_zero_on_read() {
        // The manual explicitly allows both "+00" and "-00" for zero on
        // signed-range items (035/039) — a write of either must succeed,
        // but this implementation stores a plain signed integer, so both
        // read back the same canonical "+00" (see EX_MENU_TABLE's doc
        // comment).
        for p1 in [35u16, 39] {
            let mut framework = CatFramework::new(Ft991aRadio::new());
            let mut output = Vec::new();
            framework
                .process_frame(&format!("EX{p1:03}-00;"), &mut output)
                .unwrap();
            assert!(output.is_empty(), "EX{p1:03}-00; should write silently");

            output.clear();
            framework
                .process_frame(&format!("EX{p1:03};"), &mut output)
                .unwrap();
            assert_eq!(
                String::from_utf8(output).unwrap(),
                format!("EX{p1:03}+00;"),
                "item {p1} should canonicalize -00 to +00"
            );
        }
    }

    #[test]
    fn framework_ex_rejects_wrong_digit_width_for_a_multi_digit_second_sub_batch_item() {
        // EX010 (DVS RX OUT LEVEL) is a 3-digit item; a structurally-legal
        // 2-digit write (matches EX_SET_FORMS' width-5 form, used by other
        // menu items) must still be rejected once the per-item digits
        // check runs, and a structurally-legal 4-digit write must be too.
        let mut framework = CatFramework::new(Ft991aRadio::new());

        let mut output = Vec::new();
        framework.process_frame("EX01050;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        output.clear();
        framework.process_frame("EX0100050;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        // State unchanged.
        output.clear();
        framework.process_frame("EX010;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "EX010000;");
    }

    #[test]
    fn framework_ex_second_sub_batch_items_do_not_clobber_first_sub_batch_or_each_other() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("EX0014000;", &mut output).unwrap(); // AGC FAST DELAY -> 4000
        output.clear();
        framework.process_frame("EX035-15;", &mut output).unwrap(); // QUICK SPLIT FREQ -> -15
        output.clear();
        framework.process_frame("EX0602;", &mut output).unwrap(); // PC KEYING (first sub-batch) -> RTS
        output.clear();

        framework.process_frame("EX001;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "EX0014000;");
        output.clear();
        framework.process_frame("EX035;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "EX035-15;");
        output.clear();
        framework.process_frame("EX060;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "EX0602;");
        // A fourth, untouched item stays at its default.
        output.clear();
        framework.process_frame("EX002;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "EX0020020;");
    }

    #[test]
    fn ex_menu_item_finds_all_third_sub_batch_items() {
        for p1 in (49u16..=79).filter(|p1| ![60, 71, 72, 76, 77].contains(p1)) {
            assert!(ex_menu_item(p1).is_some(), "expected item {p1} to exist");
        }
    }

    #[test]
    fn framework_ex_read_returns_default_values_for_third_sub_batch_items() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        for (frame, expected) in [
            ("EX049;", "EX049000;"),   // AM DATA GAIN, default 000 (min)
            ("EX050;", "EX05000;"),    // CW LCUT FREQ, default 00 (OFF)
            ("EX051;", "EX0510;"),     // CW LCUT SLOPE, default 0
            ("EX052;", "EX05200;"),    // CW HCUT FREQ, default 00 (OFF)
            ("EX053;", "EX0530;"),     // CW HCUT SLOPE, default 0
            ("EX054;", "EX054000;"),   // CW OUT LEVEL, default 000 (min)
            ("EX055;", "EX0550;"),     // CW AUTO MODE, default OFF (0)
            ("EX056;", "EX0560;"),     // CW BK-IN TYPE, default SEMI (0)
            ("EX057;", "EX0570030;"),  // CW BK-IN DELAY, default 0030 (min)
            ("EX058;", "EX0580;"),     // CW WAVE SHAPE, default 0
            ("EX059;", "EX0590;"),     // CW FREQ DISPLAY, default DIRECT (0)
            ("EX061;", "EX0610;"),     // QSK DELAY TIME, default 0
            ("EX062;", "EX0620;"),     // DATA MODE, default PSK (0)
            ("EX063;", "EX0630;"),     // PSK TONE, default 0
            ("EX064;", "EX064+0000;"), // OTHER DISP (SSB), default 0 (signed)
            ("EX065;", "EX065+0000;"), // OTHER SHIFT (SSB), default 0 (signed)
            ("EX066;", "EX06600;"),    // DATA LCUT FREQ, default 00 (OFF)
            ("EX067;", "EX0670;"),     // DATA LCUT SLOPE, default 0
            ("EX068;", "EX06800;"),    // DATA HCUT FREQ, default 00 (OFF)
            ("EX069;", "EX0690;"),     // DATA HCUT SLOPE, default 0
            ("EX070;", "EX0700;"),     // DATA IN SELECT, default MIC (0)
            ("EX073;", "EX073000;"),   // DATA OUT LEVEL, default 000 (min)
            ("EX074;", "EX0740;"),     // FM MIC SELECT, default MIC (0)
            ("EX075;", "EX075000;"),   // FM OUT LEVEL, default 000 (min)
            ("EX078;", "EX078000;"),   // FM PKT TX GAIN, default 000 (min)
            ("EX079;", "EX0790;"),     // FM PKT MODE, default 0
        ] {
            let mut output = Vec::new();
            framework.process_frame(frame, &mut output).unwrap();
            assert_eq!(String::from_utf8(output).unwrap(), expected, "{frame}");
        }
    }

    #[test]
    fn framework_ex_write_round_trips_for_third_sub_batch_valid_and_invalid_values() {
        let cases: &[(u16, &[&str], &[&str])] = &[
            (49, &["000", "100", "050"], &["101"]),
            (50, &["00", "19", "10"], &["20"]),
            (51, &["0", "1"], &["2"]),
            (52, &["00", "67", "30"], &["68"]),
            (53, &["0", "1"], &["2"]),
            (54, &["000", "100", "050"], &["101"]),
            (55, &["0", "1", "2"], &["3"]),
            (56, &["0", "1"], &["2"]),
            (57, &["0030", "3000", "1500"], &["0020", "3010", "0031"]),
            (58, &["0", "1", "2", "3"], &["4"]),
            (59, &["0", "1"], &["2"]),
            (61, &["0", "1", "2", "3"], &["4"]),
            (62, &["0", "1"], &["2"]),
            (63, &["0", "1", "2"], &["3"]),
            // "-0000" is deliberately excluded from this exact-echo list,
            // same reasoning as 035/039: legal to write, but reads back
            // canonically as "+0000" (see the dedicated test below).
            (
                64,
                &["+0000", "+3000", "-3000", "+0010"],
                &["+3010", "-3010", "+0005"],
            ),
            (
                65,
                &["+0000", "+3000", "-3000", "+0010"],
                &["+3010", "-3010", "+0005"],
            ),
            (66, &["00", "19", "10"], &["20"]),
            (67, &["0", "1"], &["2"]),
            (68, &["00", "67", "30"], &["68"]),
            (69, &["0", "1"], &["2"]),
            (70, &["0", "1"], &["2"]),
            (73, &["000", "100", "050"], &["101"]),
            (74, &["0", "1"], &["2"]),
            (75, &["000", "100", "050"], &["101"]),
            (78, &["000", "100", "050"], &["101"]),
            (79, &["0", "1"], &["2"]),
        ];

        for (p1, valid, invalid) in cases {
            let mut framework = CatFramework::new(Ft991aRadio::new());

            for value in *valid {
                let mut output = Vec::new();
                framework
                    .process_frame(&format!("EX{p1:03}{value};"), &mut output)
                    .unwrap();
                assert!(output.is_empty(), "EX{p1:03}{value}; should write silently");

                output.clear();
                framework
                    .process_frame(&format!("EX{p1:03};"), &mut output)
                    .unwrap();
                assert_eq!(
                    String::from_utf8(output).unwrap(),
                    format!("EX{p1:03}{value};"),
                    "item {p1} did not read back {value}"
                );
            }

            for value in *invalid {
                let mut output = Vec::new();
                framework
                    .process_frame(&format!("EX{p1:03}{value};"), &mut output)
                    .unwrap();
                assert_eq!(
                    String::from_utf8(output).unwrap(),
                    "?;",
                    "item {p1} should reject value {value}"
                );
            }
        }
    }

    #[test]
    fn framework_ex_third_sub_batch_signed_zero_collapses_to_canonical_plus_zero_on_read() {
        // Same dual-zero-encoding treatment as 035/039 (see the dedicated
        // test above), just with this sub-batch's wider 4-digit magnitude.
        for p1 in [64u16, 65] {
            let mut framework = CatFramework::new(Ft991aRadio::new());
            let mut output = Vec::new();
            framework
                .process_frame(&format!("EX{p1:03}-0000;"), &mut output)
                .unwrap();
            assert!(output.is_empty(), "EX{p1:03}-0000; should write silently");

            output.clear();
            framework
                .process_frame(&format!("EX{p1:03};"), &mut output)
                .unwrap();
            assert_eq!(
                String::from_utf8(output).unwrap(),
                format!("EX{p1:03}+0000;"),
                "item {p1} should canonicalize -0000 to +0000"
            );
        }
    }

    #[test]
    fn framework_ex_068_069_digit_width_resolution_matches_functionally_necessary_2_1_order() {
        // See EX_MENU_TABLE's doc comment and the module docs' "EX menu,
        // third sub-batch" section: the manual's own printed Digits column
        // literally shows 068=1/069=2, but that is functionally impossible
        // for 068 (legend needs values up to 67, i.e. 2 digits) and
        // contradicts every sibling *HCUT FREQ/SLOPE pair on the page
        // (always 2/1). Implemented as 068=2/069=1 — this test locks that
        // resolution in directly, independent of the general round-trip
        // loop above.
        assert_eq!(ex_menu_item(68).unwrap().digits, 2);
        assert_eq!(ex_menu_item(69).unwrap().digits, 1);

        let mut framework = CatFramework::new(Ft991aRadio::new());

        // 068 (2-digit) accepts its full 00-67 legend range...
        let mut output = Vec::new();
        framework.process_frame("EX06867;", &mut output).unwrap();
        assert!(output.is_empty());
        // ...and rejects a would-be-1-digit-shaped write outright (wrong
        // structural width for this item, "?;" not a panic).
        output.clear();
        framework.process_frame("EX0686;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        // 069 (1-digit) accepts its 2-value legend...
        output.clear();
        framework.process_frame("EX0691;", &mut output).unwrap();
        assert!(output.is_empty());
        // ...and rejects a would-be-2-digit-shaped write.
        output.clear();
        framework.process_frame("EX06901;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_ex_rejects_wrong_digit_width_for_a_multi_digit_third_sub_batch_item() {
        // EX057 (CW BK-IN DELAY) is a 4-digit item; a structurally-legal
        // 3-digit write (matches EX_SET_FORMS' width-6 form, used by other
        // menu items) must still be rejected once the per-item digits
        // check runs, and a structurally-legal 5-digit write must be too.
        let mut framework = CatFramework::new(Ft991aRadio::new());

        let mut output = Vec::new();
        framework.process_frame("EX057150;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        output.clear();
        framework.process_frame("EX05701500;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        // State unchanged.
        output.clear();
        framework.process_frame("EX057;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "EX0570030;");
    }

    #[test]
    fn framework_ex_third_sub_batch_items_do_not_clobber_first_or_second_sub_batch_or_each_other() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("EX0014000;", &mut output).unwrap(); // AGC FAST DELAY (2nd) -> 4000
        output.clear();
        framework.process_frame("EX0602;", &mut output).unwrap(); // PC KEYING (1st) -> RTS
        output.clear();
        framework.process_frame("EX05267;", &mut output).unwrap(); // CW HCUT FREQ (3rd) -> 67
        output.clear();
        framework.process_frame("EX064-0500;", &mut output).unwrap(); // OTHER DISP (3rd) -> -500
        output.clear();

        framework.process_frame("EX001;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "EX0014000;");
        output.clear();
        framework.process_frame("EX060;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "EX0602;");
        output.clear();
        framework.process_frame("EX052;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "EX05267;");
        output.clear();
        framework.process_frame("EX064;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "EX064-0500;");
        // A fifth, untouched item stays at its default.
        output.clear();
        framework.process_frame("EX065;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "EX065+0000;");
    }

    #[test]
    fn ex_menu_item_finds_all_fourth_sub_batch_items() {
        for p1 in (80u16..=153).filter(|p1| ![87, 108, 109].contains(p1)) {
            assert!(ex_menu_item(p1).is_some(), "expected item {p1} to exist");
        }
    }

    #[test]
    fn framework_ex_read_returns_default_values_for_fourth_sub_batch_items() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        for (frame, expected) in [
            ("EX080;", "EX0800000;"),     // RPT SHIFT 28MHz, default 0000
            ("EX081;", "EX0810000;"),     // RPT SHIFT 50MHz, default 0000
            ("EX082;", "EX0820000;"),     // RPT SHIFT 144MHz, default 0000
            ("EX083;", "EX08300000;"),    // RPT SHIFT 430MHz, default 00000
            ("EX084;", "EX0840;"),        // ARS 144MHz, default OFF (0)
            ("EX085;", "EX0850;"),        // ARS 430MHz, default OFF (0)
            ("EX086;", "EX0860;"),        // DCS POLARITY, default Tn-Rn (0)
            ("EX088;", "EX0880;"),        // GM DISPLY, default DISTANCE (0)
            ("EX089;", "EX0890;"),        // DISTANCE, default km (0)
            ("EX090;", "EX0900;"),        // AMS TX MODE, default AUTO (0)
            ("EX091;", "EX0910;"),        // STANDBY BEEP, default OFF (0)
            ("EX092;", "EX09200;"),       // RTTY LCUT FREQ, default 00 (OFF)
            ("EX093;", "EX0930;"),        // RTTY LCUT SLOPE, default 0
            ("EX094;", "EX09400;"),       // RTTY HCUT FREQ, default 00 (OFF)
            ("EX095;", "EX0950;"),        // RTTY HCUT SLOPE, default 0
            ("EX096;", "EX0960;"),        // RTTY SHIFT PORT, default SHIFT (0)
            ("EX097;", "EX0970;"),        // RTTY POLARITY-RX, default NORMAL (0)
            ("EX098;", "EX0980;"),        // RTTY POLARITY-TX, default NORMAL (0)
            ("EX099;", "EX099000;"),      // RTTY OUT LEVEL, default 000
            ("EX100;", "EX1000;"),        // RTTY SHIFT FREQ, default 170Hz (0)
            ("EX101;", "EX1011;"),        // RTTY MARK FREQ, default 1275Hz (1)
            ("EX102;", "EX10200;"),       // SSB LCUT FREQ, default 00 (OFF)
            ("EX103;", "EX1030;"),        // SSB LCUT SLOPE, default 0
            ("EX104;", "EX10400;"),       // SSB HCUT FREQ, default 00 (OFF)
            ("EX105;", "EX1050;"),        // SSB HCUT SLOPE, default 0
            ("EX106;", "EX1060;"),        // SSB MIC SELECT, default MIC (0)
            ("EX107;", "EX107000;"),      // SSB OUT LEVEL, default 000
            ("EX110;", "EX1100;"),        // SSB TX BPF, default 0
            ("EX111;", "EX1110;"),        // APF WIDTH, default NARROW (0)
            ("EX112;", "EX112+00;"),      // CONTOUR LEVEL, default 0 (signed)
            ("EX113;", "EX11301;"),       // CONTOUR WIDTH, default 01 (min)
            ("EX114;", "EX1140;"),        // IF NOTCH WIDTH, default NARROW (0)
            ("EX115;", "EX1150;"),        // SCP DISPLAY MODE, default SPECTRUM (0)
            ("EX116;", "EX11603;"),       // SCP SPAN FREQ, default 03 (first legal)
            ("EX117;", "EX1170;"),        // SPECTRUM COLOR, default BLUE (0)
            ("EX118;", "EX1180;"),        // WATER FALL COLOR, default BLUE (0)
            ("EX119;", "EX11900;"),       // PRMTRC EQ1 FREQ, default 00 (OFF)
            ("EX120;", "EX120+00;"),      // PRMTRC EQ1 LEVEL, default 0 (signed)
            ("EX121;", "EX12101;"),       // PRMTRC EQ1 BWTH, default 01 (min)
            ("EX122;", "EX12200;"),       // PRMTRC EQ2 FREQ, default 00 (OFF)
            ("EX123;", "EX123+00;"),      // PRMTRC EQ2 LEVEL, default 0 (signed)
            ("EX124;", "EX12401;"),       // PRMTRC EQ2 BWTH, default 01 (min)
            ("EX125;", "EX12500;"),       // PRMTRC EQ3 FREQ, default 00 (OFF)
            ("EX126;", "EX126+00;"),      // PRMTRC EQ3 LEVEL, default 0 (signed)
            ("EX127;", "EX12701;"),       // PRMTRC EQ3 BWTH, default 01 (min)
            ("EX128;", "EX12800;"),       // P-PRMTRC EQ1 FREQ, default 00 (OFF)
            ("EX129;", "EX129+00;"),      // P-PRMTRC EQ1 LEVEL, default 0 (signed)
            ("EX130;", "EX13001;"),       // P-PRMTRC EQ1 BWTH, default 01 (min)
            ("EX131;", "EX13100;"),       // P-PRMTRC EQ2 FREQ, default 00 (OFF)
            ("EX132;", "EX132+00;"),      // P-PRMTRC EQ2 LEVEL, default 0 (signed)
            ("EX133;", "EX13301;"),       // P-PRMTRC EQ2 BWTH, default 01 (min)
            ("EX134;", "EX13400;"),       // P-PRMTRC EQ3 FREQ, default 00 (OFF)
            ("EX135;", "EX135+00;"),      // P-PRMTRC EQ3 LEVEL, default 0 (signed)
            ("EX136;", "EX13601;"),       // P-PRMTRC EQ3 BWTH, default 01 (min)
            ("EX137;", "EX137005;"),      // HF TX MAX POWER, default 005 (min)
            ("EX138;", "EX138005;"),      // 50M TX MAX POWER, default 005 (min)
            ("EX139;", "EX139005;"),      // 144M TX MAX POWER, default 005 (min)
            ("EX140;", "EX140005;"),      // 430M TX MAX POWER, default 005 (min)
            ("EX141;", "EX1410;"),        // TUNER SELECT, default OFF (0)
            ("EX142;", "EX1420;"),        // VOX SELECT, default MIC (0)
            ("EX143;", "EX143000;"),      // VOX GAIN, default 000
            ("EX144;", "EX1440030;"),     // VOX DELAY, default 0030 (min)
            ("EX145;", "EX145000;"),      // ANTI VOX GAIN, default 000
            ("EX146;", "EX146000;"),      // DATA VOX GAIN, default 000
            ("EX147;", "EX1470030;"),     // DATA VOX DELAY, default 0030 (min)
            ("EX148;", "EX148000;"),      // ANTI DVOX GAIN, default 000
            ("EX149;", "EX1490;"),        // EMERGENCY FREQ TX, default DISABLE (0)
            ("EX150;", "EX1500;"),        // PRT/WIRES FREQ, default MANUAL (0)
            ("EX151;", "EX15100030000;"), // PRESET FREQUENCY, default 00030000 (min)
            ("EX152;", "EX1520;"),        // SEARCH SETUP, default HISTORY (0)
            ("EX153;", "EX15300;"),       // WIRES DG-ID, default 00 (AUTO)
        ] {
            let mut output = Vec::new();
            framework.process_frame(frame, &mut output).unwrap();
            assert_eq!(String::from_utf8(output).unwrap(), expected, "{frame}");
        }
    }

    #[test]
    fn framework_ex_write_round_trips_for_fourth_sub_batch_valid_and_invalid_values() {
        let cases: &[(u16, &[&str], &[&str])] = &[
            (80, &["0000", "1000", "0500"], &["1010"]),
            (81, &["0000", "4000", "2000"], &["4010"]),
            (82, &["0000", "4000", "2000"], &["4010"]),
            (83, &["00000", "10000", "05000"], &["10010"]),
            (84, &["0", "1"], &["2"]),
            (85, &["0", "1"], &["2"]),
            (86, &["0", "1", "2", "3"], &["4"]),
            (88, &["0", "1"], &["2"]),
            (89, &["0", "1"], &["2"]),
            (90, &["0", "1", "2", "3", "4"], &["5"]),
            (91, &["0", "1"], &["2"]),
            (92, &["00", "19", "10"], &["20"]),
            (93, &["0", "1"], &["2"]),
            (94, &["00", "67", "30"], &["68"]),
            (95, &["0", "1"], &["2"]),
            (96, &["0", "1", "2"], &["3"]),
            (97, &["0", "1"], &["2"]),
            (98, &["0", "1"], &["2"]),
            (99, &["000", "100", "050"], &["101"]),
            (100, &["0", "1", "2", "3"], &["4"]),
            // "0" is deliberately excluded from 101's own round-trip write
            // (it is 101's rejection case, not a legal write) — this item's
            // legend genuinely starts at 1 (see EX_MENU_TABLE's doc
            // comment), same as 072/077/109.
            (101, &["1", "2"], &["0"]),
            (102, &["00", "19", "10"], &["20"]),
            (103, &["0", "1"], &["2"]),
            (104, &["00", "67", "30"], &["68"]),
            (105, &["0", "1"], &["2"]),
            (106, &["0", "1"], &["2"]),
            (107, &["000", "100", "050"], &["101"]),
            (110, &["0", "1", "2", "3", "4"], &["5"]),
            (111, &["0", "1", "2"], &["3"]),
            (112, &["-40", "+20", "+00"], &["+21", "-41"]),
            (113, &["01", "10", "05"], &["00", "11"]),
            (114, &["0", "1"], &["2"]),
            (115, &["0", "1"], &["2"]),
            // 116's legal values are 03-07 only — a documented gap, not a
            // width issue; 02 and 08 are both structurally 2-digit but
            // semantically illegal.
            (116, &["03", "07", "05"], &["02", "08"]),
            (117, &["0", "6", "3"], &["7"]),
            (118, &["0", "7", "4"], &["8"]),
            (119, &["00", "07", "04"], &["08"]),
            (120, &["-20", "+10", "+00"], &["+11", "-21"]),
            (121, &["01", "10"], &["00", "11"]),
            (122, &["00", "09"], &["10"]),
            (123, &["-20", "+10", "+00"], &["+11", "-21"]),
            (124, &["01", "10"], &["11"]),
            (125, &["00", "18"], &["19"]),
            (126, &["-20", "+10", "+00"], &["+11", "-21"]),
            (127, &["01", "10"], &["11"]),
            (128, &["00", "07"], &["08"]),
            (129, &["-20", "+10", "+00"], &["+11", "-21"]),
            (130, &["01", "10"], &["11"]),
            (131, &["00", "09"], &["10"]),
            (132, &["-20", "+10", "+00"], &["+11", "-21"]),
            (133, &["01", "10"], &["11"]),
            (134, &["00", "18"], &["19"]),
            (135, &["-20", "+10", "+00"], &["+11", "-21"]),
            (136, &["01", "10"], &["11"]),
            (137, &["005", "100", "050"], &["004", "101"]),
            (138, &["005", "100", "050"], &["004", "101"]),
            (139, &["005", "050"], &["004", "051"]),
            (140, &["005", "050"], &["004", "051"]),
            (141, &["0", "4"], &["5"]),
            (142, &["0", "1"], &["2"]),
            (143, &["000", "100"], &["101"]),
            (144, &["0030", "3000", "1500"], &["0020", "3010", "0031"]),
            (145, &["000", "100"], &["101"]),
            (146, &["000", "100"], &["101"]),
            (147, &["0030", "3000", "1500"], &["0020", "3010", "0031"]),
            (148, &["000", "100"], &["101"]),
            (149, &["0", "1"], &["2"]),
            (150, &["0", "1"], &["2"]),
            (
                151,
                &["00030000", "47000000", "00500000"],
                &["00029999", "47000001"],
            ),
            (152, &["0", "1"], &["2"]),
            // 153's legal range (0-99) covers its full 2-digit domain, so
            // there is no same-width out-of-range value; "-1" (still
            // exactly 2 wire characters) exercises the below-`min`
            // rejection path instead.
            (153, &["00", "99", "50"], &["-1"]),
        ];

        for (p1, valid, invalid) in cases {
            let mut framework = CatFramework::new(Ft991aRadio::new());

            for value in *valid {
                let mut output = Vec::new();
                framework
                    .process_frame(&format!("EX{p1:03}{value};"), &mut output)
                    .unwrap();
                assert!(output.is_empty(), "EX{p1:03}{value}; should write silently");

                output.clear();
                framework
                    .process_frame(&format!("EX{p1:03};"), &mut output)
                    .unwrap();
                assert_eq!(
                    String::from_utf8(output).unwrap(),
                    format!("EX{p1:03}{value};"),
                    "item {p1} did not read back {value}"
                );
            }

            for value in *invalid {
                let mut output = Vec::new();
                framework
                    .process_frame(&format!("EX{p1:03}{value};"), &mut output)
                    .unwrap();
                assert_eq!(
                    String::from_utf8(output).unwrap(),
                    "?;",
                    "item {p1} should reject value {value}"
                );
            }
        }
    }

    #[test]
    fn framework_ex_fourth_sub_batch_signed_zero_collapses_to_canonical_plus_zero_on_read() {
        // Same dual-zero-encoding treatment as 035/039/064/065 — just with
        // this sub-batch's 3-digit (magnitude width 2) signed items.
        for p1 in [112u16, 120, 123, 126, 129, 132, 135] {
            let mut framework = CatFramework::new(Ft991aRadio::new());
            let mut output = Vec::new();
            framework
                .process_frame(&format!("EX{p1:03}-00;"), &mut output)
                .unwrap();
            assert!(output.is_empty(), "EX{p1:03}-00; should write silently");

            output.clear();
            framework
                .process_frame(&format!("EX{p1:03};"), &mut output)
                .unwrap();
            assert_eq!(
                String::from_utf8(output).unwrap(),
                format!("EX{p1:03}+00;"),
                "item {p1} should canonicalize -00 to +00"
            );
        }
    }

    #[test]
    fn framework_ex_100_rtty_shift_freq_typo_resolution_is_zero_based() {
        // See EX_MENU_TABLE's doc comment: the manual's own printed legend
        // for 100 "RTTY SHIFT FREQ" has a duplicate "1:" label ("1: 170 Hz
        // 1: 200 Hz 2: 425 Hz 3: 850 Hz"), resolved to 0-based
        // (0:170Hz 1:200Hz 2:425Hz 3:850Hz) via corroborating evidence, not
        // a guess. This test locks that resolution in directly.
        assert_eq!(ex_menu_item(100).unwrap().digits, 1);
        let mut framework = CatFramework::new(Ft991aRadio::new());

        // Default (unwritten) reads back as 0 (170 Hz).
        let mut output = Vec::new();
        framework.process_frame("EX100;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "EX1000;");

        // All four resolved values are legal.
        for value in ["0", "1", "2", "3"] {
            output.clear();
            framework
                .process_frame(&format!("EX100{value};"), &mut output)
                .unwrap();
            assert!(output.is_empty());
        }

        // A fifth value (which would only be legal under a 1-based-through-4
        // reading) is rejected.
        output.clear();
        framework.process_frame("EX1004;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_ex_116_scp_span_freq_gap_rejects_00_through_02() {
        // See EX_MENU_TABLE's doc comment: 116's legal values are 03-07
        // only — 00-02 are structurally 2-digit but absent from the
        // manual's own legend, same treatment as 028's GPS/232C SELECT gap.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        for value in ["00", "01", "02"] {
            let mut output = Vec::new();
            framework
                .process_frame(&format!("EX116{value};"), &mut output)
                .unwrap();
            assert_eq!(
                String::from_utf8(output).unwrap(),
                "?;",
                "EX116{value}; should be rejected (documented gap)"
            );
        }
    }

    #[test]
    fn framework_ex_rejects_wrong_digit_width_for_a_multi_digit_fourth_sub_batch_item() {
        // EX151 (PRESET FREQUENCY) is an 8-digit item; a structurally-legal
        // 5-digit write (matches EX_SET_FORMS' width-8 form, used by other
        // menu items) must still be rejected once the per-item digits
        // check runs, and a structurally-legal 4-digit write must be too.
        let mut framework = CatFramework::new(Ft991aRadio::new());

        let mut output = Vec::new();
        framework.process_frame("EX15105000;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        output.clear();
        framework.process_frame("EX1510500;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        // State unchanged.
        output.clear();
        framework.process_frame("EX151;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "EX15100030000;");
    }

    #[test]
    fn framework_ex_fourth_sub_batch_items_do_not_clobber_earlier_sub_batches_or_each_other() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("EX0014000;", &mut output).unwrap(); // AGC FAST DELAY (2nd) -> 4000
        output.clear();
        framework.process_frame("EX0602;", &mut output).unwrap(); // PC KEYING (1st) -> RTS
        output.clear();
        framework.process_frame("EX05267;", &mut output).unwrap(); // CW HCUT FREQ (3rd) -> 67
        output.clear();
        framework.process_frame("EX0863;", &mut output).unwrap(); // DCS POLARITY (4th) -> Tiv-Riv
        output.clear();
        framework
            .process_frame("EX15147000000;", &mut output)
            .unwrap(); // PRESET FREQUENCY (4th) -> max
        output.clear();

        framework.process_frame("EX001;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "EX0014000;");
        output.clear();
        framework.process_frame("EX060;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "EX0602;");
        output.clear();
        framework.process_frame("EX052;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "EX05267;");
        output.clear();
        framework.process_frame("EX086;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "EX0863;");
        output.clear();
        framework.process_frame("EX151;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "EX15147000000;");
        // A sixth, untouched fourth-sub-batch item stays at its default.
        output.clear();
        framework.process_frame("EX100;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "EX1000;");
    }

    // -----------------------------------------------------------------
    // Batch 2: memory channel records (`MC MR MW MT`).
    // -----------------------------------------------------------------

    /// The default-state body all three of `MR`/`MT`/`IF`-shaped answers
    /// share for channel 1 (never written): freq 0, no clarifier, mode
    /// USB (2), select 1 (Memory — `MR`/`MT` always report this), tone 0,
    /// offset type 0. Verified by hand against `ChannelStatusFields`'s
    /// `to_wire_string` field order (see the struct-level round-trip
    /// tests above) and reused across several tests below rather than
    /// re-typing a 25-character literal repeatedly.
    fn default_channel1_body() -> String {
        ChannelStatusFields {
            channel: 1,
            frequency_hz: 0,
            clarifier_offset_hz: 0,
            rx_clarifier_on: false,
            tx_clarifier_on: false,
            mode: 0x2,
            select: 1,
            tone_status: 0,
            offset_type: 0,
        }
        .to_wire_string()
    }

    #[test]
    fn framework_mc_query_default_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("MC;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MC001;");

        output.clear();
        framework.process_frame("MC117;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("MC;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MC117;");
    }

    #[test]
    fn framework_mc_rejects_out_of_range_channel() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // 000 and 118 are both out of the documented 001-117 range.
        framework.process_frame("MC000;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        output.clear();
        framework.process_frame("MC118;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        // State unchanged after both rejections.
        output.clear();
        framework.process_frame("MC;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "MC001;");
    }

    #[test]
    fn framework_mr_query_default_state() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("MR001;", &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            format!("MR{};", default_channel1_body())
        );
    }

    #[test]
    fn framework_mr_rejects_out_of_range_channel() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // MR's documented range is 001-117, unlike IF's channel field,
        // which additionally accepts 000 as this crate's VFO-mode
        // sentinel — MR has no VFO case at all.
        framework.process_frame("MR000;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        output.clear();
        framework.process_frame("MR118;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_mr_is_read_only_no_set_form() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        // A structurally-plausible 25-byte "write" is not a legal MR form
        // at all (MR's only set_forms width is 3) — the framework itself
        // rejects it as an unsupported parameter width.
        framework
            .process_frame(&format!("MR{};", default_channel1_body()), &mut output)
            .unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_mw_write_then_mr_read_round_trips() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        let write_fields = ChannelStatusFields {
            channel: 5,
            frequency_hz: 7_100_000,
            clarifier_offset_hz: -250,
            rx_clarifier_on: true,
            tx_clarifier_on: false,
            mode: 0x4, // FM
            select: 0, // fixed on write, see MemoryChannelRecord's doc comment
            tone_status: 2,
            offset_type: 1,
        };
        framework
            .process_frame(
                &format!("MW{};", write_fields.to_wire_string()),
                &mut output,
            )
            .unwrap();
        assert!(output.is_empty(), "MW has no answer (manual p.3: Ans=X)");

        output.clear();
        framework.process_frame("MR005;", &mut output).unwrap();
        let expected_read = ChannelStatusFields {
            select: 1, // MR always reports Memory, see struct docs
            ..write_fields
        };
        assert_eq!(
            String::from_utf8(output.clone()).unwrap(),
            format!("MR{};", expected_read.to_wire_string())
        );

        // A different, untouched channel stays at its default.
        output.clear();
        framework.process_frame("MR006;", &mut output).unwrap();
        let default_ch6 = ChannelStatusFields {
            channel: 6,
            ..ChannelStatusFields::parse(&default_channel1_body()).unwrap()
        };
        assert_eq!(
            String::from_utf8(output).unwrap(),
            format!("MR{};", default_ch6.to_wire_string())
        );
    }

    #[test]
    fn framework_mw_rejects_channel_zero_and_non_fixed_select() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        let base = ChannelStatusFields {
            channel: 0, // not a legal memory channel for MW (001-117 only)
            frequency_hz: 14_000_000,
            clarifier_offset_hz: 0,
            rx_clarifier_on: false,
            tx_clarifier_on: false,
            mode: 0x2,
            select: 0,
            tone_status: 0,
            offset_type: 0,
        };
        framework
            .process_frame(&format!("MW{};", base.to_wire_string()), &mut output)
            .unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        // channel=1 but select != 0 (manual: MW's P7 is fixed "0").
        output.clear();
        let bad_select = ChannelStatusFields {
            channel: 1,
            select: 3,
            ..base
        };
        framework
            .process_frame(&format!("MW{};", bad_select.to_wire_string()), &mut output)
            .unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_mt_write_then_read_round_trips_including_tag() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        let fields = ChannelStatusFields {
            channel: 42,
            frequency_hz: 14_250_000,
            clarifier_offset_hz: 1200,
            rx_clarifier_on: true,
            tx_clarifier_on: false,
            mode: 0x3,
            select: 0, // fixed on write
            tone_status: 2,
            offset_type: 1,
        };
        let tag = "REPEATER 1"; // 10 chars — shorter than the 12-char limit
        let write_body = format!("{}0{:<12}", fields.to_wire_string(), tag);
        assert_eq!(write_body.len(), 38);
        framework
            .process_frame(&format!("MT{write_body};"), &mut output)
            .unwrap();
        assert!(output.is_empty(), "MT write has no answer");

        output.clear();
        framework.process_frame("MT042;", &mut output).unwrap();
        let expected_read = ChannelStatusFields {
            select: 1,
            ..fields
        };
        assert_eq!(
            String::from_utf8(output).unwrap(),
            format!("MT{}0{tag:<12};", expected_read.to_wire_string())
        );
    }

    #[test]
    fn framework_mt_tag_shortest_and_longest_legal_content_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        let base = ChannelStatusFields {
            channel: 10,
            frequency_hz: 3_573_000,
            clarifier_offset_hz: 0,
            rx_clarifier_on: false,
            tx_clarifier_on: false,
            mode: 0x2,
            select: 0,
            tone_status: 0,
            offset_type: 0,
        };

        // Shortest legal content: an empty tag (0 characters — the wire
        // slot is still 12 bytes wide, all spaces).
        framework
            .process_frame(
                &format!("MT{}0{:<12};", base.to_wire_string(), ""),
                &mut output,
            )
            .unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("MT010;", &mut output).unwrap();
        let expect_empty = ChannelStatusFields { select: 1, ..base };
        assert_eq!(
            String::from_utf8(output.clone()).unwrap(),
            format!("MT{}0{:<12};", expect_empty.to_wire_string(), "")
        );

        // Longest legal content: exactly 12 characters (no padding
        // needed at all).
        output.clear();
        let full_tag = "ABCDEFGHIJKL"; // exactly 12 chars
        assert_eq!(full_tag.len(), 12);
        framework
            .process_frame(
                &format!("MT{}0{full_tag};", base.to_wire_string()),
                &mut output,
            )
            .unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("MT010;", &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            format!("MT{}0{full_tag};", expect_empty.to_wire_string())
        );
    }

    #[test]
    fn framework_mt_rejects_tag_with_control_character() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        let fields = ChannelStatusFields {
            channel: 10,
            frequency_hz: 7_000_000,
            clarifier_offset_hz: 0,
            rx_clarifier_on: false,
            tx_clarifier_on: false,
            mode: 0x2,
            select: 0,
            tone_status: 0,
            offset_type: 0,
        };
        // A control character (0x01) inside the tag slot must be
        // rejected per the character-set restriction documented on
        // `MemoryChannelRecord` (manual p.2's general parameter rule).
        let bad_tag = format!("{:<12}", "BAD\u{1}TAG");
        assert_eq!(bad_tag.len(), 12);
        framework
            .process_frame(
                &format!("MT{}0{bad_tag};", fields.to_wire_string()),
                &mut output,
            )
            .unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        // State unchanged: reading channel 10 still shows the default.
        output.clear();
        framework.process_frame("MT010;", &mut output).unwrap();
        let default_read = ChannelStatusFields {
            channel: 10,
            select: 1,
            ..ChannelStatusFields::parse(&default_channel1_body()).unwrap()
        };
        assert_eq!(
            String::from_utf8(output).unwrap(),
            format!("MT{}0{:<12};", default_read.to_wire_string(), "")
        );
    }

    #[test]
    fn framework_mt_rejects_out_of_range_channel() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("MT000;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        output.clear();
        framework.process_frame("MT118;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_mt_write_rejects_non_fixed_p7() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        let fields = ChannelStatusFields {
            channel: 7,
            frequency_hz: 14_000_000,
            clarifier_offset_hz: 0,
            rx_clarifier_on: false,
            tx_clarifier_on: false,
            mode: 0x2,
            select: 2, // manual: MT's Set-direction P7 is fixed "0"
            tone_status: 0,
            offset_type: 0,
        };
        framework
            .process_frame(
                &format!("MT{}0{:<12};", fields.to_wire_string(), ""),
                &mut output,
            )
            .unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    // -----------------------------------------------------------------
    // Batch 1: VFO/split/memory quick-ops (`AB BA AM VM MA CH QI QR QS
    // SV`).
    // -----------------------------------------------------------------

    #[test]
    fn framework_ab_copies_vfo_a_into_vfo_b() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework
            .process_frame("FA014250000;", &mut output)
            .unwrap();
        assert!(output.is_empty());

        framework.process_frame("AB;", &mut output).unwrap();
        assert!(output.is_empty(), "AB is a zero-response Action trigger");

        framework.process_frame("FB;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "FB014250000;");
    }

    #[test]
    fn framework_ba_copies_vfo_b_into_vfo_a() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework
            .process_frame("FB007100000;", &mut output)
            .unwrap();
        assert!(output.is_empty());

        framework.process_frame("BA;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("FA;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "FA007100000;");
    }

    #[test]
    fn framework_sv_swaps_vfo_a_and_vfo_b() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework
            .process_frame("FA014250000;", &mut output)
            .unwrap();
        framework
            .process_frame("FB007100000;", &mut output)
            .unwrap();
        output.clear();

        framework.process_frame("SV;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("FA;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "FA007100000;");
        output.clear();
        framework.process_frame("FB;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "FB014250000;");
    }

    #[test]
    fn framework_ab_ba_sv_reject_a_parameter() {
        // These are genuine zero-width Action triggers, not Set commands —
        // any parameter is structurally illegal (no matching form at all).
        for frame in ["AB1;", "BA1;", "SV1;"] {
            let mut framework = CatFramework::new(Ft991aRadio::new());
            let mut output = Vec::new();
            framework.process_frame(frame, &mut output).unwrap();
            assert_eq!(
                String::from_utf8(output).unwrap(),
                "?;",
                "{frame} should be rejected"
            );
        }
    }

    #[test]
    fn framework_am_stores_vfo_a_into_selected_memory_channel() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("MC042;", &mut output).unwrap();
        framework
            .process_frame("FA014250000;", &mut output)
            .unwrap();
        framework.process_frame("MD03;", &mut output).unwrap(); // CW-U
        output.clear();

        framework.process_frame("AM;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("MR042;", &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output.clone()).unwrap(),
            "MR042014250000+000000310000;",
            "channel 42 should now hold VFO-A's frequency and mode (CW-U = 3)"
        );
    }

    #[test]
    fn framework_ma_recalls_selected_memory_channel_into_vfo_a() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // Write channel 10 directly via MW, then select it via MC.
        let fields = ChannelStatusFields {
            channel: 10,
            frequency_hz: 3_573_000,
            clarifier_offset_hz: 250,
            rx_clarifier_on: true,
            tx_clarifier_on: false,
            mode: 0x4, // FM
            select: 0,
            tone_status: 1,
            offset_type: 0,
        };
        framework
            .process_frame(&format!("MW{};", fields.to_wire_string()), &mut output)
            .unwrap();
        framework.process_frame("MC010;", &mut output).unwrap();
        output.clear();

        framework.process_frame("MA;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("FA;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "FA003573000;");
        output.clear();
        framework.process_frame("MD0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MD04;");
    }

    #[test]
    fn framework_am_ma_round_trip_via_mc_selected_channel() {
        // Combined AM/MA + MC interaction test: store VFO-A into channel 5
        // via AM, change VFO-A, then recall channel 5 back via MA and
        // confirm the original value returns.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("MC005;", &mut output).unwrap();
        framework
            .process_frame("FA028000000;", &mut output)
            .unwrap();
        output.clear();

        framework.process_frame("AM;", &mut output).unwrap();
        assert!(output.is_empty());

        // Change VFO-A away from the stored value.
        framework
            .process_frame("FA014000000;", &mut output)
            .unwrap();
        output.clear();

        framework.process_frame("MA;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("FA;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "FA028000000;");
    }

    #[test]
    fn framework_vm_toggles_channel_select_between_vfo_and_memory() {
        let mut framework = CatFramework::new(Ft991aRadio::new());

        // Default state: channel_select = 0 (VFO).
        assert_eq!(framework.radio().state().channel_select, 0);

        let mut output = Vec::new();
        framework.process_frame("VM;", &mut output).unwrap();
        assert!(output.is_empty());
        assert_eq!(
            framework.radio().state().channel_select,
            1,
            "VM should toggle to Memory (1)"
        );

        framework.process_frame("VM;", &mut output).unwrap();
        assert_eq!(
            framework.radio().state().channel_select,
            0,
            "VM should toggle back to VFO (0)"
        );
    }

    #[test]
    fn framework_vm_toggle_is_reflected_in_if_answer() {
        // Cross-check that VM's toggle is visible through IF's P7 field
        // too, not just Ft991aState directly (IF response body index 20 is
        // P7/select — full-response index 22, since the 2-char "IF" code
        // precedes the body — see ChannelStatusFields' column table).
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("VM;", &mut output).unwrap();
        output.clear();

        framework.process_frame("IF;", &mut output).unwrap();
        let response = String::from_utf8(output.clone()).unwrap();
        assert_eq!(&response[22..23], "1");
    }

    #[test]
    fn framework_ch_up_and_down_step_selected_memory_channel() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // Default selected_memory_channel is 1.
        framework.process_frame("CH0;", &mut output).unwrap(); // UP
        assert!(output.is_empty());
        framework.process_frame("MC;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MC002;");
        output.clear();

        framework.process_frame("CH1;", &mut output).unwrap(); // DOWN
        framework.process_frame("CH1;", &mut output).unwrap(); // DOWN
        framework.process_frame("MC;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MC117;");
    }

    #[test]
    fn framework_ch_wraps_at_boundaries() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("MC117;", &mut output).unwrap();
        output.clear();
        framework.process_frame("CH0;", &mut output).unwrap(); // UP wraps to 1
        framework.process_frame("MC;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MC001;");
        output.clear();

        framework.process_frame("CH1;", &mut output).unwrap(); // DOWN wraps to 117
        framework.process_frame("MC;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MC117;");
    }

    #[test]
    fn framework_ch_rejects_illegal_selector() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("CH2;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_qi_qr_round_trip_independent_of_numbered_memory_channels() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework
            .process_frame("FA018100000;", &mut output)
            .unwrap();
        framework.process_frame("MD05;", &mut output).unwrap(); // AM
        output.clear();

        framework.process_frame("QI;", &mut output).unwrap();
        assert!(output.is_empty());

        // Change VFO-A and the numbered memory channel state; QMB is
        // independent of both.
        framework
            .process_frame("FA007000000;", &mut output)
            .unwrap();
        framework.process_frame("MD02;", &mut output).unwrap(); // USB
        framework.process_frame("MC050;", &mut output).unwrap();
        output.clear();

        framework.process_frame("QR;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("FA;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "FA018100000;");
        output.clear();
        framework.process_frame("MD0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MD05;");
        output.clear();
        // The numbered-channel selection (MC050) is untouched by QI/QR.
        framework.process_frame("MC;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MC050;");
    }

    #[test]
    fn framework_qs_toggles_split() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        assert!(!framework.radio().state().split);

        let mut output = Vec::new();
        framework.process_frame("QS;", &mut output).unwrap();
        assert!(output.is_empty());
        assert!(framework.radio().state().split);

        framework.process_frame("QS;", &mut output).unwrap();
        assert!(output.is_empty());
        assert!(!framework.radio().state().split);
    }

    #[test]
    fn framework_action_triggers_reject_query_form() {
        // AB/BA/AM/VM/MA/QI/QR/QS/SV have no query form at all — a bare
        // zero-width frame with no matching Query form and a matching
        // Action form should dispatch as Action, not fail; but a
        // structurally invalid one-parameter frame for these should be
        // rejected (see framework_ab_ba_sv_reject_a_parameter above for the
        // AB/BA/SV case). This test covers the remaining six.
        for code in ["AM", "VM", "MA", "QI", "QR", "QS"] {
            let mut framework = CatFramework::new(Ft991aRadio::new());
            let mut output = Vec::new();
            framework
                .process_frame(&format!("{code}1;"), &mut output)
                .unwrap();
            assert_eq!(
                String::from_utf8(output).unwrap(),
                "?;",
                "{code} with a parameter should be rejected"
            );
        }
    }

    // -----------------------------------------------------------------
    // Batch 3: clarifier/RIT-XIT + tone + IF-shift (RT RC RD RU XT CN CT IS)
    // -----------------------------------------------------------------

    #[test]
    fn framework_rt_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("RT;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "RT0;");

        output.clear();
        framework.process_frame("RT1;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("RT;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "RT1;");
    }

    #[test]
    fn framework_xt_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("XT;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "XT0;");

        output.clear();
        framework.process_frame("XT1;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("XT;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "XT1;");
    }

    #[test]
    fn framework_rt_and_xt_are_independent_gates() {
        // Confirms the RX/TX clarifier relationship this batch resolved
        // from the manual: RT and XT are independent on/off flags, not a
        // single shared flag under two names.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("RT1;", &mut output).unwrap();
        output.clear();
        framework.process_frame("XT;", &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output.clone()).unwrap(),
            "XT0;",
            "XT must still be off after only RT was turned on"
        );

        output.clear();
        framework.process_frame("RT;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "RT1;");
    }

    #[test]
    fn framework_rt_xt_reject_illegal_values() {
        for code in ["RT", "XT"] {
            let mut framework = CatFramework::new(Ft991aRadio::new());
            let mut output = Vec::new();
            framework
                .process_frame(&format!("{code}2;"), &mut output)
                .unwrap();
            assert_eq!(String::from_utf8(output).unwrap(), "?;");
        }
    }

    #[test]
    fn framework_rd_sets_negative_clarifier_offset() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("RD1200;", &mut output).unwrap();
        assert!(output.is_empty());

        // No direct read command for the raw offset alone; confirmed via
        // IF's composite answer (P3: sign + 4-digit offset).
        output.clear();
        framework.process_frame("IF;", &mut output).unwrap();
        let answer = String::from_utf8(output).unwrap();
        assert!(
            answer.contains("-1200"),
            "expected -1200 offset in IF answer {answer}"
        );
    }

    #[test]
    fn framework_ru_sets_positive_clarifier_offset() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("RU0500;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("IF;", &mut output).unwrap();
        let answer = String::from_utf8(output).unwrap();
        assert!(
            answer.contains("+0500"),
            "expected +0500 offset in IF answer {answer}"
        );
    }

    #[test]
    fn framework_ru_then_rd_overwrites_not_accumulates() {
        // Confirms the "absolute set, not incremental step" reading.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("RU0500;", &mut output).unwrap();
        output.clear();
        framework.process_frame("RD0300;", &mut output).unwrap();
        output.clear();

        framework.process_frame("IF;", &mut output).unwrap();
        let answer = String::from_utf8(output).unwrap();
        assert!(
            answer.contains("-0300"),
            "RD should overwrite RU's prior value, got {answer}"
        );
    }

    #[test]
    fn framework_rc_clears_offset_but_not_rt_xt_gates() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("RU1200;", &mut output).unwrap();
        output.clear();
        framework.process_frame("RT1;", &mut output).unwrap();
        output.clear();
        framework.process_frame("XT1;", &mut output).unwrap();
        output.clear();

        framework.process_frame("RC;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("IF;", &mut output).unwrap();
        let answer = String::from_utf8(output.clone()).unwrap();
        assert!(
            answer.contains("+0000"),
            "RC should zero the offset, got {answer}"
        );

        output.clear();
        framework.process_frame("RT;", &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output.clone()).unwrap(),
            "RT1;",
            "RC must not touch RT's gate"
        );
        output.clear();
        framework.process_frame("XT;", &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "XT1;",
            "RC must not touch XT's gate"
        );
    }

    #[test]
    fn framework_rd_ru_reject_non_digit_content() {
        for code in ["RD", "RU"] {
            let mut framework = CatFramework::new(Ft991aRadio::new());
            let mut output = Vec::new();
            framework
                .process_frame(&format!("{code}12ab;"), &mut output)
                .unwrap();
            assert_eq!(String::from_utf8(output).unwrap(), "?;");
        }
    }

    #[test]
    fn framework_ct_selector_read_returns_default_off() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("CT0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "CT00;");
    }

    #[test]
    fn framework_ct_write_then_read_round_trips_all_legal_values() {
        for value in 0..=4u8 {
            let mut framework = CatFramework::new(Ft991aRadio::new());
            let mut output = Vec::new();
            framework
                .process_frame(&format!("CT0{value};"), &mut output)
                .unwrap();
            assert!(output.is_empty());

            output.clear();
            framework.process_frame("CT0;", &mut output).unwrap();
            assert_eq!(String::from_utf8(output).unwrap(), format!("CT0{value};"));
        }
    }

    #[test]
    fn framework_ct_rejects_out_of_range_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("CT05;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_ct_write_is_reflected_in_if_tone_status() {
        // Confirms CT reuses IF's already-landed P8 field, not a new one.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("CT03;", &mut output).unwrap();
        output.clear();

        framework.process_frame("IF;", &mut output).unwrap();
        let answer = String::from_utf8(output).unwrap();
        // ChannelStatusFields' P8 is the second-to-last-but-one digit
        // before the fixed "00" and offset_type; parse via the real struct
        // instead of hand-counting columns.
        let body = &answer[2..answer.len() - 1];
        let fields = ChannelStatusFields::parse(body).unwrap();
        assert_eq!(fields.tone_status, 3);
    }

    #[test]
    fn framework_cn_ctcss_selector_read_returns_default() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("CN00;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "CN00000;");
    }

    #[test]
    fn framework_cn_dcs_selector_read_returns_default() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("CN01;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "CN01000;");
    }

    #[test]
    fn framework_cn_ctcss_write_then_read_round_trips_boundaries() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // First table entry (index 000, 67.0 Hz).
        framework.process_frame("CN00000;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("CN00;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "CN00000;");

        // Last table entry (index 049, 254.1 Hz).
        output.clear();
        framework.process_frame("CN00049;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("CN00;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "CN00049;");
    }

    #[test]
    fn framework_cn_dcs_write_then_read_round_trips_boundaries() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // First table entry (index 000, code 023).
        framework.process_frame("CN01000;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("CN01;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "CN01000;");

        // Last table entry (index 103, code 754).
        output.clear();
        framework.process_frame("CN01103;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("CN01;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "CN01103;");
    }

    #[test]
    fn framework_cn_ctcss_and_dcs_indices_are_independent() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("CN00012;", &mut output).unwrap();
        output.clear();
        framework.process_frame("CN01050;", &mut output).unwrap();
        output.clear();

        framework.process_frame("CN00;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "CN00012;");
        output.clear();
        framework.process_frame("CN01;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "CN01050;");
    }

    #[test]
    fn framework_cn_rejects_ctcss_index_past_table_end() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        // 50 is one past the last legal CTCSS index (49).
        framework.process_frame("CN00050;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_cn_rejects_dcs_index_past_table_end() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        // 104 is one past the last legal DCS index (103).
        framework.process_frame("CN01104;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_cn_rejects_illegal_table_selector() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("CN02000;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_is_selector_read_returns_default_zero_shift() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("IS0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "IS0+0000;");
    }

    #[test]
    fn framework_is_write_then_read_round_trips_positive() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // The manual's own worked example (p.2): "IS0+1000;".
        framework.process_frame("IS0+1000;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("IS0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "IS0+1000;");
    }

    #[test]
    fn framework_is_write_then_read_round_trips_negative() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("IS0-1200;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("IS0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "IS0-1200;");
    }

    #[test]
    fn framework_is_rejects_magnitude_above_1200() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("IS0+1220;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_is_rejects_non_multiple_of_twenty() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("IS0+0010;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_is_rejects_missing_sign() {
        // Manual p.2's own error catalog: "IS01000;" is "Not enough
        // parameters specified (No direction (+) given for the IF shift)".
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("IS01000;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    // -----------------------------------------------------------------
    // CTCSS/DCS table integrity + boundary/invalid-index lookups
    // -----------------------------------------------------------------

    #[test]
    fn ctcss_tones_table_has_fifty_unique_entries() {
        assert_eq!(CTCSS_TONES_DECIHZ.len(), 50);
        let unique: HashSet<u16> = CTCSS_TONES_DECIHZ.iter().copied().collect();
        assert_eq!(unique.len(), 50, "CTCSS table must have no duplicate tones");
    }

    #[test]
    fn dcs_codes_table_has_104_unique_entries() {
        assert_eq!(DCS_CODES.len(), 104);
        let unique: HashSet<u16> = DCS_CODES.iter().copied().collect();
        assert_eq!(unique.len(), 104, "DCS table must have no duplicate codes");
    }

    #[test]
    fn ctcss_tone_hz_first_and_last_entries() {
        assert_eq!(ctcss_tone_hz(0), Some(67.0));
        assert_eq!(ctcss_tone_hz(49), Some(254.1));
    }

    #[test]
    fn ctcss_tone_hz_rejects_out_of_range_index() {
        assert_eq!(ctcss_tone_hz(50), None);
    }

    #[test]
    fn ctcss_tone_index_first_and_last_entries() {
        assert_eq!(ctcss_tone_index(67.0), Some(0));
        assert_eq!(ctcss_tone_index(254.1), Some(49));
    }

    #[test]
    fn ctcss_tone_index_rejects_unlisted_frequency() {
        // 100.5 Hz is not one of the 50 standard tones.
        assert_eq!(ctcss_tone_index(100.5), None);
    }

    #[test]
    fn dcs_code_number_first_and_last_entries() {
        assert_eq!(dcs_code_number(0), Some(23));
        assert_eq!(dcs_code_number(103), Some(754));
    }

    #[test]
    fn dcs_code_number_rejects_out_of_range_index() {
        assert_eq!(dcs_code_number(104), None);
    }

    #[test]
    fn dcs_code_index_first_and_last_entries() {
        assert_eq!(dcs_code_index(23), Some(0));
        assert_eq!(dcs_code_index(754), Some(103));
    }

    #[test]
    fn dcs_code_index_rejects_unlisted_code() {
        // 999 is not one of the 104 standard DCS codes.
        assert_eq!(dcs_code_index(999), None);
    }

    // -----------------------------------------------------------------
    // Batch 4: keyer/CW/break-in (KM KP KR KS KY CS ZI BI SD)
    // -----------------------------------------------------------------

    #[test]
    fn framework_km_read_default_channel_reports_empty_message() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("KM1;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "KM1;");
    }

    #[test]
    fn framework_km_write_then_read_round_trips_a_short_message() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework
            .process_frame("KM3CQ CQ DE N0CALL;", &mut output)
            .unwrap();
        assert!(output.is_empty(), "KM write has no answer");

        output.clear();
        framework.process_frame("KM3;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "KM3CQ CQ DE N0CALL;");
    }

    #[test]
    fn framework_km_write_then_read_round_trips_a_near_max_length_message() {
        // 49 characters — one short of KM's documented 50-character limit
        // (manual p.10), to prove the variable-width KM_SET_FORMS form
        // accepts content right up against the boundary, not just short
        // strings.
        let message = "CQ CQ CQ DE N0CALL N0CALL N0CALL PSE K 12345";
        assert_eq!(message.len(), 44);
        let long_message = format!("{message}12345"); // pad to 49 chars
        assert_eq!(long_message.len(), 49);

        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework
            .process_frame(&format!("KM5{long_message};"), &mut output)
            .unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("KM5;", &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            format!("KM5{long_message};")
        );
    }

    #[test]
    fn framework_km_write_accepts_exactly_fifty_characters() {
        let message = "A".repeat(50);
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework
            .process_frame(&format!("KM2{message};"), &mut output)
            .unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("KM2;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), format!("KM2{message};"));
    }

    #[test]
    fn framework_km_rejects_out_of_range_channel() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("KM0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        output.clear();
        framework.process_frame("KM6X;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_km_rejects_control_character_in_message() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework
            .process_frame("KM1BAD\u{1}MSG;", &mut output)
            .unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_km_channels_are_independent() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("KM1CQ DX;", &mut output).unwrap();
        output.clear();
        framework.process_frame("KM2TEST;", &mut output).unwrap();
        output.clear();

        framework.process_frame("KM1;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "KM1CQ DX;");
        output.clear();
        framework.process_frame("KM2;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "KM2TEST;");
        output.clear();
        framework.process_frame("KM3;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "KM3;");
    }

    #[test]
    fn framework_kp_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("KP;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "KP00;");

        output.clear();
        framework.process_frame("KP75;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("KP;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "KP75;");
    }

    #[test]
    fn framework_kp_rejects_out_of_range_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("KP76;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_kr_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("KR;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "KR0;");

        output.clear();
        framework.process_frame("KR1;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("KR;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "KR1;");
    }

    #[test]
    fn framework_kr_rejects_illegal_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("KR2;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_ks_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("KS;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "KS004;");

        output.clear();
        framework.process_frame("KS060;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("KS;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "KS060;");
    }

    #[test]
    fn framework_ks_rejects_out_of_range_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("KS003;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        output.clear();
        framework.process_frame("KS061;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_ky_valid_selectors_are_accepted_and_produce_no_answer() {
        for code in ["1", "2", "3", "4", "5", "6", "7", "8", "9", "A"] {
            let mut framework = CatFramework::new(Ft991aRadio::new());
            let mut output = Vec::new();
            let outcome = framework
                .process_frame(&format!("KY{code};"), &mut output)
                .unwrap();
            assert!(output.is_empty(), "KY{code} has no answer");
            assert_eq!(outcome.events.len(), 1, "KY{code} pushes exactly one event");
            assert_eq!(outcome.events[0].field, "keyer_playback");
        }
    }

    #[test]
    fn framework_ky_reports_correct_channel_and_mode_in_event() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        let outcome = framework.process_frame("KY3;", &mut output).unwrap();
        assert_eq!(outcome.events[0].value, "3:KeyerMemory");

        let mut output = Vec::new();
        let outcome = framework.process_frame("KY8;", &mut output).unwrap();
        assert_eq!(outcome.events[0].value, "3:MessageKeyer");

        let mut output = Vec::new();
        let outcome = framework.process_frame("KYA;", &mut output).unwrap();
        assert_eq!(outcome.events[0].value, "5:MessageKeyer");
    }

    #[test]
    fn framework_ky_rejects_illegal_selector_and_has_no_query_form() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("KYB;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        output.clear();
        framework.process_frame("KY;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_cs_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("CS;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "CS0;");

        output.clear();
        framework.process_frame("CS1;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("CS;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "CS1;");
    }

    #[test]
    fn framework_cs_rejects_illegal_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("CS2;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_zi_triggers_with_no_answer_and_no_query_form() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        let outcome = framework.process_frame("ZI;", &mut output).unwrap();
        assert!(output.is_empty());
        assert_eq!(outcome.events.len(), 1);
        assert_eq!(outcome.events[0].field, "zero_in");

        output.clear();
        framework.process_frame("ZI1;", &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "?;",
            "ZI with a parameter should be rejected"
        );
    }

    #[test]
    fn framework_bi_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("BI;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "BI0;");

        output.clear();
        framework.process_frame("BI1;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("BI;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "BI1;");
    }

    #[test]
    fn framework_bi_rejects_illegal_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("BI2;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_sd_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("SD;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "SD0030;");

        output.clear();
        framework.process_frame("SD3000;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("SD;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "SD3000;");
    }

    #[test]
    fn framework_sd_rejects_out_of_range_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("SD0029;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        output.clear();
        framework.process_frame("SD3001;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn ky_selector_from_wire_covers_all_ten_legal_characters() {
        assert_eq!(
            ky_selector_from_wire('1'),
            Some((1, KeyerPlaybackMode::KeyerMemory))
        );
        assert_eq!(
            ky_selector_from_wire('5'),
            Some((5, KeyerPlaybackMode::KeyerMemory))
        );
        assert_eq!(
            ky_selector_from_wire('6'),
            Some((1, KeyerPlaybackMode::MessageKeyer))
        );
        assert_eq!(
            ky_selector_from_wire('9'),
            Some((4, KeyerPlaybackMode::MessageKeyer))
        );
        assert_eq!(
            ky_selector_from_wire('A'),
            Some((5, KeyerPlaybackMode::MessageKeyer))
        );
        assert_eq!(ky_selector_from_wire('0'), None);
        assert_eq!(ky_selector_from_wire('B'), None);
    }

    #[test]
    fn ky_selector_to_wire_is_the_inverse_of_from_wire() {
        for c in ['1', '2', '3', '4', '5', '6', '7', '8', '9', 'A'] {
            let (channel, mode) = ky_selector_from_wire(c).unwrap();
            assert_eq!(ky_selector_to_wire(channel, mode), Some(c));
        }
        assert_eq!(ky_selector_to_wire(0, KeyerPlaybackMode::KeyerMemory), None);
        assert_eq!(
            ky_selector_to_wire(6, KeyerPlaybackMode::MessageKeyer),
            None
        );
    }

    // -----------------------------------------------------------------
    // Batch 5: scan/VOX/busy (SC VX VD VG BY).
    // -----------------------------------------------------------------

    #[test]
    fn framework_sc_query_and_set_round_trip_all_three_values() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("SC;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "SC0;");

        for value in ["1", "2", "0"] {
            output.clear();
            framework
                .process_frame(&format!("SC{value};"), &mut output)
                .unwrap();
            assert!(output.is_empty());

            output.clear();
            framework.process_frame("SC;", &mut output).unwrap();
            assert_eq!(
                String::from_utf8(output.clone()).unwrap(),
                format!("SC{value};")
            );
        }
    }

    #[test]
    fn framework_sc_rejects_illegal_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("SC3;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_vx_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("VX;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "VX0;");

        output.clear();
        framework.process_frame("VX1;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("VX;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "VX1;");
    }

    #[test]
    fn framework_vx_rejects_illegal_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("VX2;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_vg_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("VG;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "VG000;");

        output.clear();
        framework.process_frame("VG100;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("VG;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "VG100;");
    }

    #[test]
    fn framework_vg_rejects_out_of_range_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("VG101;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_vd_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("VD;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "VD0030;");

        output.clear();
        framework.process_frame("VD3000;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("VD;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "VD3000;");
    }

    #[test]
    fn framework_vd_rejects_out_of_range_and_non_step_values() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        // Below the 30 msec minimum.
        framework.process_frame("VD0020;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        // Above the 3000 msec maximum.
        output.clear();
        framework.process_frame("VD3010;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        // Not a multiple of the 10 msec step.
        output.clear();
        framework.process_frame("VD0035;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_by_query_reports_default_not_busy_and_has_no_set_form() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("BY;", &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output.clone()).unwrap(),
            "BY00;",
            "P1=rx_busy (default false=0), P2=fixed 0"
        );

        // BY is read-only per the manual (Set X) — any parameter should be
        // rejected, not silently accepted.
        output.clear();
        framework.process_frame("BY10;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_by_reflects_rx_busy_state_seeded_via_from_state() {
        let state = Ft991aState {
            rx_busy: true,
            ..Default::default()
        };
        let mut framework = CatFramework::new(Ft991aRadio::from_state(state));
        let mut output = Vec::new();
        framework.process_frame("BY;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "BY10;");
    }

    // -- Batch 6: attenuator/preamp/noise/AGC/notch/filter-width -------

    #[test]
    fn framework_ra_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("RA0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "RA00;");

        output.clear();
        framework.process_frame("RA01;", &mut output).unwrap();
        assert!(output.is_empty());

        output.clear();
        framework.process_frame("RA0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "RA01;");
    }

    #[test]
    fn framework_ra_rejects_illegal_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("RA02;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_pa_round_trip_all_three_values() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        for v in 0..=2u8 {
            output.clear();
            framework
                .process_frame(&format!("PA0{v};"), &mut output)
                .unwrap();
            assert!(output.is_empty());

            output.clear();
            framework.process_frame("PA0;", &mut output).unwrap();
            assert_eq!(
                String::from_utf8(output.clone()).unwrap(),
                format!("PA0{v};")
            );
        }
    }

    #[test]
    fn framework_pa_rejects_illegal_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("PA03;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_nb_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("NB01;", &mut output).unwrap();
        assert!(output.is_empty());
        framework.process_frame("NB0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "NB01;");
    }

    #[test]
    fn framework_nl_round_trip_and_boundaries() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("NL0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "NL0000;");

        output.clear();
        framework.process_frame("NL0010;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("NL0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "NL0010;");

        output.clear();
        framework.process_frame("NL0011;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_nr_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("NR01;", &mut output).unwrap();
        assert!(output.is_empty());
        framework.process_frame("NR0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "NR01;");
    }

    #[test]
    fn framework_rl_round_trip_and_boundaries() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        // Default is 1 (the minimum legal value, not 0 — `NR` is the
        // separate on/off gate).
        framework.process_frame("RL0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "RL001;");

        output.clear();
        framework.process_frame("RL015;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("RL0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "RL015;");

        output.clear();
        framework.process_frame("RL000;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        output.clear();
        framework.process_frame("RL016;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_gt_round_trip_p2_0_to_3_matches_p3() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        for v in 0..=3u8 {
            output.clear();
            framework
                .process_frame(&format!("GT0{v};"), &mut output)
                .unwrap();
            assert!(output.is_empty());
            output.clear();
            framework.process_frame("GT0;", &mut output).unwrap();
            assert_eq!(
                String::from_utf8(output.clone()).unwrap(),
                format!("GT0{v};")
            );
        }
    }

    #[test]
    fn framework_gt_auto_resolves_to_auto_fast() {
        // P2=4 ("AUTO") is a documented judgment call: it resolves to
        // P3=4 (AUTO-FAST), not AUTO-MID/AUTO-SLOW — see module docs' "GT"
        // section.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("GT04;", &mut output).unwrap();
        assert!(output.is_empty());
        framework.process_frame("GT0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "GT04;");
    }

    #[test]
    fn framework_gt_rejects_illegal_set_value_but_reports_seeded_wider_domain() {
        // P2=5/6 are not legal Set values (Set only accepts 0-4), even
        // though the wider Answer/P3 domain (0-6) legally includes them.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("GT05;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        // AUTO-MID/AUTO-SLOW (P3=5/6) are only reachable by seeding state
        // directly via `from_state`, never through any CAT `Set`.
        output.clear();
        let state = Ft991aState {
            agc_mode: 6,
            ..Default::default()
        };
        let mut framework = CatFramework::new(Ft991aRadio::from_state(state));
        framework.process_frame("GT0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "GT06;");
    }

    #[test]
    fn framework_co_contour_on_off_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("CO00;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "CO000000;");

        output.clear();
        framework.process_frame("CO000001;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("CO00;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "CO000001;");
    }

    #[test]
    fn framework_co_contour_freq_round_trip_and_boundaries() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        // Default is 10 Hz (the minimum legal value).
        framework.process_frame("CO01;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "CO010010;");

        output.clear();
        framework.process_frame("CO013200;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("CO01;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "CO013200;");

        output.clear();
        framework.process_frame("CO010009;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        output.clear();
        framework.process_frame("CO013201;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_co_apf_on_off_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("CO020001;", &mut output).unwrap();
        assert!(output.is_empty());
        framework.process_frame("CO02;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "CO020001;");
    }

    #[test]
    fn framework_co_apf_freq_round_trip_and_boundaries() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        // Default is 0 Hz (center, raw index 25).
        framework.process_frame("CO03;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "CO030025;");

        // raw 0 -> -250 Hz.
        output.clear();
        framework.process_frame("CO030000;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("CO03;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "CO030000;");

        // raw 50 -> +250 Hz.
        output.clear();
        framework.process_frame("CO030050;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("CO03;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "CO030050;");

        output.clear();
        framework.process_frame("CO030051;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_co_rejects_unknown_item_selector() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("CO04;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        output.clear();
        framework.process_frame("CO040000;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_bp_manual_notch_on_off_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("BP00;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "BP00000;");

        output.clear();
        framework.process_frame("BP00001;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("BP00;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "BP00001;");
    }

    #[test]
    fn framework_bp_manual_notch_freq_round_trip_and_boundaries() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        // Default is 10 Hz (raw 001, the minimum legal value).
        framework.process_frame("BP01;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "BP01001;");

        // raw 320 -> 3200 Hz.
        output.clear();
        framework.process_frame("BP01320;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("BP01;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "BP01320;");

        // raw 000 is illegal — the manual's own range is 001-320.
        output.clear();
        framework.process_frame("BP01000;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        output.clear();
        framework.process_frame("BP01321;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_bc_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("BC01;", &mut output).unwrap();
        assert!(output.is_empty());
        framework.process_frame("BC0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "BC01;");
    }

    #[test]
    fn framework_na_round_trip_uses_na_wire_code_not_ma() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // `NA` (this command) round-trips normally.
        framework.process_frame("NA01;", &mut output).unwrap();
        assert!(output.is_empty());
        framework.process_frame("NA0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "NA01;");

        // `MA` is a *different*, already-landed batch-1 command (a
        // zero-width Action trigger, "MEMORY CHANNEL TO VFO-A") —
        // confirming the manual's own wire-diagram typo (`M A P1 P2 ;` on
        // `NA`'s own per-command box) was NOT followed literally: a
        // 2-parameter `MA01;` frame does not silently collide with or
        // mutate `narrow_on`, it's simply not a legal `MA` frame shape.
        output.clear();
        framework.process_frame("MA01;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_sh_round_trip_and_boundaries() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        // Default is index 0 (the manual's own "00 (Default)" row).
        framework.process_frame("SH0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "SH000;");

        output.clear();
        framework.process_frame("SH021;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("SH0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "SH021;");

        output.clear();
        framework.process_frame("SH022;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    // -- SH_BANDWIDTH_TABLE / mode_family_for / filter_bandwidth_hz -----

    #[test]
    fn mode_family_for_maps_only_the_modes_named_in_shs_table_header() {
        assert_eq!(mode_family_for(0x1), Some(ModeFamily::Ssb)); // LSB
        assert_eq!(mode_family_for(0x2), Some(ModeFamily::Ssb)); // USB
        assert_eq!(mode_family_for(0x3), Some(ModeFamily::Cw)); // CW-U
        assert_eq!(mode_family_for(0x7), Some(ModeFamily::Cw)); // CW-L
        assert_eq!(mode_family_for(0x6), Some(ModeFamily::RttyPsk)); // RTTY-LSB
        assert_eq!(mode_family_for(0x9), Some(ModeFamily::RttyPsk)); // RTTY-USB

        // Deliberately unmapped: FM, AM, DATA-LSB, DATA-FM, FM-N, DATA-USB,
        // AM-N, C4FM — none of these are literally named by SH's own table
        // header, so this implementation does not guess a family for them.
        for mode in [0x4u8, 0x5, 0x8, 0xA, 0xB, 0xC, 0xD, 0xE] {
            assert_eq!(
                mode_family_for(mode),
                None,
                "mode {mode:#x} should be unmapped"
            );
        }
    }

    #[test]
    fn sh_bandwidth_table_has_exactly_22_rows() {
        assert_eq!(SH_BANDWIDTH_TABLE.len(), 22);
    }

    #[test]
    fn filter_bandwidth_hz_ssb_narrow_first_and_last_valid_p2() {
        // First valid P2 (00, the manual's own "Default" row).
        assert_eq!(filter_bandwidth_hz(ModeFamily::Ssb, true, 0), Some(1500));
        // Last valid P2 for SSB Narrow (09) — P2=10..=21 are all "-".
        assert_eq!(filter_bandwidth_hz(ModeFamily::Ssb, true, 9), Some(1800));
        assert_eq!(filter_bandwidth_hz(ModeFamily::Ssb, true, 10), None);
    }

    #[test]
    fn filter_bandwidth_hz_cw_narrow_first_and_last_valid_p2() {
        assert_eq!(filter_bandwidth_hz(ModeFamily::Cw, true, 0), Some(500));
        // Last valid P2 for CW Narrow (10) — P2=11..=21 are all "-".
        assert_eq!(filter_bandwidth_hz(ModeFamily::Cw, true, 10), Some(500));
        assert_eq!(filter_bandwidth_hz(ModeFamily::Cw, true, 11), None);
    }

    #[test]
    fn filter_bandwidth_hz_rtty_psk_wide_first_and_last_valid_p2() {
        assert_eq!(
            filter_bandwidth_hz(ModeFamily::RttyPsk, false, 0),
            Some(500)
        );
        // Last valid P2 for RTTY/PSK Wide (17) — P2=18..=21 are all "-".
        assert_eq!(
            filter_bandwidth_hz(ModeFamily::RttyPsk, false, 17),
            Some(3000)
        );
        assert_eq!(filter_bandwidth_hz(ModeFamily::RttyPsk, false, 18), None);
    }

    #[test]
    fn filter_bandwidth_hz_rejects_out_of_range_p2() {
        assert_eq!(filter_bandwidth_hz(ModeFamily::Ssb, false, 22), None);
    }

    #[test]
    fn apf_raw_hz_round_trips_at_boundaries_and_center() {
        assert_eq!(apf_raw_to_hz(0), -250);
        assert_eq!(apf_raw_to_hz(25), 0);
        assert_eq!(apf_raw_to_hz(50), 250);
        assert_eq!(apf_hz_to_raw(-250), 0);
        assert_eq!(apf_hz_to_raw(0), 25);
        assert_eq!(apf_hz_to_raw(250), 50);
    }

    // -- Batch 7: speech processor/mic/monitor tests -----------------------

    #[test]
    fn framework_mg_zero_width_query_and_plain_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("MG;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MG000;");

        output.clear();
        framework.process_frame("MG050;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("MG;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MG050;");
    }

    #[test]
    fn framework_mg_rejects_out_of_range_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("MG101;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_pl_zero_width_query_and_plain_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("PL;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "PL000;");

        output.clear();
        framework.process_frame("PL100;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("PL;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "PL100;");
    }

    #[test]
    fn framework_pl_rejects_out_of_range_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("PL101;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_pr_speech_processor_selector_read_returns_default() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("PR0;", &mut output).unwrap();
        // Default `speech_processor_on = false` → P2 encodes as "1" (OFF).
        assert_eq!(String::from_utf8(output).unwrap(), "PR01;");
    }

    #[test]
    fn framework_pr_parametric_mic_eq_selector_read_returns_default() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("PR1;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "PR11;");
    }

    #[test]
    fn framework_pr_speech_processor_write_then_read_round_trips() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // P2=2 means "ON" per the manual's non-zero-based encoding.
        framework.process_frame("PR02;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("PR0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "PR02;");

        // P2=1 means "OFF".
        output.clear();
        framework.process_frame("PR01;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("PR0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "PR01;");
    }

    #[test]
    fn framework_pr_parametric_mic_eq_write_then_read_round_trips() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("PR12;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("PR1;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "PR12;");
    }

    #[test]
    fn framework_pr_speech_processor_and_parametric_eq_are_independent() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("PR02;", &mut output).unwrap();
        output.clear();
        // Parametric mic EQ should still report its own (default) state,
        // unaffected by the speech-processor write above.
        framework.process_frame("PR1;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "PR11;");
        output.clear();
        framework.process_frame("PR0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "PR02;");
    }

    #[test]
    fn framework_pr_rejects_illegal_feature_selector() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("PR2;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_pr_rejects_illegal_on_off_encoding() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        // `0`/`1` is NOT the legal encoding for PR's P2 (it's `1`/`2`).
        framework.process_frame("PR00;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_ml_on_off_selector_read_returns_default() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("ML0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "ML0000;");
    }

    #[test]
    fn framework_ml_level_selector_read_returns_default() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("ML1;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "ML1000;");
    }

    #[test]
    fn framework_ml_on_off_write_then_read_round_trips() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("ML0001;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("ML0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "ML0001;");

        output.clear();
        framework.process_frame("ML0000;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("ML0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "ML0000;");
    }

    #[test]
    fn framework_ml_level_write_then_read_round_trips_boundaries() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("ML1000;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("ML1;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "ML1000;");

        output.clear();
        framework.process_frame("ML1100;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("ML1;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "ML1100;");
    }

    #[test]
    fn framework_ml_on_off_and_level_are_independent() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("ML0001;", &mut output).unwrap();
        output.clear();
        framework.process_frame("ML1050;", &mut output).unwrap();
        output.clear();

        framework.process_frame("ML0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "ML0001;");
        output.clear();
        framework.process_frame("ML1;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "ML1050;");
    }

    #[test]
    fn framework_ml_rejects_illegal_on_off_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        // `002` is neither `000` (OFF) nor `001` (ON).
        framework.process_frame("ML0002;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_ml_rejects_out_of_range_level() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("ML1101;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_ml_rejects_illegal_selector() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("ML2;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    // -----------------------------------------------------------------
    // Batch 8: band/step/encoder front-panel controls (BS BU BD FS ED EU
    // EK DN UP).
    // -----------------------------------------------------------------

    #[test]
    fn framework_bs_sets_selected_band_first_and_last_valid_codes() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // BS has no Read/Answer form at all — inspect state directly.
        framework.process_frame("BS00;", &mut output).unwrap();
        assert!(output.is_empty());
        assert_eq!(framework.radio().state().selected_band, 0);

        output.clear();
        framework.process_frame("BS16;", &mut output).unwrap();
        assert!(output.is_empty());
        assert_eq!(framework.radio().state().selected_band, 16);
    }

    #[test]
    fn framework_bs_rejects_the_documented_gap_at_index_13() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("BS13;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
        // Rejected write must not have mutated state.
        assert_eq!(framework.radio().state().selected_band, 0);
    }

    #[test]
    fn framework_bs_rejects_out_of_range_band() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("BS17;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_bu_steps_band_up_wrapping_and_skipping_the_gap() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // Step from 12 (MW) up should land on 14 (AIR), skipping 13.
        framework.process_frame("BS12;", &mut output).unwrap();
        output.clear();
        framework.process_frame("BU0;", &mut output).unwrap();
        assert!(output.is_empty());
        assert_eq!(framework.radio().state().selected_band, 14);

        // Step from the highest band (16) wraps to the lowest (0).
        framework.process_frame("BS16;", &mut output).unwrap();
        output.clear();
        framework.process_frame("BU0;", &mut output).unwrap();
        assert_eq!(framework.radio().state().selected_band, 0);
    }

    #[test]
    fn framework_bd_steps_band_down_wrapping_and_skipping_the_gap() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // Step from 14 (AIR) down should land on 12 (MW), skipping 13.
        framework.process_frame("BS14;", &mut output).unwrap();
        output.clear();
        framework.process_frame("BD0;", &mut output).unwrap();
        assert!(output.is_empty());
        assert_eq!(framework.radio().state().selected_band, 12);

        // Step from the lowest band (0) wraps to the highest (16).
        framework.process_frame("BS00;", &mut output).unwrap();
        output.clear();
        framework.process_frame("BD0;", &mut output).unwrap();
        assert_eq!(framework.radio().state().selected_band, 16);
    }

    #[test]
    fn framework_bu_bd_reject_non_fixed_selector() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("BU1;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");
        output.clear();
        framework.process_frame("BD1;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_fs_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework.process_frame("FS;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "FS0;");

        output.clear();
        framework.process_frame("FS1;", &mut output).unwrap();
        assert!(output.is_empty());

        framework.process_frame("FS;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "FS1;");
    }

    #[test]
    fn framework_fs_rejects_illegal_value() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("FS2;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_ed_validates_and_acknowledges_without_state_mutation() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        let before = framework.radio().state().vfo_a_hz;

        // P1=0 (MAIN), P2=01 steps.
        let outcome = framework.process_frame("ED001;", &mut output).unwrap();
        assert!(output.is_empty());
        assert_eq!(outcome.events.len(), 1);
        assert_eq!(outcome.events[0].field, "encoder_down");
        assert_eq!(framework.radio().state().vfo_a_hz, before);
    }

    #[test]
    fn framework_eu_validates_and_acknowledges_without_state_mutation() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        let before = framework.radio().state().vfo_a_hz;

        // P1=8 (MULTI), P2=99 steps (upper boundary).
        let outcome = framework.process_frame("EU899;", &mut output).unwrap();
        assert!(output.is_empty());
        assert_eq!(outcome.events.len(), 1);
        assert_eq!(outcome.events[0].field, "encoder_up");
        assert_eq!(framework.radio().state().vfo_a_hz, before);
    }

    #[test]
    fn framework_ed_eu_reject_illegal_p1_and_out_of_range_p2() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        // P1=2 is not a legal encoder selector.
        framework.process_frame("ED201;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");

        // P2=00 is below the 1-99 legal range.
        output.clear();
        framework.process_frame("ED000;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "?;");
    }

    #[test]
    fn framework_ek_triggers_with_no_answer_and_no_query_form() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        let outcome = framework.process_frame("EK;", &mut output).unwrap();
        assert!(output.is_empty());
        assert_eq!(outcome.events.len(), 1);
        assert_eq!(outcome.events[0].field, "ent_key");

        output.clear();
        framework.process_frame("EK1;", &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "?;",
            "EK with a parameter should be rejected"
        );
    }

    #[test]
    fn framework_dn_steps_vfo_a_down_by_mic_step_hz() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        let before = framework.radio().state().vfo_a_hz;

        framework.process_frame("DN;", &mut output).unwrap();
        assert!(output.is_empty());
        assert_eq!(framework.radio().state().vfo_a_hz, before - MIC_STEP_HZ);
    }

    #[test]
    fn framework_up_steps_vfo_a_up_by_mic_step_hz() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        let before = framework.radio().state().vfo_a_hz;

        framework.process_frame("UP;", &mut output).unwrap();
        assert!(output.is_empty());
        assert_eq!(framework.radio().state().vfo_a_hz, before + MIC_STEP_HZ);
    }

    #[test]
    fn framework_dn_up_saturate_at_fa_range_boundaries() {
        let mut framework = CatFramework::new(Ft991aRadio::from_state(Ft991aState {
            vfo_a_hz: 30_000,
            ..Default::default()
        }));
        let mut output = Vec::new();
        framework.process_frame("DN;", &mut output).unwrap();
        assert_eq!(framework.radio().state().vfo_a_hz, 30_000);

        let mut framework = CatFramework::new(Ft991aRadio::from_state(Ft991aState {
            vfo_a_hz: 470_000_000,
            ..Default::default()
        }));
        let mut output = Vec::new();
        framework.process_frame("UP;", &mut output).unwrap();
        assert_eq!(framework.radio().state().vfo_a_hz, 470_000_000);
    }

    #[test]
    fn framework_dn_up_reject_a_parameter() {
        for frame in ["DN1;", "UP1;"] {
            let mut framework = CatFramework::new(Ft991aRadio::new());
            let mut output = Vec::new();
            framework.process_frame(frame, &mut output).unwrap();
            assert_eq!(
                String::from_utf8(output).unwrap(),
                "?;",
                "{frame} should be rejected"
            );
        }
    }

    // -- BAND_CODES / next_band / prev_band / EncoderSelector -----------

    #[test]
    fn band_codes_table_has_sixteen_unique_entries_excluding_the_gap() {
        assert_eq!(BAND_CODES.len(), 16);
        assert!(!BAND_CODES.contains(&13));
        let mut sorted = BAND_CODES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 16, "BAND_CODES must contain no duplicates");
    }

    #[test]
    fn next_band_wraps_past_the_highest_band_to_the_lowest() {
        assert_eq!(next_band(16), 0);
    }

    #[test]
    fn next_band_skips_the_gap_at_13() {
        assert_eq!(next_band(12), 14);
    }

    #[test]
    fn prev_band_wraps_past_the_lowest_band_to_the_highest() {
        assert_eq!(prev_band(0), 16);
    }

    #[test]
    fn prev_band_skips_the_gap_at_13() {
        assert_eq!(prev_band(14), 12);
    }

    #[test]
    fn encoder_selector_wire_digit_round_trips() {
        for selector in [
            EncoderSelector::Main,
            EncoderSelector::Sub,
            EncoderSelector::Multi,
        ] {
            assert_eq!(
                EncoderSelector::from_wire_digit(selector.as_wire_digit()),
                Some(selector)
            );
        }
        assert_eq!(EncoderSelector::from_wire_digit('2'), None);
    }

    // -- Batch 10 (last of the 10 core batches): misc system/TX/tuner/DVS --

    // -- AC --------------------------------------------------------------

    #[test]
    fn framework_ac_zero_width_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("AC;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "AC000;");
        output.clear();

        framework.process_frame("AC001;", &mut output).unwrap();
        assert!(output.is_empty(), "Set should produce no response");
        output.clear();

        framework.process_frame("AC;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "AC001;");
        output.clear();

        framework.process_frame("AC002;", &mut output).unwrap();
        output.clear();
        framework.process_frame("AC;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "AC002;");
    }

    #[test]
    fn framework_ac_rejects_non_fixed_p1_p2_and_out_of_range_p3() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        for frame in ["AC103;", "AC013;", "AC003;"] {
            output.clear();
            framework.process_frame(frame, &mut output).unwrap();
            assert_eq!(
                String::from_utf8(output.clone()).unwrap(),
                "?;",
                "{frame} should be rejected"
            );
        }
    }

    // -- AI --------------------------------------------------------------

    #[test]
    fn framework_ai_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("AI;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "AI0;");
        output.clear();

        framework.process_frame("AI1;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();

        framework.process_frame("AI;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "AI1;");
    }

    // -- DA ----------------------------------------------------------------

    #[test]
    fn framework_da_zero_width_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("DA;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "DA000100;");
        output.clear();

        framework.process_frame("DA000215;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();

        framework.process_frame("DA;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "DA000215;");
    }

    #[test]
    fn framework_da_rejects_out_of_range_led_tft_and_non_fixed_p1() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        for frame in ["DA000315;", "DA000216;", "DA010115;"] {
            output.clear();
            framework.process_frame(frame, &mut output).unwrap();
            assert_eq!(
                String::from_utf8(output.clone()).unwrap(),
                "?;",
                "{frame} should be rejected"
            );
        }
    }

    // -- DT: the three P1-selected shapes (date/time/offset) --------------

    #[test]
    fn framework_dt_date_shape_default_and_write_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("DT0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "DT000000101;");
        output.clear();

        framework
            .process_frame("DT020260715;", &mut output)
            .unwrap();
        assert!(output.is_empty());
        output.clear();

        framework.process_frame("DT0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "DT020260715;");
    }

    #[test]
    fn framework_dt_date_shape_rejects_illegal_month_and_day() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        for frame in ["DT020261315;", "DT020260732;", "DT020260700;"] {
            output.clear();
            framework.process_frame(frame, &mut output).unwrap();
            assert_eq!(
                String::from_utf8(output.clone()).unwrap(),
                "?;",
                "{frame} should be rejected"
            );
        }
    }

    #[test]
    fn framework_dt_time_shape_default_and_write_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("DT1;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "DT1000000;");
        output.clear();

        framework.process_frame("DT1153045;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();

        framework.process_frame("DT1;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "DT1153045;");
    }

    #[test]
    fn framework_dt_time_shape_rejects_illegal_hour_minute_second() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        for frame in ["DT1240000;", "DT1006000;", "DT1000060;"] {
            output.clear();
            framework.process_frame(frame, &mut output).unwrap();
            assert_eq!(
                String::from_utf8(output.clone()).unwrap(),
                "?;",
                "{frame} should be rejected"
            );
        }
    }

    #[test]
    fn framework_dt_offset_shape_default_and_write_round_trip_both_signs() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("DT2;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "DT2+0000;");
        output.clear();

        framework.process_frame("DT2+1400;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("DT2;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "DT2+1400;");
        output.clear();

        framework.process_frame("DT2-1200;", &mut output).unwrap();
        output.clear();
        framework.process_frame("DT2;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "DT2-1200;");
    }

    #[test]
    fn framework_dt_offset_shape_rejects_out_of_range_and_non_30_minute_steps() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        // +14:30 (870 min, > 840 max), +00:15 (not a 30-minute step), and a
        // -13:00 magnitude that alone would be in range but whose sign
        // pushes the total below -720.
        for frame in ["DT2+1430;", "DT2+0015;", "DT2-1300;"] {
            output.clear();
            framework.process_frame(frame, &mut output).unwrap();
            assert_eq!(
                String::from_utf8(output.clone()).unwrap(),
                "?;",
                "{frame} should be rejected"
            );
        }
    }

    #[test]
    fn framework_dt_selector_read_rejects_unknown_p1() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("DT3;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");
    }

    #[test]
    fn framework_dt_structurally_legal_width_with_wrong_p1_is_rejected() {
        // Width 6 is legal (the offset shape's total width), but this
        // frame's P1 is "0" (date), not "2" (offset) — structural match
        // succeeded, semantic validation still per-item, same pattern
        // `EX`/`FA` established.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("DT020261;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");
    }

    // -- LK ------------------------------------------------------------

    #[test]
    fn framework_lk_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("LK;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "LK0;");
        output.clear();

        framework.process_frame("LK1;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();

        framework.process_frame("LK;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "LK1;");
    }

    // -- OI, confirming reuse of ChannelStatusFields -----------------------

    #[test]
    fn framework_oi_query_default_state_reports_vfo_b_frequency() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("OI;", &mut output).unwrap();
        // Default state: channel 0, VFO-B 14,100,000 Hz, no clarifier, mode
        // USB (0x2), select 0 (VFO), tone 0 (off), offset 0 (simplex) —
        // same shared fields IF reports (default `IF;` is
        // "IF000014000000+000000200000;"), only the frequency differs.
        assert_eq!(
            String::from_utf8(output.clone()).unwrap(),
            "OI000014100000+000000200000;"
        );
    }

    #[test]
    fn framework_oi_reuses_channel_status_fields_shared_with_if_except_frequency() {
        // Confirms OI's non-frequency fields are exactly the same shared
        // state IF reports: FA/FB set independent VFO-A/VFO-B frequencies,
        // MD sets the (single, shared) mode — IF and OI's answers should be
        // byte-identical from the frequency field onward.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();

        framework
            .process_frame("FA014250000;", &mut output)
            .unwrap();
        output.clear();
        framework
            .process_frame("FB014777000;", &mut output)
            .unwrap();
        output.clear();
        framework.process_frame("MD0E;", &mut output).unwrap(); // C4FM
        output.clear();

        framework.process_frame("IF;", &mut output).unwrap();
        let if_answer = String::from_utf8(output.clone()).unwrap();
        output.clear();
        framework.process_frame("OI;", &mut output).unwrap();
        let oi_answer = String::from_utf8(output.clone()).unwrap();

        assert_eq!(if_answer, "IF000014250000+000000E00000;");
        assert_eq!(oi_answer, "OI000014777000+000000E00000;");
        // Identical from wire column 14 onward (2-char code + 3-digit
        // channel + 9-digit frequency = the first 14 characters, which are
        // the only ones that can legitimately differ).
        assert_eq!(
            &if_answer[14..],
            &oi_answer[14..],
            "OI must reuse IF's shared clarifier/mode/select/tone/offset-type state verbatim"
        );
    }

    #[test]
    fn framework_oi_reflects_non_default_channel_status_state_seeded_via_from_state() {
        let state = Ft991aState {
            if_channel: 5,
            vfo_b_hz: 7_100_000,
            clarifier_offset_hz: -250,
            rx_clarifier_on: true,
            tx_clarifier_on: true,
            channel_select: 1, // Memory
            tone_status: 3,    // DCS ENC/DEC
            offset_type: 2,    // Minus Shift
            ..Ft991aState::default()
        };
        let mut framework = CatFramework::new(Ft991aRadio::from_state(state));
        let mut output = Vec::new();
        framework.process_frame("OI;", &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output.clone()).unwrap(),
            "OI005007100000-025011213002;"
        );
    }

    #[test]
    fn framework_oi_has_no_set_form() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("OI1;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "?;");
    }

    // -- OS ------------------------------------------------------------

    #[test]
    fn framework_os_selector_read_and_write_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("OS0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "OS00;");
        output.clear();

        framework.process_frame("OS01;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();

        framework.process_frame("OS0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "OS01;");
    }

    #[test]
    fn framework_os_rejects_out_of_range_value_and_wrong_selector() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        for frame in ["OS03;", "OS13;", "OS1;"] {
            output.clear();
            framework.process_frame(frame, &mut output).unwrap();
            assert_eq!(
                String::from_utf8(output.clone()).unwrap(),
                "?;",
                "{frame} should be rejected"
            );
        }
    }

    #[test]
    fn framework_os_writes_the_same_offset_type_field_if_reports() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("OS02;", &mut output).unwrap(); // Minus Shift
        output.clear();
        framework.process_frame("IF;", &mut output).unwrap();
        let if_answer = String::from_utf8(output.clone()).unwrap();
        assert!(
            if_answer.ends_with("2;"),
            "IF's P10 should reflect OS's write: {if_answer}"
        );
    }

    // -- FT: the write/report domain mismatch ------------------------------

    #[test]
    fn framework_ft_query_default_and_domain_translated_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("FT;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "FT0;");
        output.clear();

        framework.process_frame("FT3;", &mut output).unwrap(); // Set VFO-B TX
        assert!(output.is_empty());
        output.clear();

        framework.process_frame("FT;", &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output.clone()).unwrap(),
            "FT1;",
            "Answer domain is 0/1, not Set's 2/3"
        );
        output.clear();

        framework.process_frame("FT2;", &mut output).unwrap(); // Set VFO-A TX
        output.clear();
        framework.process_frame("FT;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "FT0;");
    }

    #[test]
    fn framework_ft_rejects_answer_domain_values_and_out_of_range_on_set() {
        // FT's Set domain is {2,3}; {0,1} are Answer-only.
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        for frame in ["FT0;", "FT1;", "FT4;"] {
            output.clear();
            framework.process_frame(frame, &mut output).unwrap();
            assert_eq!(
                String::from_utf8(output.clone()).unwrap(),
                "?;",
                "{frame} should be rejected"
            );
        }
    }

    // -- TS ("TXW") ------------------------------------------------------

    #[test]
    fn framework_ts_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("TS;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "TS0;");
        output.clear();

        framework.process_frame("TS1;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();

        framework.process_frame("TS;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "TS1;");
    }

    // -- MX ------------------------------------------------------------

    #[test]
    fn framework_mx_query_and_set_round_trip() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("MX;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MX0;");
        output.clear();

        framework.process_frame("MX1;", &mut output).unwrap();
        assert!(output.is_empty());
        output.clear();

        framework.process_frame("MX;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "MX1;");
    }

    // -- LM: selector read + per-channel Start/Stop toggle -----------------

    #[test]
    fn framework_lm_selector_read_default_and_toggle_start_stop() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("LM0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "LM00;");
        output.clear();

        framework.process_frame("LM03;", &mut output).unwrap(); // start CH3
        assert!(output.is_empty());
        output.clear();
        framework.process_frame("LM0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "LM03;");
        output.clear();

        framework.process_frame("LM03;", &mut output).unwrap(); // toggle: stop
        output.clear();
        framework.process_frame("LM0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "LM00;");
    }

    #[test]
    fn framework_lm_switching_channel_while_recording_starts_the_new_one() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("LM01;", &mut output).unwrap();
        output.clear();
        framework.process_frame("LM04;", &mut output).unwrap(); // different channel
        output.clear();
        framework.process_frame("LM0;", &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output.clone()).unwrap(),
            "LM04;",
            "switching channels starts the new one, does not stop"
        );
    }

    #[test]
    fn framework_lm_explicit_stop_always_stops() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("LM02;", &mut output).unwrap();
        output.clear();
        framework.process_frame("LM00;", &mut output).unwrap();
        output.clear();
        framework.process_frame("LM0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "LM00;");
    }

    #[test]
    fn framework_lm_rejects_out_of_range_channel_and_wrong_selector() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        for frame in ["LM06;", "LM13;", "LM1;"] {
            output.clear();
            framework.process_frame(frame, &mut output).unwrap();
            assert_eq!(
                String::from_utf8(output.clone()).unwrap(),
                "?;",
                "{frame} should be rejected"
            );
        }
    }

    // -- PB: selector read + unconditional Start/Stop (not a toggle) -------

    #[test]
    fn framework_pb_selector_read_default_and_unconditional_start_stop() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        framework.process_frame("PB0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "PB00;");
        output.clear();

        framework.process_frame("PB03;", &mut output).unwrap();
        output.clear();
        framework.process_frame("PB0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "PB03;");
        output.clear();

        // Sending the SAME channel again does NOT toggle-stop it (unlike
        // `LM`) — playback of channel 3 simply (re)starts, unconditionally.
        framework.process_frame("PB03;", &mut output).unwrap();
        output.clear();
        framework.process_frame("PB0;", &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output.clone()).unwrap(),
            "PB03;",
            "PB is not a toggle like LM"
        );
        output.clear();

        framework.process_frame("PB00;", &mut output).unwrap();
        output.clear();
        framework.process_frame("PB0;", &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap(), "PB00;");
    }

    #[test]
    fn framework_pb_rejects_out_of_range_channel_and_wrong_selector() {
        let mut framework = CatFramework::new(Ft991aRadio::new());
        let mut output = Vec::new();
        for frame in ["PB06;", "PB13;", "PB1;"] {
            output.clear();
            framework.process_frame(frame, &mut output).unwrap();
            assert_eq!(
                String::from_utf8(output.clone()).unwrap(),
                "?;",
                "{frame} should be rejected"
            );
        }
    }
}
