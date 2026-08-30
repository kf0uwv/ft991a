# Renderer parity

Required by radio-cat-rs ADR 0013. Parity is on **capabilities**, not on
pixels: every capability this console offers should be reachable in both
renderers, and each place it is not gets a row here naming the ground.

The three grounds ADR 0013 allows are:

- **(a) fidelity** — the renderers can't represent the thing equally well
- **(b) gesture** — the interaction has no sensible counterpart
- **(c) in progress** — not built yet, with a tracking item

Development cost is explicitly **not** a ground.

## Capability parity: TUI vs GUI

**The GUI does not exist yet.** Every capability below is therefore
TUI-only under ground (c), and this table collapses to a single row rather
than one per feature — enumerating a hundred rows that all say "the GUI has
not been written" would be noise, not a record. It becomes a real table the
moment the first GUI panel ships.

| capability | missing from | ground | tracking |
|---|---|---|---|
| all of them | GUI | (c) | the GUI console, not yet started |

One item is worth naming ahead of that, because it is a **permanent**
exception rather than a pending one:

| capability | missing from | ground | note |
|---|---|---|---|
| waterfall / spectrum | both | (a) | This radio has no IF tap and no bandscope over CAT — `capabilities::FT991A.signal` is `SignalSupport::None`. Neither renderer can show a spectrum, because there is no spectrum. Not a gap to close. |

## Operator-visible changes from the shared-widget migration

Not ADR 0013 exceptions — both renderers would show these — but radio-cat-rs
ADR 0011 rev 4 sets "the operator sees no change" as the bar for migrating
the TUI onto shared widgets, and these are where that bar was knowingly
crossed.

| what changed | before | after | why |
|---|---|---|---|
| S-meter and gain bar resolution | whole cells, `(raw × width) / 255` truncated | eight sub-levels per cell, rounded | the shared bar resolves 160 steps across 20 cells. Strictly finer than the meter reports, so no reading is lost — but the bar moves at raw values where it used to sit still. |
| Error panel ordering | first three errors of the cycle | most recent three | a radio failing in a loop used to pin the panel to its oldest failures and never show the current one. A bug fix that happens to be visible. |
| S-meter bar end caps | inside the bar string | drawn by this crate | no visual change; noted because the caps are now layout (ours) and the 20 cells between them are the shared widget. |

Everything else is byte-identical.

## What this radio deliberately does not publish

An **S-unit table**. `radio::capabilities::FT991A`'s S-meter carries
`s_units: None`, so the readout stays a proportional bar plus the raw
`nnn/255` rather than named S-units.

That is not an omission to fix later. The manual gives no S-unit
breakpoints for the 0-255 scale, and `MeterReading` hands a table straight
to the renderer — so inventing one would be a fabricated claim about
hardware that the console would then display as fact. Contrast `ts570d`,
which publishes a measured table and shows `S9+10`.
