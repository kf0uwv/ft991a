# Yaesu Agent Findings

## Wave 3 — CAT batch 9 (Meters/status: `IF RM RI RS MS UL`)

- **`IF`'s composite Answer layout, fully resolved** (manual printed p.10,
  PDF page 11). 25-byte body after the `IF` code: P1 channel(3, cols3-5) +
  P2 VFO-A freq(9, cols6-14) + P3 clarifier sign+offset(5, cols15-19) + P4
  RX-CLAR(1, col20) + P5 TX-CLAR(1, col21) + P6 mode(1, col22) + P7
  VFO/Mem/QMB select(1, col23) + P8 CTCSS/DCS status(1, col24) + P9 fixed
  `"00"`(2, cols25-26) + P10 offset-type(1, col27); terminator at col28.
  Reading the manual's own column-numbered sub-rows directly (rather than
  inferring boundaries from prose) resolved the exact ambiguity Wave 1
  deferred over. Factored into `ChannelStatusFields` in
  `ft991a_radio.rs` for reuse by `MR`/`MT` (batch 2) and `OI` (batch 10),
  per the architect's cross-batch finding.

- **`RM`/`MS` relationship, narrower than the architect's summary
  implied**: only `RM`'s P1=`0` and P1=`2` ("Depends on the front panel
  METER") resolve through `MS`'s current selection. `RM`'s other 7
  selectors (`1`=S-meter, `3`-`8`=direct COMP/ALC/PO/SWR/ID/VDD) are
  self-contained and ignore `MS` entirely. Confirmed against manual p.15's
  `RM` P1 legend directly.

- **`RI` selector gap, transcribed exactly**: manual p.15's P1 legend
  lists only `0`,`3`,`4`,`5`,`6`,`7`,`A` — `1`,`2`,`8`,`9`,`B`-`F` are not
  documented. Implemented as a hard rejection (not a silent accept), and a
  dedicated `RadioIndicator` enum in `radio_trait.rs` that only
  constructs the 7 legal values.

- **Two small citation corrections** against
  `planning/architect/task_plan.md` §10.5's summary (flagged, not silently
  fixed): `RS`'s detail box is entirely on printed p.16 (not "p.15-16"),
  and `MS`'s detail box is entirely on printed p.12 (not "p.12-13"). Both
  are single-page detail boxes; the wider citation ranges likely conflated
  the p.3 master-table row with the detail-box page.

- **Minor manual self-inconsistency, not blocking**: `IF`'s own P6 (MODE)
  legend uses "CW"/"CW-R" for values 3/7, while `MD`'s P2 legend (same
  page, p.10/p.11) uses "CW-U"/"CW-L" for the identical numeric values.
  Numeric encoding is identical either way; `ChannelStatusFields.mode`
  reuses the existing raw-nibble representation without needing a
  decision. Documented, not resolved in favor of either label.

- **Judgment call, explicitly flagged (not the core Wave-1 ambiguity)**:
  `IF`'s P1 (memory channel) is documented as `001`-`117`; the manual
  doesn't say what appears there in VFO mode. This implementation accepts
  `000`-`117` and treats `0` as an implementation-chosen VFO-mode
  sentinel.

- No blocking ambiguity was found this task — the one Wave 1 flagged
  (`IF`'s field boundaries) resolved cleanly once read from the manual's
  own column-numbered sub-rows rather than inferred from surrounding
  prose/extracted text.

## Wave 3 — `EX` menu, first sub-batch (plumbing + 9 PTT/keying items)

- **`EX`'s wire shape, confirmed exactly as the architect's §10.6
  described**: manual printed p.7 (PDF p.8), `EX` box. Set:
  `EX<3-digit P1><variable-width P2>;`. Read: `EX<P1>;` (structurally a
  3-char `Set` to the parser — another "selector read," same treatment as
  `MD`/`SM`/`RM`/`RI`). Answer: same shape as Set. One composite command,
  not 153 codes.

- **All 153 rows' "Digits" column re-transcribed (not just the 9 target
  items), to verify the architect's "~6 distinct total-length
  `CommandForm` entries" estimate rather than assume it.** Cross-checked
  every row via `pdftotext -layout -f 8 -l 10` against the rendered page
  images. Result: **exactly 6 distinct P2 digit widths across the whole
  table — 1, 2, 3, 4, 5, 8** — confirming the architect's estimate
  precisely. Item 087 "RADIO ID" is excluded from this count (P2 shown as
  literal dashes `----------`, no digit count given anywhere in the 20
  pages — confirms the architect's own flag; correctly out of scope for
  this task and absent from `EX_MENU_TABLE`). This yields total wire
  widths `3 + {1,2,3,4,5,8} = {4,5,6,7,8,11}` — `EX_SET_FORMS` in
  `ft991a_radio.rs` has 7 entries (those 6 write widths plus the width-3
  selector-read form), built now even though this sub-batch's
  `EX_MENU_TABLE` only populates width-1 (total-width-4) rows, per §10.6's
  own "later sub-batches just add table rows" design intent.

- **One digit-count discrepancy caught and resolved by cross-checking two
  independent extraction methods, not silently guessed**: the rendered
  page image's text extraction showed item 072 "DATA PORT SELECT" with
  Digits=`3`. This is inconsistent with every sibling `*PORT SELECT` item
  (048, 077, 109 — all Digits=`1`) and with the field itself being a
  2-value enumeration (`1: DATA`, `2: USB`) that cannot need 3 wire
  digits. Re-extracting the same page with `pdftotext -layout` returned
  Digits=`1` for item 072, matching the image-render's own immediately
  *following* row (073 "DATA OUT LEVEL," genuinely Digits=`3`) — strong
  evidence the image-render pass mis-attributed 073's digit count to 072.
  Resolved via cross-tool + sibling-row-pattern agreement (two independent
  signals converging), not treated as an unresolvable ambiguity requiring
  escalation.

- **Genuine manual inconsistency, transcribed exactly, not silently
  normalized**: items 048 ("AM PORT SELECT") and 109 ("SSB PORT SELECT")
  encode the DATA/USB choice as `0: DATA 1: USB`, while items 072 ("DATA
  PORT SELECT") and 077 ("FM PKT PORT SELECT") encode the *identical*
  DATA/USB concept as `1: DATA 2: USB` — no `0` value at all for those
  two. Confirmed present verbatim on the manual page via both extraction
  methods (not a transcription typo introduced here). `ExMenuItem`'s
  `legal_values` field stores each item's actual legend rather than
  assuming a uniform zero-based convention, and `Ft991aState`'s defaults
  for `ex_data_port_select`/`ex_fm_pkt_port_select` are `1` (not `0`,
  which isn't a legal value for those two items) — documented as an
  arbitrary implementation default (the manual states no factory default
  for any `EX` item), same category of open item as `meter_select`'s
  default in the batch-9 task.

- **Item 060 "PC KEYING," the item this wave's RTS/DTR CW-keying feature
  actually reads/writes, confirmed exactly**: `0: OFF 1: DAKY 2: RTS
  3: DTR`, 1 digit — matches the architect's §10.2 correction (and its own
  §10.6 citation) precisely; no discrepancy found.

- **Deliberate scope narrowing against Wave 3 batch 9's precedent, flagged
  for architect review rather than silently decided**: this task's own
  numbered instructions list only `radio/src/ft991a_radio.rs` work
  (command table, state, dispatch, tests) — unlike batch 9's task, which
  explicitly named `radio/src/ft991a.rs` (controller client methods) and
  `radio/src/radio_trait.rs` growth as deliverables too. No `Ft991a<S>`
  client method (e.g. a hypothetical `get_ex_menu_item`/`set_ex_menu_item`)
  was added, and `radio_trait.rs` was not touched. Followed the explicit,
  narrower task scope rather than assuming batch 9's broader pattern
  applies here. This is independently consistent with `yaesu.md`'s own
  guidance that FT-991A-specific "menu access" belongs as inherent
  `Ft991a` methods, never `Radio`-trait methods — and with
  `planning/architect/task_plan.md` §10.2's own note that CAT support for
  menu 060 isn't a hard dependency for the RTS/DTR feature (the front
  panel can set it by hand) — so no controller-client-shaped functionality
  is actually blocked by this omission. Flagged here, not assumed correct
  without saying so, since it's a real narrowing versus the most recent
  precedent in this same file.

- No manual ambiguity blocked this task. The one open item from §10.6
  (item 087 "RADIO ID"'s unresolvable P2 width) was independently
  reconfirmed rather than encountered fresh — it's correctly outside this
  sub-batch's 9-item scope regardless.

## Wave 3 — CAT batch 2 (Memory channel records: `MC MR MW MT`)

- **`MR`/`MW` share `IF`'s `ChannelStatusFields` shape with zero field-
  boundary differences** — confirmed column-by-column against the manual
  page image (printed p.12, PDF p.13), not assumed from the architect's
  cross-batch summary. The two differences found are semantic, not
  structural: `MR`/`MW`'s channel field is documented `001`-`117` only
  (no `000` VFO sentinel `IF` has), and `MR`'s own P7 legend is narrower
  than `IF`'s (`0`/`1` only, vs. `IF`'s full 0-6). Neither required
  changing `ChannelStatusFields` itself — both are handled as extra
  semantic checks in `handle_command`'s `Mr`/`Mw` arms, on top of
  `ChannelStatusFields::parse`'s existing (wider, `IF`-shared) structural
  validation.

- **`MW`'s P7 legend text vs. its own column diagram disagree** — the
  legend prose says `"00: (Fixed)"` (implying 2 digits), but the column
  diagram gives P7 exactly 1 wire column, matching `IF`/`MR`'s P7 width.
  Almost certainly a copy-paste artifact from the immediately adjacent P9
  legend line (also `"00: (Fixed)"`, but genuinely 2 columns wide in P9's
  case). Resolved in favor of the column diagram (ground truth for wire
  width, per this crate's established `IF`-verification practice), not
  the prose — flagged, not silently picked without documentation.

- **`MT` is a genuine superset of `MR`/`MW`'s shape, not a duplicate**:
  same 25-byte P1-P10 body plus a reserved P11 byte (unvalidated, same
  treatment as `ChannelStatusFields`'s own P9) plus the 12-character P12
  tag (genuinely new — no overlap with `ChannelStatusFields` at all).
  `MT`'s P7 legend explicitly states different values for Set vs.
  Read/Answer direction (`"Set: 0: (Fixed) / Read: 0: VFO 1: Memory"`) —
  this independently *confirms* (rather than merely parallels) the
  fixed-on-write/reported-as-Memory-on-read treatment already applied to
  `MR`/`MW` by inference from their own, less explicit legends.

- **Tag character-set/padding is a documented judgment call, not
  manual-cited on `MT`'s own page**: `MT`'s own legend states only `"(up
  to 12 characters) (ASCII)"`. Applied the CAT Operation section's general
  parameter rule (manual p.2) as the character-set restriction (printable
  ASCII space-tilde, excluding `;`) and adopted right-space-padding to the
  fixed 12-column wire slot as this implementation's own convention
  (inspired by, but not identical to, `ts570d`'s analogous fixed-width
  padding for a different field) — both explicitly flagged as
  implementation choices, not manual facts, in `MemoryChannelRecord`'s and
  `MemoryTag`'s doc comments.

- **A defensive fix applied to this task's own new code, not a fix to
  pre-existing code**: `CommandForm`'s width check in `cat-framework` is a
  *byte* length, not a char count, so direct byte-index slicing
  (`split_at`/`&s[a..b]`) on an untrusted parameter string can panic if
  stray multi-byte UTF-8 content lands off a char boundary — a real (if
  narrow) risk. `ChannelStatusFields::parse` already avoids this via
  `.get()`-based safe slicing; this task's new `Mt` write-arm code
  (originally written with `split_at`/direct indexing) was rewritten to
  match that same defensive style before landing. Noted for awareness:
  the pre-existing `Ex` arm (batch `EX`, landed earlier) still uses direct
  indexing (`&params[0..3]`) and has the same theoretical exposure — out
  of this task's scope to fix (a different command, a different task),
  flagged here rather than silently left unmentioned.

- No manual ambiguity blocked this task. `ChannelStatusFields` required no
  changes at all — full reuse for `MR`'s answer and `MW`'s Set, and for
  the first 25 bytes of `MT`'s Set/Answer.

## Wave 3 — CAT batch 1 (VFO/split/memory quick-ops: `AB BA AM VM MA CH QI
QR QS SV`)

- **All ten commands confirmed write-only per manual p.3** (Set O, Read X,
  Ans X, AI X) and independently re-confirmed against each command's own
  per-command box (printed p.4-5, p.11, p.14-15, p.17-18). Nine are
  genuinely zero-width triggers; `CH` is the sole exception (1-digit P1,
  `0`=UP `1`=DOWN, manual p.5).

- **`VM`/`AM` heading inconsistency, the architect's flagged item,
  confirmed exactly and resolved (not silently)**: `VM`'s own per-command
  box (printed p.18) is headed "VFO-A TO MEMORY CHANNEL" — byte-identical
  to `AM`'s own heading (printed p.4). Checked whether the wire-format
  boxes disambiguate first, per the task brief's instruction — they do
  **not**: both are structurally identical zero-width triggers with no
  P1/P2 columns at all, unlike `IF`'s Wave-1 ambiguity which genuinely did
  resolve from column numbers. Resolved via three corroborating,
  independent signals instead: (1) `VM`'s master-table name
  `"[V/M] KEY FUNCTION"` is the **only** bracketed entry in the entire
  20-page manual (confirmed via a full-text `grep` for `[`), strongly
  suggesting this notation specifically marks "emulates a physical
  front-panel key press"; (2) two byte-identical CAT commands with the
  same purpose would be a redundant manual design — `AM` already
  unambiguously covers "store" via its own non-bracketed heading; (3)
  well-established real-world Yaesu operating knowledge (outside this
  manual): the physical `[V/M]` key toggles VFO/Memory operating mode, it
  does not store anything. Implemented `VM` as toggling
  `Ft991aState::channel_select` between `0` (VFO) and `1` (Memory) —
  reusing `IF`'s existing P7 field from batch 9 rather than inventing new
  state. **Documented as a judgment call, not a manual-proven fact** — the
  wire shape genuinely cannot disambiguate the two commands; flagged for
  architect/hardware review.

- **State-model constraint inherited from Wave 1** (not discovered this
  task, but directly relevant): `Ft991aState` has a single `mode: u8`
  field, not one per VFO. `AB`/`BA`/`SV` therefore only copy/swap
  `vfo_a_hz`/`vfo_b_hz`; `AM`/`MA` copy the full set (frequency, mode,
  clarifier, tone, offset) since `MemoryChannelRecord` does carry a
  per-channel `mode`.

- **`QI`/`QR`'s dedicated QMB slot, confirmed from `IF`'s own P7 legend,
  not assumed**: batch 9's `IF` P7 legend (same manual page as this task
  reused, not re-read fresh) already lists `3`=QMB and `4`=QMB-MT as
  select values distinct from `1`=Memory — confirming the Quick Memory
  Bank is a separate single-slot storage location. Modeled as
  `Ft991aState::qmb: MemoryChannelRecord`, tested independently of the 117
  numbered channels (`framework_qi_qr_round_trip_independent_of_numbered_memory_channels`).

- **`CH`'s wrap-around and `QS`'s toggle semantics, both documented
  judgment calls**: the manual states no `CH` boundary behavior at
  channel 1/117 (implemented as wrap, not clamp) and no dedicated
  "split on"/"split off" command exists anywhere in the 91-command master
  table for `QS` to pair with (implemented as a plain boolean toggle).
  Neither is manual-cited beyond "the manual is silent here."

- **`Radio` trait scope, a judgment call made explicit**: added
  `copy_vfo_a_to_b`/`copy_vfo_b_to_a`/`swap_vfos`/`store_vfo_to_memory`/
  `recall_memory_to_vfo`/`memory_channel_up`/`memory_channel_down` to the
  trait (generic dual-VFO/memory-channel concepts, rounding out the
  existing `get_vfo_a`/`get_vfo_b`/memory-channel family). Kept `VM`
  (residual meaning uncertainty — the toggle's exact identity rests on
  corroborating evidence, not a crisp manual spec), `QI`/`QR` (FT-991A-
  named "Quick Memory Bank," distinct from the generic numbered-memory-
  channel concept already trait-covered), and `QS` (FT-991A-named "Quick
  Split," no generic on/off split concept exists elsewhere in this trait
  to attach a method to) as `Ft991a`-inherent-only methods
  (`toggle_vfo_memory_mode`/`qmb_store`/`qmb_recall`/`quick_split`) —
  flagged here as a judgment call, not silently decided.

- No manual ambiguity blocked this task in the "STOP and report" sense —
  the one genuine ambiguity found (`VM`/`AM`'s identical wire shape) was
  resolvable via corroborating evidence per the task brief's own guidance
  to try that before escalating, and is fully documented rather than
  silently picked.

## Wave 3 — CAT batch 3 (Clarifier/RIT-XIT + tone + IF-shift: `RT RC RD RU
XT CN CT IS`)

- **RX/TX clarifier relationship — confirmed from the manual, not
  assumed, resolving the architect's flagged item**: `RT`'s own
  per-command box (printed p.16, PDF page 17) is headed just "CLAR",
  `P1  0: RX Clarifier "OFF"  1: RX Clarifier "ON"` — a plain on/off flag,
  zero-width read (`RT;`), 1-digit set (`RT<0/1>;`). `XT`'s own box
  (printed p.18, PDF page 19, "TX CLAR") is structurally identical,
  `P1  0: TX CLAR "OFF"  1: TX CLAR "ON"`. Critically, there is **no**
  `XD`/`XU` pair anywhere in the master table (printed p.3) alongside
  `RD`/`RU` — ruling out "two independent offsets" and confirming **one**
  shared `clarifier_offset_hz` value, with `RT`/`XT` as independent gates
  on whether that one value applies to RX/TX respectively. This is also
  exactly what `IF`'s own P3/P4/P5 fields already modeled (batch 9,
  landed before any batch-3 `Set` command existed to change them) —
  `Ft991aState` already carried `clarifier_offset_hz`/`rx_clarifier_on`/
  `tx_clarifier_on` with a doc comment explicitly deferring to "batch 3";
  this task wires the `Set` side onto that pre-existing state rather than
  inventing new fields. `RU`'s own heading, "RX CLARIFIER PLUS OFFSET" —
  the specific thing the architect's brief flagged as a possible
  RX-only signal — was cross-checked against `IF`'s P3 legend (`"Clarifier
  Direction +: Plus Shift, --: Minus Shift"`, no RX/TX qualifier at all)
  and the absent `XD`/`XU` pair; concluded the "RX" in `RU`'s heading is a
  naming leftover, not evidence of a second offset.

- **`RD`/`RU`'s direction encoding, confirmed via `IF`'s own P3 field**:
  `IF`'s P3 legend (batch 9) reads `"Clarifier Direction +: Plus Shift,
  --: Minus Shift"` then `"Clarifier Offset: 0000-9999 (Hz)"` — sign +
  magnitude, matching `RD` ("DOWN"→minus) and `RU` ("PLUS OFFSET"→plus)
  exactly. Modeled both as **absolute sets** (`RD<mag>;` →
  `clarifier_offset_hz = -mag`, `RU<mag>;` → `= +mag`, overwriting not
  accumulating) rather than incremental steps — both boxes give P1 an
  explicit `0000-9999 (Hz)` magnitude field (unlike `CH`'s pure `0`/`1`
  direction selector, batch 1's genuine incremental-step command), and
  the manual never uses "increment"/"step" for either. **Documented
  judgment call**: an alternative "step by magnitude" reading cannot be
  100% ruled out from text alone, but absolute-set is what the explicit
  magnitude field most directly supports — flagged, not silently assumed.

- **`RC` (CLAR CLEAR), a documented judgment call**: zero-width Action
  trigger (manual p.15, `Set O Read X Ans X`, no parameter). Modeled as
  zeroing `clarifier_offset_hz` only, leaving `RT`/`XT`'s on/off gates
  untouched — matches how a physical "CLR" button next to a RIT/XIT dial
  conventionally behaves (zeroes the offset reading, doesn't disable
  RIT/XIT). Not itself manual-cited beyond "RC has no parameter to say
  otherwise."

- **`CT`, a genuine selector-read reusing already-landed state**: manual
  p.5's box (`C T P1 P2 ;` Set/Answer, `C T P1 ;` Read) is structurally
  identical to `MD`'s selector-read shape (1-char read, 2-char write).
  `CT`'s P2 legend (`0`:OFF `1`:CTCSS ENC/DEC `2`:CTCSS ENC `3`:DCS
  ENC/DEC `4`:DCS ENC) is byte-for-byte the same legend `IF`'s P8 already
  used (`Ft991aState::tone_status`, landed batch 9, explicitly deferred to
  "batch 3" in its own doc comment) — wired `CT`'s `Set` onto that
  existing field rather than adding a new one.

- **`CN`, the two lookup tables — transcribed and independently
  cross-checked, one error caught**: manual p.6's Table 1 (CTCSS Tone
  Chart, 50 entries, `000`-`049`) and Table 2 (DCS Code Chart, 104
  entries, `000`-`103`) were transcribed in full from the page image
  directly (not sampled or approximated), then cross-checked against the
  well-known standard 50-tone CTCSS / 104-code DCS lists used
  industry-wide (the same values appear across Yaesu/Kenwood/Icom
  equipment — a legitimate independent verification pass, not a
  substitute for reading the manual itself). One single-digit misread was
  caught this way: DCS table index 078 was initially read as `465`
  (duplicating index 077's `465`) — an easy 5/6 confusion in small print —
  corrected to `466` after the cross-check disagreed; documented in
  `ft991a_radio.rs`'s `DCS_CODES` doc comment, not silently fixed.
  Stored as `CTCSS_TONES_DECIHZ: [u16; 50]` (tenths of a Hz, not `f32`
  directly — a documented judgment call so index lookups compare integers,
  not floats, for equality) and `DCS_CODES: [u16; 104]` (plain code
  numbers). `CN`'s own box (`C N P1 P2 P3 P3 P3 ;` Set/Answer, `C N P1 P2
  ;` Read) makes P2 itself part of both read and write forms (unlike
  `CT`'s fixed-P1-only read) — a genuine two-width selector-read shape
  where the selector varies, not fixed.

- **`IS`, a resolved manual discrepancy — the highest-risk item this
  task found**: `IS`'s own per-command box (printed p.10, PDF page 11)
  gives the Set/Answer row as `I S P1 -/+ P2 P2 P2 ;` — only **three** P2
  cells. But the manual's own general "Parameters" worked example
  (printed p.2, PDF page 3) states outright "when the correct parameter is
  `IS0+1000` (IF SHIFT)" and separately flags `IS0+100;` (3 P2 digits) as
  an error — "Not enough digits (Only three frequency digits given)" —
  confirming the *correct* form needs **four** digits. Independently
  corroborated by the box's own stated range, `-1200 ~ +1200 Hz`: `1200`
  itself needs 4 digits (a 3-digit field maxes out at `999`). Two
  independent signals (the p.2 worked example + error catalog, and the
  box's own numeric range) agree with each other and disagree only with
  the box's column-count diagram — resolved in favor of 4 digits (the
  column diagram treated as a one-cell drafting omission), matching the
  p.2 example exactly (`"IS0+1000;"` body `"0+1000"`, 6 characters:
  1-digit P1 + 1-char sign + 4-digit magnitude). The `-1200~+1200 Hz
  (20 Hz steps)` range is enforced literally (`magnitude <= 1200 &&
  magnitude % 20 == 0`) — the "20 Hz steps" text taken at face value, a
  judgment call not independently re-confirmed by a second manual signal
  the way the digit-width resolution was. **Flagged and resolved, not
  silently picked** — full citation in `ft991a_radio.rs`'s module docs'
  "IS, a resolved manual discrepancy" section.

- **`Radio` trait scope**: clarifier/RIT-XIT (`get_rx_clarifier_on`/
  `set_rx_clarifier_on`/`get_tx_clarifier_on`/`set_tx_clarifier_on`/
  `clarifier_clear`/`clarifier_down`/`clarifier_up`) and IF-shift
  (`get_if_shift_hz`/`set_if_shift_hz`) were added as trait methods —
  `CLAUDE.md`'s "Radio trait scope" section names "RIT/XIT" directly, and
  IF-shift is a standard superhet-receiver concept, not FT-991A-specific.
  CTCSS/DCS tone *selection* was also added to the trait
  (`get_tone_squelch_mode`/`set_tone_squelch_mode` via a new
  [`ToneSquelchMode`] enum, `get_ctcss_tone_hz`/`set_ctcss_tone_hz`,
  `get_dcs_code`/`set_dcs_code`) — but exposed as **domain values** (Hz,
  3-digit code number), never the raw `CN` wire table index, since the
  specific 50/104-entry tables are FT-991A manual data per the task
  brief's own framing, not a trait-level concept. The Hz↔index and
  code↔index conversions live in `ft991a_radio.rs`
  (`ctcss_tone_index`/`ctcss_tone_hz`/`dcs_code_index`/`dcs_code_number`)
  and are invoked only at `ft991a.rs`'s client boundary — mirroring how
  `Mode`'s raw wire nibble stays internal to `ChannelStatusFields` while
  the trait-facing type is the domain `Mode` enum. Two new `RadioError`
  variants added for lookup failures: `InvalidCtcssTone(f32)`,
  `InvalidDcsCode(u16)`, plus `InvalidToneSquelchMode(u8)` and
  `InvalidIfShift(i16)` for the other two new domain-typed setters.

- No manual ambiguity blocked this task in the "STOP and report" sense.
  The `IS` P2-width discrepancy and the one DCS table transcription error
  were both resolved via corroborating evidence found within the manual
  itself (or, for the DCS table, an industry-standard cross-check) before
  writing any code — flagged and documented rather than either guessed
  silently or escalated unnecessarily, per the task brief's own guidance
  to try resolution first.

## Wave 3 — CAT batch 4 (Keyer/CW/break-in: `KM KP KR KS KY CS ZI BI SD`)

- **All nine wire shapes confirmed exactly as the architect's §10.5 batch 4
  summary described** — no corrections found this task, unlike several
  prior batches. Citations: `KM`/`KP`/`KR` manual p.10 (PDF p.11), `KS`/`KY`
  p.11 (PDF p.12), `BI` p.5 (PDF p.6), `CS` p.6 (PDF p.7), `SD` p.16 (PDF
  p.17), `ZI` p.18 (PDF p.19). Cross-checked via both `pdftotext -layout`
  (page-located via a `csplit` on the full extracted text, one page per
  file, matching printed-page = file-index exactly) and the rendered page
  image for each command's own box — zero discrepancies between the two
  extraction methods, unlike the `EX` sub-batch's item-072 misread.

- **`KM`'s variable-width shape, the batch's highest-risk item, resolved
  cleanly**: manual p.10's box gives Set as `K M P1 P2 P2 P2 P2 ~ P2 ;`
  with P2 legended "Message Characters (up to 50 characters)" — the first
  command in this crate whose write width is genuinely free-length (1-50
  chars) rather than one of a small enumerated set (`EX`'s 6 discrete
  widths). Used `cat-framework`'s already-present-but-previously-unused
  `CommandForm::variable(operation, min_len, max_len)` constructor for the
  write form (`2..=51` total chars) instead of enumerating 50 fixed
  widths — confirmed this constructor exists and behaves as an inclusive
  range by reading `cat-framework/src/cat.rs`'s `CommandForm::matches`
  directly before using it, not assumed from the API name alone.

- **Read-vs-write disambiguation forces a minimum 1-character message, a
  documented consequence not stated in the manual**: since `KM`'s read
  form (`KM<channel>;`) is exactly 1 character, the write form's minimum
  width must be ≥2 to stay structurally distinguishable — meaning this
  implementation cannot write (or represent) a 0-character "clear"
  message via `KM` alone. Same category of consequence as `MD`/`EX`'s
  established selector-read pattern, just newly relevant because `KM` is
  the first variable-width selector read in this crate.

- **`KY`, the task's explicitly flagged distinction, confirmed and kept
  separate — not conflated with the not-yet-consumed RTS/DTR feature**:
  manual p.11's box is Set-only (`KY<1 char>;`, blank Read/Answer rows,
  confirmed against the p.3 master table: `Set O Read X Ans X`). P1's
  legend splits into two families: `1`-`5` "Keyer Memory 'N' Playback" and
  `6`-`9`,`A` "Message Keyer 'N' Playback". Cross-referenced against the
  already-landed `EX` menu items 018-022 ("CW MEMORY 1"-"5", `EX` first
  sub-batch, manual p.7-9), each of which selects `0: TEXT`/`1: MESSAGE`
  playback mode for the *same* numbered `KM` channel — this independently
  confirms the two `KY` families are two playback **modes** for one
  5-channel `KM` store, not two separate message stores. Implemented as
  `KeyerPlaybackMode` (`KeyerMemory`/`MessageKeyer`), a tag over the same
  `Ft991aState::keyer_memories` array, not a duplicate array. `KY` is
  entirely CAT-driven and only ever plays back pre-stored `KM` content
  with no timing information; the RTS/DTR feature (§10.2-10.4,
  `ModemControlLines`, already landed in `radio-cat-rs` but not yet
  consumed on this repo's side) is real-time PC-driven keying of arbitrary
  Morse timing via serial control lines, with no CAT command and no
  pre-stored message involved at all — the two are unrelated features that
  happen to both be "CW keying," and this implementation's code for `KY`
  never references or touches anything RTS/DTR-shaped. Also explicitly
  checked (and rejected as a trap): `ts570d`'s own `Radio` trait has an
  unrelated `send_cw(message: &str)` method backing *its* manual's `KY`
  command (arbitrary free-text real-time keying) — a different
  manufacturer's command with the same 2-letter code and a genuinely
  different wire shape/semantics; this crate's `KY` does **not** get a
  `send_cw`-shaped method, since the FT-991A's `KY` has no free-text
  parameter at all.

- **`KY`/`ZI` mutate no persisted `Ft991aState` field — a documented
  judgment call, not an oversight**: neither command has anything
  meaningful for this software-only emulator to represent state-wise (no
  simulated sidetone/RF playback output for `KY`'s stored-message
  playback, no simulated received-signal frequency for `ZI`'s "CW AUTO
  ZERO IN" to zero-beat against). Both are modeled as pure
  acknowledgments, observable only via the `CommandOutcome::events` the
  handler pushes (tested directly via `CatFramework::process_frame`'s own
  return value, which is a `CommandOutcome`, not just the written wire
  bytes — confirmed this return type exists by reading
  `cat-framework/src/cat.rs`'s `process_frame` signature before relying on
  it in tests).

- **Reuse over duplication**: renamed `is_valid_tag_wire` (batch 2, written
  for `MT`'s tag) to `is_valid_ascii_wire_content` and reused it unmodified
  for `KM`'s message validation — both are the same general p.2 parameter
  character-set rule applied to a different variable-ASCII field, not two
  separate conventions. Updated `MT`'s existing call site to match; no
  behavior change to `MT`.

- **Per-field arbitrary defaults, same category as `meter_select`'s
  default (batch 9)**: none of the 7 new `Ft991aState` fields
  (`keyer_memories`, `key_pitch`, `keyer_on`, `key_speed`, `cw_spot_on`,
  `break_in_on`, `cw_break_in_delay_ms`) has a manual-stated factory
  default. Chose empty string for `keyer_memories` (vacant, matches
  `MemoryChannelRecord::tag`'s precedent), `false` for the three booleans
  (matches this crate's other undocumented-default booleans), and the
  minimum legal value for `key_speed`/`cw_break_in_delay_ms` (`4`/`30`) and
  `key_pitch` (`0`, i.e. 300 Hz) — all documented per-field, not silently
  chosen.

- **`Radio` trait scope, directly informed by checking `ts570d::Radio`'s
  own trait surface first (not just `CLAUDE.md`'s abstract list)**: found
  `ts570d::Radio` already has `get_keyer_speed`/`set_keyer_speed` and
  `get_semi_break_in_delay`/`set_semi_break_in_delay` for the Kenwood
  analogs of `KS`/`SD` — strong precedent that these (plus break-in on/off,
  CW spot, keyer pitch, keyer on/off, and zero-in) are genuinely
  cross-radio CW-operating concepts, not FT-991A-specific. Added all seven
  as trait methods (`get/set_break_in_on`, `get/set_semi_break_in_delay`,
  `get/set_cw_spot_on`, `get/set_keyer_speed`, `get/set_keyer_pitch_hz`,
  `get/set_keyer_enabled`, `zero_in`). `zero_in`'s trait method is shaped
  as a zero-width trigger (`RadioResult<()>`, no getter), **not** a
  get/set toggle like `ts570d::Radio::get_cw_auto_zerobeat`/
  `set_cw_auto_zerobeat` — a deliberate shape divergence, since the
  FT-991A's own manual gives `ZI` a blank Read/Answer row (confirmed
  write-only, not a toggle), documented on the trait method rather than
  forcing `ts570d`'s toggle shape onto a command that doesn't have one.
  `KM` (`read_keyer_memory`/`write_keyer_memory`) and `KY`
  (`play_keyer_memory`) were kept `Ft991a`-inherent-only, per the task's
  explicit instruction that the specific keyer-memory-message system is
  FT-991A-specific — consistent with `EX`'s earlier precedent of keeping
  "menu access"-shaped features off the trait.

- No manual ambiguity blocked this task. All nine wire shapes matched the
  architect's summary exactly; the two genuine design decisions this task
  made (`KM`'s minimum-message-length consequence, `KY`/`ZI`'s
  no-persisted-state modeling) are both documented judgment calls, not
  unresolved ambiguities requiring escalation.

## Wave 3 — CAT batch 5 (Scan/VOX/busy: `SC VX VD VG BY`)

- **All five wire shapes confirmed exactly as the architect's §10.5 batch 5
  summary described** — no corrections found this task. Citations: `BY`
  manual p.5 (PDF p.6), `SC` p.16 (PDF p.17), `VD` p.17 (PDF p.18), `VG`/`VX`
  p.18 (PDF p.19). Page numbers located via `pdftotext -layout` page-by-page
  before reading the rendered image, confirming the "PDF page = printed
  footer + 1" offset still holds.

- **`SC`, confirmed NOT the same shape as `TX`'s 3-valued answer, despite
  both being "3-valued"**: `TX`'s `2` is answer-only (never legally set);
  `SC`'s `0`/`1`/`2` are all legally settable per its own Set row (`S C P1
  ;`, all three values in the P1 legend, `Set O Read O Ans O` per p.3) — a
  plain three-way enumerated state, not an "answer can express more than
  Set can" situation. Modeled as a dedicated `ScanState` enum
  (`TryFrom<u8>`, mirroring `TxState`'s shape) rather than reusing
  `TxState` or forcing a bool — `ts570d::Radio::get_scan`/`set_scan` uses a
  plain bool for the analogous Kenwood command, but that would lose the
  FT-991A's genuine UP/DOWN scan direction, so this crate deliberately
  diverges from that precedent (documented on the trait method, not
  silently copied).

- **`VD`'s `EX` menu 142 dependency, transcribed verbatim and
  cross-referenced against the already-landed `EX_MENU_TABLE`, per the
  task's explicit instruction not to hide it**: `VD`'s own manual box
  (printed p.17) carries a doc note directly under its wire diagram: "VD
  command has different parameters to be changed according to the setting
  of Menu item '142 VOX SELECT'. 'MIC': VOX DELAY. 'DATA': DATA VOX DELAY."
  Confirmed via `pdftotext -layout` of the full 153-row `EX` menu table
  (manual p.18/PDF p.19) that item **142** ("VOX SELECT," `0: MIC 1: DATA`,
  1 digit) is a real, distinct row — not implemented by this task (out of
  scope; only 9 `EX` items landed so far, in a different sub-batch). **New
  finding beyond what the task brief stated**: two *further* menu items
  echo the identical MIC/DATA split for the adjacent gain/delay settings —
  **143** "VOX GAIN" and **144** "VOX DELAY" (MIC-side, matching `VG`/`VD`'s
  own ranges exactly) vs. **146** "DATA VOX GAIN" and **147** "DATA VOX
  DELAY" (DATA-side) — meaning the front-panel menu system stores MIC/DATA
  as **two separate settings** for gain too, not just delay. Since menu 142
  itself isn't implemented, this emulator cannot expose two separate
  stored values the way the front panel's menu system implies it might;
  `Ft991aState::vox_delay_ms`/`vox_gain` each model **one** shared value,
  addressed unconditionally by `VD`/`VG` regardless of what menu 142 would
  (if implemented) currently select. Documented on
  `Ft991aState::vox_delay_ms`, `Radio::get_vox_delay`, and
  `Ft991a::get_vox_delay`'s doc comments, drawing the parallel to
  `TxState::RadioKeyedNonCat`'s own "meaning depends on something outside
  this command's own wire bytes" category of open item, per the task's
  framing.

- **`VG`'s own manual box carries no equivalent doc note — flagged as an
  asymmetry, not silently assumed away**: only `VD`'s per-command box has
  the menu-142 note; `VG`'s box (same page, p.18) has no such text, even
  though menu items 143/146 suggest the identical MIC/DATA duality could
  plausibly apply to VOX gain too. This implementation does **not** treat
  `VG` as menu-142-dependent, since nothing on `VG`'s own manual page
  states it — the manual's own documentation is asymmetric here (one box
  flags the dependency, the sibling box doesn't), transcribed exactly
  rather than "fixed" by assuming symmetry that isn't written down.

- **`BY`, the same "no CAT-reachable driver" simplification category as
  `RI`'s status bits (batch 9)**: manual p.5's `BY` box has a blank Set
  wire-diagram row (legend text only, no `B Y P1 P2 ;` line under "Set") —
  confirmed read-only against the p.3 master table (`Set X Read O Ans O`).
  This emulator has no simulated received-signal/squelch-open condition
  anywhere in any landed batch, so `Ft991aState::rx_busy` always reports
  `false` — same documented-simplification treatment `Ft991aState::ri_status`
  already established, cited directly in `rx_busy`'s own doc comment rather
  than independently re-justified.

- **`Radio` trait scope, directly informed by checking `ts570d::Radio`'s own
  trait surface first (same practice batch 4 used)**: found
  `ts570d::Radio` already has `get_scan`/`set_scan`, `get_vox`/`set_vox`,
  `get_vox_gain`/`set_vox_gain`, `get_vox_delay`/`set_vox_delay`, and
  `is_busy` — strong precedent that scan, VOX (on/off, gain, delay), and
  busy status are all genuinely cross-radio concepts, not FT-991A-specific,
  independently confirming `CLAUDE.md`'s explicit naming of "scan" and
  "VOX" in its "Radio trait scope" section. All 5 commands' operations
  (10 methods total) were added to the trait — no `Ft991a`-inherent-only
  carve-out was needed this task, unlike several prior batches (`KM`/`KY`,
  `VM`/`QI`/`QR`/`QS`, etc.), since nothing about scan/VOX/busy is
  FT-991A-specific in the way those excluded features were. `get_rx_busy`
  is named to match this crate's own `get_*`/`set_*` convention rather than
  `ts570d::Radio::is_busy`'s naming, documented as a deliberate naming
  divergence from the otherwise-followed precedent.

- No manual ambiguity blocked this task in the "STOP and report" sense. The
  `VD`/menu-142 dependency was exactly what the task brief predicted and
  instructed how to handle; the one genuine *new* finding (menu 143/146
  extending the same duality to VOX gain, and `VG`'s own box not stating
  it) is a documented observation, not an unresolved ambiguity requiring
  escalation.

## Wave 3 — CAT batch 6 (Attenuator/preamp/noise/AGC/notch/filter-width: `RA
PA NB NL NR RL GT CO BP BC NA SH`)

- **All twelve wire shapes confirmed from each command's own per-command
  box** (manual printed p.4-5 `BC`/`BP`/`CO`, p.10 `GT`, p.13-14 `NA`/`NB`/
  `NL`/`NR`/`PA`, p.15 `RA`/`RL`, p.16 `SH`). All twelve are "selector read"
  shapes: a 1- or 2-char Read row carrying a fixed `P1=0` selector byte
  (never a zero-width `Query`), same treatment as batch 3's `CT`. `CO`
  confirmed to be CONTOUR (a parametric audio-EQ feature, P2=0/1) plus APF
  (Audio Peak Filter, P2=2/3) — not "carrier," despite what the 2-letter
  code alone might suggest; `BP`/`BC` confirmed as MANUAL NOTCH and AUTO
  NOTCH respectively (distinct commands, not variants of one notch
  feature); `NA` confirmed as NARROW (filter-width toggle), per its own
  master-table entry — see the typo finding below for why its own
  per-command box's wire cells cannot be trusted at face value.

- **`GT`'s write/report domain mismatch — a genuine manual asymmetry, not
  an extraction artifact**: `GT`'s own box (manual p.10) gives Set as
  `G T P1 P2 ;` with `P2` 5-valued (`0`-`4`: OFF/FAST/MID/SLOW/AUTO) but
  Answer as `G T P1 P3 ;` with `P3` **7**-valued (`0`-`6`: the same four,
  plus AUTO resolved into AUTO-FAST/AUTO-MID/AUTO-SLOW) — confirmed via both
  the rendered page image and `pdftotext -layout`, both agreeing. Modeled
  as a single `AgcMode` enum (the wider 7-valued domain) with two wire-value
  methods: `as_u8()` (the reported `P3` domain, `0`-`6`) and
  `set_wire_value()` (the settable `P2` domain, `0`-`4` — collapses
  `AutoMid`/`AutoSlow` onto the same wire value `AutoFast` uses, since
  `GT`'s Set command has no way to request a specific AUTO sub-variant).
  `Ft991aState::agc_mode` stores the full `P3` domain directly;
  `handle_command`'s `Gt` write arm stores `P2` verbatim (`0`-`3` map onto
  `P3` `0`-`3` identically; `P2=4` stores as `P3=4`, i.e. AUTO resolves to
  AUTO-FAST) — **a documented judgment call, not provable from the manual
  alone**: nothing states which AUTO sub-variant a plain "AUTO" `Set` should
  produce. `AutoMid`/`AutoSlow` (`P3=5`/`6`) are consequently unreachable
  via any CAT `Set` in this emulator, only reportable if seeded directly via
  `Ft991aRadio::from_state` — tested explicitly
  (`framework_gt_rejects_illegal_set_value_but_reports_seeded_wider_domain`).

- **`NA`, a genuine manual wire-diagram typo — resolved via three
  corroborating signals, not followed literally**: `NA`'s own per-command
  box (manual p.13) is correctly headed `NA` / `NARROW` and correctly
  placed in the master table's alphabetical `N`-block, but its own
  Set/Read/Answer wire-diagram cells literally spell out `M A P1 P2 ;` /
  `M A P1 ;` (the two-letter code `MA`, not `NA`) — confirmed present
  verbatim via both the rendered page image and `pdftotext -layout` (not an
  OCR artifact). Same category of manual self-inconsistency as batch 1's
  `VM`/`AM` heading clash and batch 2's `MW` P7 legend-vs-diagram mismatch.
  Resolved in favor of `NA` via three corroborating signals: (1) the master
  table's own alphabetized listing and heading unambiguously name this
  command `NA`; (2) `MA` is already a distinct, differently-shaped,
  already-landed batch 1 command ("MEMORY CHANNEL TO VFO-A," a zero-width
  Action trigger) — treating the wire cells literally would silently
  collide two unrelated commands onto one code, an untenable protocol
  design; (3) `NA`'s legend and overall shape are byte-for-byte identical to
  the immediately adjacent `NB` box (same page), strongly suggesting a
  copy-paste template error, not a deliberate design. Tested explicitly
  (`framework_na_round_trip_uses_na_wire_code_not_ma`, which also confirms
  a 2-parameter `MA01;` frame is rejected, not silently accepted as a
  collision).

- **`SH`, the batch's highest-risk item — full six-column bandwidth table
  transcribed and cross-checked, no discrepancy found this time**: manual
  p.16's table (22 rows, P2 `00`-`21`; columns SSB Narrow/Wide, CW
  Narrow/Wide, RTTY/PSK Narrow/Wide) was transcribed in full from the page
  image and independently cross-checked via `pdftotext -layout` — both
  extractions agreed exactly, unlike batch 3's `CN` DCS table (which did
  catch a one-digit misread this way). Stored as `SH_BANDWIDTH_TABLE: [
  ShBandwidthRow; 22]`, a named-field struct per row (not raw tuples), for
  readability. **Critically, `SH`'s own wire format (`S H P1 P2 P2 ;`)
  carries no mode or narrow/wide parameter at all** — nothing in `SH`'s own
  box text references the table's column headers. Two further judgment
  calls, both flagged: (1) `mode_family_for` maps only the modes literally
  named by the table's headers (`LSB`/`USB`→SSB, `CW`/`CW-R`→CW,
  `RTTY-LSB`/`RTTY-USB`→RTTY/PSK), leaving `FM`/`AM`/`DATA-*`/`C4FM`
  unmapped rather than guessing whether `DATA-LSB`/`DATA-USB` share the SSB
  or RTTY/PSK family; (2) the "narrow/wide" column selector is a
  well-evidenced but not literally-stated candidate for `NA`'s own on/off
  state (this same batch) — `filter_bandwidth_hz` takes `narrow: bool` as
  an explicit parameter for exactly this reason, but the connection to `NA`
  is never stated on either command's own page. `handle_command`'s `Sh`
  write arm deliberately does not cross-validate against current mode/`NA`
  state (accepts any `P2` `0`-`21` unconditionally) — consistent with this
  crate's established precedent of not inventing cross-command write-time
  validation beyond what a command's own wire format states.

- **`CO`'s APF frequency mapping, a documented judgment call**: `CO`'s
  `P2=3` item's own legend gives only the raw range (`0000`-`0050`) and the
  Hz endpoints (`-250` to `+250`), not an explicit formula. Modeled as a
  linear mapping (`raw=0`→`-250Hz`, `raw=25`→`0Hz`, `raw=50`→`+250Hz`,
  i.e. `hz = (raw - 25) * 10`) — the natural reading of the three given
  data points, not independently re-confirmed by a second manual signal.
  `BP`'s manual notch frequency, by contrast, is a direct manual-stated
  multiplier ("NOTCH Frequency: x 10 Hz," not a judgment call).

- **`Radio` trait scope, directly informed by checking `ts570d::Radio`'s
  own trait surface first (same practice batches 4/5 used)**: found
  `ts570d::Radio` already has `get_attenuator`/`set_attenuator`,
  `get_preamp`/`set_preamp`, `get_noise_blanker`/`set_noise_blanker`,
  `get_noise_reduction`/`set_noise_reduction`, and `get_agc`/`set_agc` —
  strong precedent that all five concepts are generic, not FT-991A-specific.
  Ten of the twelve commands' operations (20 methods: `RA`, `PA`, `NB`,
  `NL`, `NR`, `RL`, `GT`, `BC`, `NA`, `SH`) were added to the trait, with
  deliberate shape divergences from `ts570d::Radio` documented per-method
  (`PreampMode` instead of bool, noise reduction split into on/off + level
  instead of one combined field, `AgcMode`'s 7-valued domain instead of a
  raw time-constant `u8`). `CO` (Contour/APF, 8 methods) and `BP` (Manual
  Notch, 4 methods) were kept `Ft991a`-inherent-only — FT-991A-named
  parametric-EQ/audio-peaking features with no generic-concept precedent in
  either `CLAUDE.md`'s "Radio trait scope" list or `ts570d::Radio`, and
  distinct from batch 7's separately-scoped "audio chain" theme (`PL PR MG
  ML`) despite some surface similarity — flagged as a deliberate scope
  decision, not an oversight. `BC` (a plain bool) being on the trait while
  `BP` (a 2-item selector) is not is a documented asymmetry, not an
  inconsistency: the shape complexity, not the "notch" concept itself,
  drove the split.

- No manual ambiguity blocked this task in the "STOP and report" sense. The
  two genuine discrepancies found (`GT`'s domain mismatch, `NA`'s wire-cell
  typo) were both resolved via corroborating evidence found within the
  manual itself, following this crate's established practice, and are
  fully documented rather than silently picked or unnecessarily escalated.

## Wave 3 — CAT batch 7 (Speech processor/mic/monitor: `PL PR MG ML`)

- **All four wire shapes confirmed from each command's own per-command box**
  (manual printed p.11 `MG`, p.12 `ML`, p.14 `PL`/`PR`). `MG`/`PL` are plain
  query/set (no selector byte, same shape as `PC`); `PR`/`ML` are two-width
  "selector read" shapes (same treatment as `CT`/`NL`).

- **`PR`, a genuine manual heading typo, confirmed and resolved — the
  batch's one real finding**: `PR`'s own per-command box (manual p.14) is
  headed "SPEECH PROCESSOR LEVEL," byte-for-byte identical to `PL`'s own
  heading immediately above it — confirmed present verbatim via the
  rendered page image, not a `pdftotext` artifact. Same category as batch
  1's `VM`/`AM` clash and batch 6's `NA` wire-cell typo. Resolved via two
  independent signals, not the box's own (misleading) heading: (1) the
  master table (manual p.3) independently names this command just "SPEECH
  PROCESSOR," no "LEVEL"; (2) the command's own wire content is
  unambiguous regardless of heading — `P1` selects between two named
  features (`0`=Speech Processor, `1`=Parametric Mic EQ) and `P2` is an
  on/off value, not a numeric level. Implemented as an on/off toggle
  command, per both signals agreeing.

- **`PR`'s on/off wire encoding, transcribed exactly, not normalized**:
  `P2` is `1`=OFF, `2`=ON — the only on/off command in this crate's entire
  table (all 70 commands) that doesn't use the usual `0`=OFF/`1`=ON
  convention. Confirmed via the rendered page image; not an extraction
  artifact. `Ft991a::set_speech_processor_on`/`set_parametric_mic_eq_on`
  encode this explicitly (`if on { 2 } else { 1 }`), and
  `parse_pr_answer` decodes it the same way, both documented inline.

- **`ML`'s composite shape, a genuine two-item `P1` selector over a
  fixed-width `P2`, not literally spelled out as related to any other
  command**: `P1=0` addresses a `000`/`001` on/off boolean, `P1=1`
  addresses a `000`-`100` level — both packed into the same fixed 3-digit
  `P2` wire width regardless of which `P1` is in play. Structurally the
  closest precedent is `CN`'s two-item `P1` selector (batch 3, CTCSS vs.
  DCS table selection) rather than `NL`/`RL`/`SH`'s fixed-`P1="0"`
  selector-read shape (batch 6), even though `ML`'s total wire widths (1
  read / 4 write) exactly match `NL`'s.

- **`Radio` trait scope, directly informed by checking `ts570d::Radio`'s own
  trait surface first (established practice)**: `ts570d::Radio` has direct
  precedent for `MG` (`get_mic_gain`/`set_mic_gain`, u8) and `PR`'s
  Speech-Processor branch (`get_speech_processor`/`set_speech_processor`,
  bool) — both added to the trait, along with `PL`
  (`get_speech_processor_level`/`set_speech_processor_level`, the natural
  level companion to `ts570d`'s on/off method). `PR`'s Parametric Mic EQ
  branch has no `ts570d::Radio` precedent and no `CLAUDE.md`-listed generic
  concept — kept `Ft991a`-inherent-only, matching batch 6's `CO`/`BP`
  treatment exactly (FT-991A-named parametric-audio feature, no generic
  precedent). `ML`'s monitor on/off and level are the one **judgment call**
  in this batch's trait-scope decisions: `ts570d::Radio` has *zero* "monitor"
  concept anywhere (checked directly, not assumed), so unlike every other
  method added this task there is no direct precedent to point to. Added to
  the trait anyway, reasoning: an audio monitor (hearing one's own
  transmitted signal, "sidetone monitor" on some other radios) is a
  standard, near-universal transceiver concept, not an FT-991A-specific
  named feature the way CONTOUR/APF/parametric-EQ are, and `CLAUDE.md`'s
  "Radio trait scope" section's "gain controls"/"etc." language is broad
  enough to plausibly cover it. Flagged explicitly for architect review
  rather than silently decided, since it's the one trait-scope call this
  task made without a `ts570d::Radio` method to cite as direct precedent.

- No manual ambiguity blocked this task in the "STOP and report" sense. The
  one genuine finding (`PR`'s heading typo) was resolved via corroborating
  evidence found within the manual itself (the master table's own naming
  plus the wire content), following this crate's established practice, and
  is fully documented rather than silently picked or escalated
  unnecessarily.

## Wave 3 — CAT batch 8 (Band/step/encoder front-panel controls: `BS BU BD
FS ED EU EK DN UP`)

- **All nine wire shapes confirmed from their own per-command boxes**,
  citations: `BS`/`BU`/`BD` manual p.4-5 (PDF pages 5-6), `FS` p.9 (PDF
  p.10), `ED`/`EK`/`EU` p.7 (PDF p.8), `DN` p.6 (PDF p.7), `UP` p.17 (PDF
  p.18). Located page-by-page via `pdftotext -layout` piped through
  `csplit` on form-feed boundaries (20 extracted page files = 20 PDF
  pages, confirmed via `pdfinfo`), then cross-read against the master
  table's p.3 O/X flags before touching each per-command box.

- **`BS`'s full 16-band table, transcribed exactly, including the
  documented gap at index `13`**: `00`=1.8MHz, `01`=3.5MHz, `02`=5MHz,
  `03`=7MHz, `04`=10MHz, `05`=14MHz, `06`=18MHz, `07`=21MHz,
  `08`=24.5MHz, `09`=28MHz, `10`=50MHz, `11`=GEN, `12`=MW, `13`=*(no
  entry — genuine gap, present verbatim on the page image, not a
  transcription omission)*, `14`=AIR, `15`=144MHz, `16`=430MHz. 16 real
  bands across 17 wire codes (`00`-`16`). `BS` itself is Set-only (`B S
  P1 P1 ;`, manual p.3: `Set O Read X Ans X`) — no Read/Answer form
  exists at all for querying the current band back over CAT.

- **`DN`'s heading mismatch, the batch's flagged item, resolved via
  cross-radio corroboration — the highest-confidence resolution of this
  category found so far in this crate's history**: master table (p.3)
  names `DN`/`UP` plainly `"DOWN"`/`"UP"`; `UP`'s own per-command box
  (p.17) agrees exactly; `DN`'s own box (p.6) is headed **"MIC DWN"**
  instead — confirmed via a full-text search to be the only occurrence of
  "MIC" combined with UP/DOWN semantics anywhere in the manual's 20
  pages. Both wire formats are structurally identical zero-width Action
  triggers (`D N ;` / `U P ;`, manual p.3: `Set O Read X Ans X` for
  both), so — same as batch 1's `VM`/`AM` — the wire format alone cannot
  disambiguate. Per the task's explicit instruction to check "symmetric
  commands, wire-format columns, and any master-table clues" before
  guessing from the heading, checked `ts570d`'s Kenwood TS-570D CAT
  protocol directly (a sibling repo at
  `/home/mattfranklin/src/github.com/kf0uwv/ts570d`, present in this
  sandbox): it defines **wire-identical** `UP`/`DN` commands
  (`ts570d/radio/src/ts570d_radio.rs`: `definition!(Up, "UP", "Frequency
  Up", NONE, NONE, ACTION)`, `definition!(Dn, "DN", "Frequency Down",
  NONE, NONE, ACTION)`), implemented via `ts570d::Radio::mic_up`/
  `mic_down` under a `radio_trait.rs` section literally titled `"MIC
  up/down (write-only momentary)"`, backed by
  `ts570d_radio_handlers.rs`'s own inline comments (`"UP — VFO frequency
  up by 100 Hz (Menu 02 default step, write-only)"` /
  `"DN — VFO frequency down by 100 Hz ..."`). A **different
  manufacturer's** CAT protocol independently landing on the exact same
  two 2-letter codes for the exact same "hand mic UP/DWN button" concept
  is strong, independent corroboration (not a naming coincidence given
  the byte-identical codes) — resolved `DN`/`UP` as mic-button commands.
  Implemented as `Ft991a::mic_down`/`mic_up` (mirroring `ts570d`'s exact
  method names, also added to the `Radio` trait under those names), each
  stepping `Ft991aState::vfo_a_hz` by a fixed `MIC_STEP_HZ` (`10`, a
  documented arbitrary choice — neither manual states an exact
  Hz-per-press value; `ts570d`'s own emulator picks a different arbitrary
  value, `100`). Deliberately **not** tied to `FS`'s `fast_step_on`
  state, mirroring `ts570d`'s own design where `FS`/`UP`/`DN` are
  unlinked despite being adjacent commands in both manuals.

- **`ED`/`EU`/`EK`, a documented "no simulate-able effect" judgment call**:
  `ED`/`EU` (manual p.7) validate `P1` (encoder selector, `0`/`1`/`8`) and
  `P2` (`01`-`99` step count) structurally and semantically, but mutate no
  persisted `Ft991aState` field — the manual's own legend states the
  actual Hz-per-step depends on what function the selected encoder is
  currently assigned to (front-panel/menu state this emulator doesn't
  model), so inventing a concrete frequency effect would be pure
  speculation, unlike `DN`/`UP` which had direct cross-radio precedent to
  lean on. Same category as batch 4's `KY`/`ZI` "no persisted state"
  treatment. `EK` (ENT KEY, p.7) is a zero-width Action trigger, same
  treatment as `ZI`/`RC`.

- **`Radio` trait scope, directly informed by checking `ts570d::Radio`'s
  own trait surface first (established practice)**: `FS`
  (`get_fine_step`/`set_fine_step`) and `DN`/`UP` (`mic_up`/`mic_down`)
  are direct `ts570d::Radio` precedent, added unchanged (same method
  names). `BS`/`BU`/`BD` (`set_band`/`band_up`/`band_down`) have **no**
  `ts570d::Radio` precedent at all (`ts570d` has no band concept
  anywhere, checked directly) — added to the trait anyway per this
  task's own explicit framing ("band select/up/down are fairly generic")
  and this crate's established "near-universal transceiver concept,
  added anyway, flagged for review" treatment (same category as batch
  5's `ScanState`, batch 7's `ML`). `set_band` has no paired getter
  (`BS`'s own wire format has no Read/Answer at all). `ED`/`EU`/`EK`
  stay `Ft991a`-inherent-only, per this task's own framing that encoder/
  front-panel-key concepts are FT-991A-specific — consistent with the
  crate's established `KM`/`KY`/`CO`/`BP` precedent.

- **`BU`/`BD`'s wrap-around behavior, a documented judgment call**: the
  manual states no boundary behavior for stepping past the highest/lowest
  band (same category of open item as batch 1's `CH` wrap-around).
  Modeled as wrapping (`next_band`/`prev_band`), skipping the `13` gap
  entirely — `BAND_CODES` deliberately excludes `13`, so neither helper
  can ever land on it.

- No manual ambiguity blocked this task in the "STOP and report" sense.
  The one genuine finding this batch's own brief flagged (`DN`'s heading
  mismatch) was resolved via corroborating evidence found *outside* this
  manual (a sibling repo's independently-arrived-at Kenwood CAT protocol)
  rather than guessed from the heading text alone, per the task's own
  instruction — fully documented rather than silently picked.

## Wave 3 — CAT batch 10 (last of the 10 core batches): misc system/TX/tuner/DVS
(`AC AI DA DT LK OI OS FT TS MX LM PB`)

- **`TS`, a genuine mismatch against the architect's own dispatch-prompt
  guess ("tuning step?"), resolved from the manual's own per-command box,
  not the guess**: manual p.17, headed unambiguously "TXW" — a plain
  boolean with no elaboration anywhere in the manual of what "TXW" means.
  Implemented per the manual's actual wire box; not modeled as tuning step.

- **`OI` confirmed to share `IF`'s `ChannelStatusFields` shape exactly —
  verified column-by-column against the manual image, not assumed from the
  architect's cross-batch note alone**: manual p.13. A one-cell rendering
  ambiguity in the page image (the first 10-column sub-row appears to show
  only 4, not 5, `P2`-frequency digit cells) was investigated, not silently
  waved through — cross-checked against `IF`'s own box (identical apparent
  pattern) and independently against `FA`'s unambiguous two-row 8+1
  frequency split; both signals agree `OI`'s `P2` is the full 9-digit field
  `IF`'s is, the missing cell being a low-resolution render artifact common
  to both boxes. `ChannelStatusFields::to_wire_string` reused **unmodified**.
  The sole real difference from `IF`'s payload: `OI`'s `P2` is `vfo_b_hz`,
  confirmed directly from the legend text ("VFO-B Frequency" vs. "VFO-A
  Frequency"). All other fields (`P1`,`P3`-`P10`) read from the exact same
  shared, non-per-VFO state `IF` already uses — an inherited Wave-1
  state-model constraint (single `mode`/`clarifier`/etc. fields, not one set
  per VFO), not a new discovery this task made; flagged for
  architect/hardware review, not silently assumed correct.

- **`FT`, a genuine write/report domain mismatch, confirmed via the image**:
  manual p.9. Set's `P1`∈{`2`,`3`} and Answer's `P2`∈{`0`,`1`} both encode
  the identical two states ("VFO-A Band Transmitter: TX" / "VFO-B..."), just
  with different wire digits — a direct 1:1 remap, distinct from `GT`'s
  batch-6 *widening* mismatch (Set 5-valued, Answer 7-valued) but the same
  broader category ("a command's Set and Answer don't share one wire
  domain"). Confirmed present verbatim via the rendered page image, not an
  extraction artifact.

- **`OS` reuses `Ft991aState::offset_type` directly, confirmed as the
  intended design, not a coincidence**: that field was declared in batch 9
  with a doc comment explicitly reading "No `Set` command in any landed
  batch changes this yet (`OS` is batch 10)" — this task's `OS` write arm
  wires onto it with zero new state needed, matching the architect's own
  cross-batch field-sharing intent exactly.

- **`LM`/`PB`'s differing `P2` semantics, transcribed exactly, not assumed
  symmetric despite their otherwise-identical wire shape**: `LM`'s own
  legend (manual p.11) phrases every non-zero `P2` value as a per-channel
  "Recording **Start/Stop**" — a toggle, where sending the currently-active
  channel's own number stops it, and any other non-zero value starts a new
  one. `PB`'s own legend (manual p.14) phrases its non-zero values as
  "Playback **Start**" only — no toggle wording, so a repeated send of the
  same channel just (re)starts it, unconditionally; only `P2=0` stops
  playback. Confirmed present verbatim on both pages, not a copy-paste
  assumption from one command's legend onto the other's.

- **`AC`'s Read row is genuinely zero-width, not a selector read — checked
  directly rather than pattern-matched from this crate's many other
  fixed-`P1`-selector commands**: manual p.4, `AC`'s Read row is literally
  `A C ;` with no `P1` column, unlike `RA`/`PA`/`CT`/`OS`/`LM`/`PB` etc.,
  which all carry a fixed `P1="0"` selector on Read. A structural exception
  worth flagging since it would have been easy to assume uniformity across
  this crate's now-large set of "selector read" commands.

- **`DA`'s narrow `P2` range (`01`-`02`, only 2 legal LED-brightness values)
  looked suspicious enough on first extracted-text read to warrant an
  independent image re-check — confirmed correct, not a `pdftotext`
  artifact**: manual p.6, the rendered page image's own column diagram
  agrees exactly with the extracted text (`P1`: 2 digits fixed "00", `P2`:
  2 digits "01"-"02", `P3`: 2 digits "00"-"15"). No discrepancy found; the
  narrow range is a genuine manual fact, not a misread.

- **`DT`'s 3-shape design, confirmed exactly as the architect's brief
  predicted, no discrepancy found**: manual p.6. `P1` selects `P2`'s shape
  (date `yyyymmdd`/8-digit, time `hhmmss`/6-digit, offset
  `-hhmm`/`+hhmm`/1-sign+4-digit) — implemented via 4 `DT_SET_FORMS` widths
  (`1`,`6`,`7`,`9`), all mutually distinguishable by length, with `P1`
  cross-checked against the actual width used at write time (same
  "structural match succeeded, semantic validation still per-item" pattern
  `EX`/`FA` already established) — successfully served as the smaller
  rehearsal of `EX`'s pattern the architect's brief anticipated.

- **`Radio` trait scope, directly informed by checking `ts570d::Radio`'s own
  trait surface first (established practice)**: `AI` (`set_auto_info`,
  narrowed from `ts570d`'s 4-valued mode to a plain bool), `LK`
  (`get_frequency_lock`/`set_frequency_lock`, unchanged), and `FT`
  (`get_tx_vfo`/`set_tx_vfo`, narrowed from `ts570d`'s 3-valued domain
  — VFO-A/VFO-B/Memory — to the FT-991A's own 2-valued one, no
  memory-channel-TX option) all have direct `ts570d::Radio` precedent.
  `OS` (`get_repeater_shift`/`set_repeater_shift`) and `MX`
  (`get_mox_on`/`set_mox_on`) have **no** `ts570d::Radio` precedent
  (`ts570d` is an HF-only rig with no FM-repeater or MOX concept, checked
  directly) — added anyway as near-universal transceiver concepts, the same
  "no direct precedent, flagged for review" treatment `Band`/`ScanState`/
  `ML` already received in prior batches. `AC` (antenna tuner — explicitly
  excluded by both this repo's and `ts570d`'s own `CLAUDE.md`), `DA`
  (dimmer — display hardware), `DT` (date/time — system/utility setting,
  closer in kind to `EX`'s off-trait menu settings), `OI` (composite status
  dump, following `IF`'s own precedent), `TS` (meaning genuinely uncertain
  even after reading the manual), and `LM`/`PB` (FT-991A-specific DVS
  system, mirroring `KM`/`KY`'s exclusion) were all kept
  `Ft991a`-inherent-only — six judgment calls, each individually flagged
  rather than silently decided.

- No manual ambiguity blocked this task in the "STOP and report" sense. The
  one genuine mismatch found (`TS` vs. the architect's dispatch-prompt
  guess) was resolved cleanly from the command's own manual box, and the
  one apparent discrepancy investigated (`OI`'s rendering-quirk digit count)
  resolved via corroborating evidence from two independent sources within
  the manual itself, following this crate's established practice — both
  fully documented rather than silently picked or unnecessarily escalated.
  This was the last of the 10 core CAT batches; all 91 top-level (non-`EX`)
  CAT commands are now implemented.

## Wave 3 — RTS/DTR modem-control-lines consumption (§10.4)

- **`ModemControlLines` matches §10.4's sketch exactly, no shape
  deviation**: `cat-transport-core/src/modem.rs`'s trait is byte-for-byte
  what the architect's brief described — `set_rts`/`set_dtr`/`read_cts`/
  `read_dsr`/`read_dcd`, all `&self`, all `Result<_, TransportError>`, no
  `#[async_trait]`. `cat-transport-serial`'s blanket `SerialCatSession<T:
  Transport + ModemControlLines>: ModemControlLines` delegation and
  `SerialPort`'s own `TIOCMBIS`/`TIOCMBIC`/`TIOCMGET`-based impl both match
  §10.3's description too. Confirmed by reading both files directly in the
  sibling `radio-cat-rs` checkout, not assumed from the plan's prose.

- **BLOCKING FINDING, the actual substance of this task's report**: the
  `ModemControlLines` work in `radio-cat-rs` exists only as **uncommitted
  working-tree changes** — `git status` in that checkout shows `modem.rs`
  untracked and `session.rs`/`io_uring.rs`/`lib.rs` modified-not-staged;
  `git fetch origin && git log origin/main` confirms `origin/main`'s tip is
  still the pre-`ModemControlLines` extraction commit. This repo's
  `cat-transport-core = { git = "...", branch = "main" }` dependency
  therefore cannot see this code at all — not a version-pinning issue, a
  genuine "the commit doesn't exist yet" gap. `cargo build -p radio`
  against the real, unmodified dependency graph fails with `unresolved
  import cat_transport_core::ModemControlLines`. This contradicts the
  task's own framing ("already landed there, done") — flagged per the
  explicit instruction to trust the real source over the plan's prose and
  to report discrepancies rather than force a mismatched shape (this one
  isn't a shape mismatch — the API is exactly right — but a build-graph
  gap that sits outside what any local code change can fix).

- **Verified anyway, without leaving a permanent trace**: added a temporary
  `[patch."https://github.com/kf0uwv/radio-cat-rs"]` table to root
  `Cargo.toml` (not a `[dependencies]`/`[[bin]]` edit, so within the task's
  stated constraint) pointing the four affected crates at the sibling
  checkout's local paths purely to prove this task's own new code is
  correct against the real trait signatures — ran the full verification
  suite under that patch (all clean, see progress.md), then reverted
  `Cargo.toml` to a `diff`-confirmed byte-identical match of its pre-task
  state and restored `Cargo.lock` from a pre-task backup (also
  `diff`-confirmed identical). Re-confirmed the unpatched build fails
  exactly as described, so the repo is left in its true, honest state, not
  a silently-patched one.

- **`main.rs` zero-change claim, confirmed not just asserted**: under the
  temporary patch, `cargo build --workspace` succeeded with zero edits to
  `src/main.rs` — `SerialPort: ModemControlLines` composing with
  `SerialCatSession<T: Transport + ModemControlLines>: ModemControlLines`
  really does give `Ft991a<SerialCatSession<SerialPort>>` (exactly what
  `main.rs` constructs today) the new bound for free. This part of §10.4's
  design is correct and verified, independent of the commit/push gap above.

- **Error mapping — no new `RadioError` variant needed**: `RadioError`
  already has `Transport(#[from] TransportError)` (`radio_trait.rs`), which
  `ModemControlLines`'s `Result<_, TransportError>` methods map onto
  directly via `.map_err(Into::into)` — the same idiom every other
  `Ft991a` method in this file already uses at its own `cat_client`
  boundary. No new error variant, no new `From` impl.

- **Test double — reused the existing `FakeTransport`/
  `SerialCatSession<FakeTransport>` fixture, no new fake session type**:
  gave the existing in-module `FakeTransport` (already used for every
  wire-byte-level test in `ft991a.rs`) a `ModemControlLines` impl via
  `Cell`s (mirroring `cat-transport-serial::session`'s own test-module
  `FakeTransport` precedent, the canonical fake for this trait), since
  `SerialCatSession<T: Transport + ModemControlLines>: ModemControlLines`
  already exists upstream as a blanket impl — adding the capability to the
  fake `Transport` was sufficient to make
  `Ft991a<SerialCatSession<FakeTransport>>` satisfy this task's new bound.

- No design-level ambiguity blocked this task — §10.4's sketch translated
  directly into working code with zero shape adjustments needed. The one
  real problem found (the commit/push gap) is an infrastructure/process
  issue, not a design or API-shape issue, and is outside this agent's
  authority to fix (explicit "read-only reference" constraint on
  `radio-cat-rs`) — flagged for architect/coordinating-session action.

## Wave 3 — `EX` menu, second sub-batch (items 001-046, minus 027) (2026-07-19)

- **Full 001-153 table re-read up front** (`pdftotext -layout`, printed
  p.7-9 / PDF p.8-10) before committing to a sub-batch boundary, so the
  001-046 cut was an informed choice against the manual's own visual
  page-1 grouping, not an arbitrary stopping point.

- **068/069 Digits-column swap caught, adjacent to but outside this
  sub-batch's range**: `pdftotext -layout`'s first pass showed 068 "DATA
  HCUT FREQ" as Digits=1 and 069 "DATA HCUT SLOPE" as Digits=2 — backwards
  vs. every sibling `*HCUT FREQ`(2)/`*HCUT SLOPE`(1) pair on the same page
  (CW/RTTY/SSB all agree: FREQ=2, SLOPE=1). The rendered page image
  confirmed the correct 2/1 order. Both items are in the next sub-batch's
  scope (049-079), not landed here — flagged so the next task doesn't
  re-derive it.

- **027 "TIME ZONE" skipped, not guessed**: its P2 cell (`UTC -12:00 ~
  +14:00`) has no `"(P2 = ...)"` wire-encoding formula anywhere in the
  manual, unlike its signed-range siblings 035 ("QUICK SPLIT FREQ") and
  039 ("REF FREQ ADJ"), which both state one explicitly. Real-world UTC
  offsets also aren't uniformly stepped (e.g. `+05:30`, `+05:45`,
  `+12:45`), so a guessed encoding risked being wrong in a way this manual
  alone cannot resolve — same treatment as item 087 "RADIO ID."

- **New `ExMenuValueKind` enum** (`Enumerated`/`Range{min,max,step,signed}`)
  added to model continuous numeric ranges, which the first sub-batch's 9
  items (all small named pick-lists) never needed. `ExMenuItem.legal_values`
  renamed to `kind: ExMenuValueKind`; the 9 existing rows converted to
  `Enumerated` with zero behavior change (verified: all 9 pre-existing `EX`
  tests pass unmodified). `Ft991aState::ex_menu_value`/`set_ex_menu_value`
  widened `u8`→`i32` to hold this sub-batch's signed and 4-digit values;
  the first sub-batch's 9 `u8` state fields are unchanged, only cast at
  this boundary.

- **Signed-zero dual encoding, a real manual detail, not an edge case
  invented for testing**: items 035/039 explicitly permit both `"+00"` and
  `"-00"` for zero. Parsing sign+magnitude into a plain signed integer
  handles both inputs for free, but means the read-back is always
  canonical `"+00"` regardless of which sign was written — tested
  explicitly (`framework_ex_signed_zero_collapses_to_canonical_plus_zero_
  on_read`), not left as an undocumented surprise.

- **Latent zero-padding bug in the first sub-batch's read-response
  formatting, found and fixed before it could bite**: `format!("EX{p1_str}
  {value};")` never zero-padded — harmless while every landed item was
  digit-width-1, but would have emitted wrong wire output (e.g. `"5"`
  instead of `"005"`) for any digit-width>1 item, several of which this
  sub-batch introduces. Replaced with `ExMenuValueKind::format(value,
  digits)`. Caught during implementation, not via a failing test in a
  later session.

- **Value-encoding classification is a judgment call for ~14 items**
  (005, 008, 010, 011, 014, 015, 017, 025, 026, 035, 036, 039, 041, 043,
  046): the manual gives these as a bare `min ~ max` (or `min ~ max` plus
  a step note), not a named `X: LABEL` legend for every value, so they're
  modeled as `Range` rather than `Enumerated`. `step` is taken from the
  manual's own wording where stated, else assumed `1` — a documented
  assumption for those specific items, not manual-cited.

- **Default-value policy, consistent but arbitrary** (no factory default
  stated anywhere in the manual): `Enumerated` → first legend value;
  unsigned `Range` → `min`; signed `Range` (035, 039) → `0` (the neutral
  "no adjustment" point, itself a legal manual-shown value, not an
  arbitrary endpoint pick).

## Wave 3 — `EX` menu, third sub-batch (items 049-079) (2026-07-19)

- **068/069 digit-width discrepancy, definitively resolved via a fresh
  300 DPI page render — the prior sub-batch's own image-based resolution
  was itself wrong, not just `pdftotext`'s extraction**: this task
  re-rendered PDF page 9 (printed p.8) at 300 DPI and zoomed 2x directly on
  the 068/069 rows. The manual's literal printed table shows **068 "DATA
  HCUT FREQ" Digits=1, 069 "DATA HCUT SLOPE" Digits=2** — the same order
  `pdftotext -layout` already reported, contradicting the second
  sub-batch's findings.md claim that "the page-image read confirmed the
  correct 2/1 order." That prior claim does not hold up against a careful
  re-check. However, the literal 1/2 reading is functionally impossible
  for 068 (its own legend needs values 00-67, which cannot fit in 1
  digit) and contradicts every one of 6 sibling `*HCUT FREQ`/`*HCUT SLOPE`
  pairs on the same page (all 2/1, no exceptions: 043/044, 052/053,
  066/067, 092/093, 094/095, 102/103, 104/105). Implemented as **068
  Digits=2, 069 Digits=1**, treating the manual's printed order for this
  one row pair as a genuine typesetting error (most likely an
  adjacent-row swap), not a real protocol difference — a judgment call
  corroborated by two independent kinds of evidence, not a guess, and
  explicitly not treated as the second sub-batch's claimed "already
  confirmed" fact. Flagged for hardware verification like any other
  judgment call in this table.

- **First functional test of `ExMenuValueKind::Range`'s signed encoding at
  a wider magnitude**: items 064/065 ("OTHER DISP/SHIFT (SSB)") are
  signed, digits=5 (1 sign + 4 magnitude), `-3000..=+3000` in 10 Hz steps
  — the first signed items since 035/039 (digits=3, 1 sign + 2 magnitude).
  No code change was needed in `ExMenuValueKind::parse`/`format` — the
  existing sign-then-magnitude logic generalizes to any magnitude width
  for free, confirming the second sub-batch's design was already general
  enough. Same dual-zero-encoding behavior (`"-0000"` canonicalizes to
  `"+0000"` on read) as 035/039, tested explicitly.

- **No item in 049-079 was unresolvable** — unlike the second sub-batch's
  027 ("TIME ZONE"), every item in this range had either an explicit
  named legend or an explicit `(P2 = ...)` wire-encoding formula. 068/069
  needed a judgment call (see above) but not a skip.

- **`EX_SET_FORMS` needed zero changes**: this sub-batch's digit widths
  (1, 2, 3, 4, 5) were all already present, confirming the first
  sub-batch's "build all 6 widths ahead of need" design paid off exactly
  as intended for a second consecutive sub-batch.

- **Two pre-existing tests needed updating, not left stale**: 
  `ex_menu_item_returns_none_for_any_unlanded_p1` used 49 and 61 as
  example "real but not yet landed" menu numbers — both are landed by
  this task, so the examples were swapped to 80 and 100 (still genuinely
  unlanded). `framework_ex_out_of_table_p1_fails_cleanly_not_a_panic`
  used EX049 as its concrete "unlanded item" example — swapped to EX080
  ("RPT SHIFT 28MHz", also unlanded, 4-digit shape). Both are necessary
  updates given this task's own scope, not incidental churn.

- **`EX_MENU_TABLE`'s entry count grew from 54 to 80** (26 net new rows:
  31 numbers in 049-079 minus 5 already-landed by the first sub-batch).
  The integrity test asserting the table's exact size and P1 set was
  updated accordingly (`ex_menu_table_has_exactly_eighty_entries`,
  renamed from `...fifty_four_entries`).

## Wave 3 — `EX` menu, fourth sub-batch (items 080-153, minus 087 and 108/109) (2026-07-19)

Per the architect's dispatch: implement `EX` menu items 080 onward, no
fixed upper bound, covering as much as can be transcribed cleanly and
confidently. Read the full 080-153 range up front (both `pdftotext
-layout` and a 300 DPI rendered-page-image read, column-by-column) before
committing to a stopping point — same methodology as the third sub-batch's
068/069 resolution, applied here to every item, not just the ones that
turned out ambiguous. Result: **the entire remaining range (080-153,
minus 087 and 108/109 already landed) transcribed cleanly** — nothing was
left for a "next sub-batch." `EX_MENU_TABLE` now has 151 entries, covering
every one of the 153 manual menu numbers except 027 ("TIME ZONE") and 087
("RADIO ID"), both permanently unresolvable from this manual alone.

- **Two independent extraction methods used, same rigor as prior
  sub-batches**: `pdftotext -layout -f 8 -l 10` (covering PDF pages 8-10,
  printed p.7-9, in one pass) transcribed all of 080-153 first; then
  `pdftoppm -png -r 300` re-rendered PDF pages 9 and 10 (printed p.8-9) and
  each was read directly via cropped high-resolution image sections,
  column-by-column, cross-checked against the `pdftotext` pass. Every row
  in 080-153 agreed between both extractions except the two items detailed
  below (100, 147) — both resolved via corroboration, not guessed or
  skipped.

- **100 "RTTY SHIFT FREQ", a genuine manual typo — resolved via
  corroboration, the same rigor standard the task brief required for any
  ambiguous item**: the manual's own printed P2 legend reads `1: 170 Hz
  1: 200 Hz  2: 425 Hz  3: 850 Hz` — a duplicate `1:` label. Confirmed
  identical on both `pdftotext -layout` and a tight 300 DPI crop zoomed 2x
  directly on the row (not a rendering/OCR artifact — the manual itself is
  printed this way). Two independent corroborating signals resolved this
  to 0-based (`0:170Hz 1:200Hz 2:425Hz 3:850Hz`, treating the first
  printed `1:` as a typesetting error for `0:`): (1) every other 4-value
  single-digit selector in the full 153-row table, without exception (012,
  016, 029, 030, 031, 032, 090, and others), is zero-based — the only
  documented exceptions are 2-value "PORT SELECT"-family fields (072, 077,
  and this table's own 101 "RTTY MARK FREQ") which start at 1, a narrower,
  already-documented pattern that does not extend to a 4-value field; (2)
  170 Hz is the well-established real-world default/standard amateur-radio
  RTTY (FSK) shift, matching this table's own convention of `0` as the
  neutral/default/most-common option elsewhere. Implemented as
  `Enumerated(&["0", "1", "2", "3"])`, locked in by a dedicated regression
  test (`framework_ex_100_rtty_shift_freq_typo_resolution_is_zero_based`).

- **147 "DATA VOX DELAY" step assumption, a documented judgment call, not
  a manual-stated fact for this item's own row**: 147's P2 cell reads `30
  ~ 3000 msec (P2 = 0030 ~ 3000)` — no step note, confirmed not a
  column-truncation artifact via the same 300 DPI re-render (the full row
  text is visible and simply omits the note). Sibling item **144 "VOX
  DELAY"** — the identical underlying quantity, MIC vs. DATA variant, a
  duality batch 5's `VD` command finding already ties to menu items
  144/147 by explicit manual citation — states `10 msec/step` for the same
  `0030~3000` range. Applying `step=10` to 147 (rather than the
  functionally-implausible literal default of every millisecond value
  being legal) is this implementation's judgment call, flagged for
  hardware/architect review, not silently assumed. No dedicated
  regression test locks this one in beyond the general round-trip test's
  step-rejection cases (`0031` rejected) — the manual gives no way to
  independently prove the step value the way 068/069's legend arithmetic
  did.

- **116 "SCP SPAN FREQ" gap, transcribed exactly, not treated as an
  error**: legal P2 values are `03`-`07` only (`00`-`02` absent from the
  manual's own legend) — same treatment as `RI`'s selector gap (batch 9)
  and item 028's `GPS/232C SELECT` gap (second sub-batch). Modeled as
  `Enumerated(&["03", "04", "05", "06", "07"])` — the first 2-digit-width
  `Enumerated` item in this table (all prior `Enumerated` items were
  1-digit); `ExMenuValueKind`'s existing `parse`/`format` needed no code
  change to support this, confirming the design generalizes to any digit
  width for free.

- **Parametric-EQ sextet (119-136), a large structurally-repetitive
  block**: 6 FREQ/LEVEL/BWTH triples (3 "PRMTRC EQ1-3" + 3 "P-PRMTRC
  EQ1-3"). FREQ items are contiguous-from-`00` named-frequency-point
  selectors (`00`=OFF, `01..=N`=specific Hz points) — modeled as unsigned
  `Range` over the raw contiguous wire integers (same convention already
  established for `*LCUT`/`*HCUT FREQ` items: a plain `00..=max` integer
  range, not an exhaustive `Enumerated` list of named Hz labels — the
  display-Hz meaning of each integer is not itself stored anywhere).
  LEVEL items are signed `-20..=+10` (`(P2 = -20 ~ -00 or +00 ~ +10)`,
  same sign-then-magnitude shape as 035/039/064/065/112). BWTH items are
  unsigned `01..=10`. No new judgment call needed beyond the FREQ-item
  convention already established — this is the largest single
  contiguous, uniform block of items landed in one sub-batch so far, and
  it needed zero new `ExMenuValueKind` capability.

- **`EX_SET_FORMS` needed zero changes**: this sub-batch's digit widths
  (1, 2, 3, 4, 5, 8) were all already present — the eighth-digit item
  (151 "PRESET FREQUENCY") confirms the first sub-batch's "sole 8-digit
  outlier" citation was correct; no second 8-digit item exists anywhere
  in the table.

- **Two pre-existing tests needed updating, not left stale**, same
  category of necessary churn as the third sub-batch's own note:
  `ex_menu_item_returns_none_for_any_unlanded_p1` used 80 and 100 as
  example "real but not yet landed" menu numbers — both are landed by
  this task, so the examples were reduced to just 27 and 87 (the only two
  permanently-skipped numbers) plus 999 (not a real menu number).
  `framework_ex_out_of_table_p1_fails_cleanly_not_a_panic` used EX080 as
  its concrete "unlanded item" example — swapped to EX027 ("TIME ZONE",
  the only other genuinely unlanded real item, 5-digit shape).

- **`EX_MENU_TABLE`'s entry count grew from 80 to 151** (71 net new rows:
  74 numbers in 080-153 minus 087 skipped minus 2 already-landed by the
  first sub-batch, 108/109). The integrity test was renamed
  (`ex_menu_table_has_exactly_151_entries`, from `...eighty_entries`) and
  extended to assert the full expected `P1` set, not just the count.

- **Nothing left unresolved or deferred to a further sub-batch**: this
  task completes `EX_MENU_TABLE`. The only two menu numbers without a row
  are 027 and 087, both explicitly and permanently skipped as
  unresolvable from this manual (documented in the second/first
  sub-batches respectively) — not oversights, not left for "next time."
