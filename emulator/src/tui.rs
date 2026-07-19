// Copyright 2026 Matt Franklin
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// Rewritten against `Ft991aState`'s (first-slice) field set — this is NOT a
// port of `ts570d/emulator/src/tui.rs`'s per-field content. The low-level
// visual-language helpers (`bargraph`, `tick_label_line`, `big_digit`/
// `render_big_freq`, `on_style`) and the overall three-column layout
// convention ARE carried over, since those are generic rendering helpers,
// not TS-570D-specific. See `planning/emulator/task_plan.md` §7.3/Findings
// for the design rationale and judgment calls below.
//
// Wave 4 (`planning/architect/task_plan.md` §11.5): proportional growth of
// this screen alongside `Ft991aState`'s Wave 3 field growth (11 -> 91
// commands, first-slice -> full state machine). Per §11.5's explicit shape
// recommendation, this stays a flat, wider annunciator list (now wrapped
// across 3 lines instead of 1) rather than becoming a grouped/paginated
// display — the emulator has no keybindings, so the discoverability
// pressure that motivated `ui`'s grouped-menu redesign doesn't apply here.
// See `planning/emulator/task_plan.md`'s Wave 4 section for the full
// per-region mapping from architect field list -> real `Ft991aState` fields.

use radio::ft991a_radio::Ft991aState as RadioState;
use radio::{AgcMode, Mode, PreampMode, ScanState, FT991A_COMMAND_TABLE};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Look up the human-readable description for a 2-character CAT command code.
fn lookup_description(code: &str) -> Option<&'static str> {
    FT991A_COMMAND_TABLE.find(code).map(|cmd| cmd.description)
}

// ─── 5-row block-character digit font ────────────────────────────────────────

fn big_digit(d: char) -> [&'static str; 5] {
    match d {
        '0' => ["▄███▄", "█   █", "█   █", "█   █", "▀███▀"],
        '1' => ["  ▄█ ", "  ██ ", "   █ ", "   █ ", "  ███"],
        '2' => ["▄███▄", "    █", "▄███▀", "█    ", "█████"],
        '3' => ["▄███▄", "    █", " ███▄", "    █", "▀███▀"],
        '4' => ["█   █", "█   █", "▀████", "    █", "    █"],
        '5' => ["█████", "█    ", "▀███▄", "    █", "▀███▀"],
        '6' => ["▄███▄", "█    ", "████▄", "█   █", "▀███▀"],
        '7' => ["█████", "    █", "   █ ", "  █  ", "  █  "],
        '8' => ["▄███▄", "█   █", "▄███▄", "█   █", "▀███▀"],
        '9' => ["▄███▄", "█   █", "▀████", "    █", "▀███▀"],
        '.' => ["     ", "     ", "     ", "  ▄  ", "  █  "],
        _ => ["     ", "     ", "     ", "     ", "     "],
    }
}

/// Render a frequency string as 5 lines of block-character glyphs.
/// Returns an array of 5 strings, one per row.
fn render_big_freq(freq_str: &str) -> [String; 5] {
    // Collect glyph rows for each character.
    let glyphs: Vec<[&'static str; 5]> = freq_str.chars().map(big_digit).collect();

    let mut rows: [String; 5] = Default::default();
    for row in 0..5 {
        let mut line = String::new();
        for (i, glyph) in glyphs.iter().enumerate() {
            if i > 0 {
                line.push(' ');
            }
            line.push_str(glyph[row]);
        }
        rows[row] = line;
    }
    rows
}

/// Format a raw Hz value in FT-991A display format: `030.000.00` (3-digit
/// MHz, since the FT-991A's range runs up to 470,000,000 Hz — ts570d's
/// 2-digit-MHz format caps out at its 60 MHz range and isn't reusable here).
fn format_freq_ascii(freq_hz: u64) -> String {
    let mhz = freq_hz / 1_000_000;
    let khz = (freq_hz % 1_000_000) / 1_000;
    let ten_hz = (freq_hz % 1_000) / 10;
    format!("{:>3}.{:03}.{:02}", mhz, khz, ten_hz)
}

/// Bright amber / yellow style used for ON annunciators.
fn on_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

/// Build the bargraph string from a 0.0–1.0 fill ratio and a total character width.
pub fn bargraph(ratio: f64, width: usize) -> String {
    const BLOCKS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if width == 0 {
        return String::new();
    }
    let total_eighths = (ratio.clamp(0.0, 1.0) * (width * 8) as f64).round() as usize;
    let full_blocks = total_eighths / 8;
    let remainder = total_eighths % 8;
    let mut s = String::with_capacity(width);
    for _ in 0..full_blocks.min(width) {
        s.push('█');
    }
    if full_blocks < width && remainder > 0 {
        s.push(BLOCKS[remainder - 1]);
        for _ in (full_blocks + 1)..width {
            s.push(' ');
        }
    } else {
        for _ in full_blocks..width {
            s.push(' ');
        }
    }
    s
}

// ─────────────────────────────────────────────────────────────────────────────
//  Public entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Draw the full three-column layout into `f`.
///
/// - Col 1 (~22 chars):  Meter column — S-meter (RX) or PWR meter (TX)
/// - Col 2 (remaining):  Main LCD — annunciators, frequency, mode
/// - Col 3 (~45% total): Command/status panel — port, command log, controls
pub fn draw(f: &mut Frame, state: &RadioState, port: &str, log: &[String]) {
    let area = f.size();

    // Outer border titled "YAESU FT-991A" in amber.
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .title(" YAESU FT-991A ")
        .border_style(Style::default().fg(Color::Yellow));
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    // ── Four-column split (meter | spacer | LCD | command) ─────────────────
    let total_w = inner.width;
    let meter_w: u16 = 22;
    let spacer: u16 = 1;
    let remaining = total_w.saturating_sub(meter_w).saturating_sub(spacer);
    let cmd_w: u16 = ((total_w as u32 * 45 / 100) as u16).min(remaining);
    let lcd_w: u16 = remaining.saturating_sub(cmd_w);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(meter_w), // meters
            Constraint::Length(spacer),  // spacer
            Constraint::Length(lcd_w),   // LCD
            Constraint::Length(cmd_w),   // command panel
        ])
        .split(inner);

    let meter_area = cols[0];
    // cols[1] is the spacer — render nothing there
    let lcd_area = cols[2];
    let cmd_area = cols[3];

    draw_meter_col(f, meter_area, state);
    draw_lcd_main(f, lcd_area, state);
    draw_command_panel(f, cmd_area, port, log);
}

// ─────────────────────────────────────────────────────────────────────────────
//  COL 1 — Meter column
// ─────────────────────────────────────────────────────────────────────────────

fn draw_meter_col(f: &mut Frame, area: Rect, state: &RadioState) {
    if state.cat_tx == 1 {
        draw_tx_meter(f, area, state);
    } else {
        draw_rx_smeter(f, area, state);
    }
}

/// Build a tick-label line of exactly `width` chars.
/// `ticks`: list of (position 0..width, label &str) sorted by position.
/// Labels are placed left-to-right; if a label would overlap the previous one, it is skipped.
fn tick_label_line(width: usize, ticks: &[(usize, &str)]) -> String {
    let mut buf = vec![b' '; width];
    let mut next_free = 0usize;
    for &(pos, label) in ticks {
        let label_bytes = label.as_bytes();
        let start = pos.min(width.saturating_sub(label_bytes.len()));
        if start >= next_free && start + label_bytes.len() <= width {
            buf[start..start + label_bytes.len()].copy_from_slice(label_bytes);
            next_free = start + label_bytes.len() + 1; // +1 gap
        }
    }
    String::from_utf8(buf).unwrap_or_default()
}

/// Map an `SM` raw value (0–255, manual p.17) to a human-readable S-unit
/// label.
///
/// **Not manual-cited**: the FT-991A CAT manual documents `SM`'s wire
/// format (`SM0<3 digits>;`, 000-255) but no raw-value-to-S-unit curve.
/// This is a synthetic linear mapping — 16 raw units per S-unit up to S9
/// (raw 144), then four wider bands covering the remaining range for
/// +10/+20/+30/+40 dB over S9 — chosen only to give the emulator's own TUI
/// a readable label; it has no bearing on wire behavior.
fn smeter_label(v: u8) -> &'static str {
    match v {
        0..=15 => "S1",
        16..=31 => "S2",
        32..=47 => "S3",
        48..=63 => "S4",
        64..=79 => "S5",
        80..=95 => "S6",
        96..=111 => "S7",
        112..=127 => "S8",
        128..=143 => "S9",
        144..=170 => "S9+10",
        171..=197 => "S9+20",
        198..=224 => "S9+30",
        _ => "S9+40",
    }
}

fn draw_rx_smeter(f: &mut Frame, area: Rect, state: &RadioState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title + current value
            Constraint::Length(1), // tick labels
            Constraint::Length(1), // bargraph
            Constraint::Length(1), // blank padding
            Constraint::Min(0),    // filler
        ])
        .split(area);

    let width = area.width as usize;

    // Row 0: title left, current S-unit label right-aligned
    let label = smeter_label(state.smeter);
    let title = "S-METER";
    let pad = width.saturating_sub(title.len() + label.len());
    let title_line = Line::from(vec![
        Span::styled(title, Style::default().fg(Color::DarkGray)),
        Span::raw(" ".repeat(pad)),
        Span::styled(
            label,
            if state.smeter <= 85 {
                Style::default().fg(Color::Green)
            } else if state.smeter <= 170 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            },
        ),
    ]);

    // Row 1: tick mark labels at computed positions, mapped from the raw
    // 0-255 scale onto the meter column's character width.
    let ticks: Vec<(usize, &str)> = [
        (16usize, "1"),
        (48, "3"),
        (80, "5"),
        (112, "7"),
        (144, "9"),
        (200, "+20"),
    ]
    .iter()
    .map(|&(v, lbl)| (v * width / 255, lbl))
    .collect();
    let tick_str = tick_label_line(width, &ticks);

    // Row 2: bargraph, color based on fill ratio
    let ratio = state.smeter as f64 / 255.0;
    let bar_color = if ratio <= 0.5 {
        Color::Green
    } else if ratio <= 0.75 {
        Color::Yellow
    } else {
        Color::Red
    };
    let bar_str = bargraph(ratio, width.max(1));
    let bar_line = Line::from(Span::styled(bar_str, Style::default().fg(bar_color)));

    if rows[0].height > 0 {
        f.render_widget(Paragraph::new(title_line), rows[0]);
    }
    if rows[1].height > 0 {
        f.render_widget(
            Paragraph::new(Span::styled(tick_str, Style::default().fg(Color::DarkGray))),
            rows[1],
        );
    }
    if rows[2].height > 0 {
        f.render_widget(Paragraph::new(bar_line), rows[2]);
    }
    // rows[3] is blank padding — render nothing
}

/// Map `MS`'s P1 meter-select value (0-5) to its meter name (manual p.12).
fn meter_select_label(v: u8) -> &'static str {
    match v {
        0 => "COMP",
        1 => "ALC",
        2 => "PO",
        3 => "SWR",
        4 => "ID",
        5 => "VDD",
        _ => "?",
    }
}

/// Render one row of the TX-side 6-way meter bank: `LABEL <bar> <value>`.
/// Highlighted (amber/bold) when `selected` — i.e. this is the meter `MS`
/// currently has chosen on the front panel; the other five are dimmed but
/// still shown, since the emulator's job is showing everything at once for
/// whoever is debugging the wire protocol, not just what a real front panel
/// would currently display.
fn draw_meter_row(f: &mut Frame, area: Rect, label: &str, value: u8, selected: bool) {
    if area.height == 0 {
        return;
    }
    let width = area.width as usize;
    let value_str = format!("{value:3}");
    let bar_width = width.saturating_sub(4 + 1 + 1 + value_str.len());
    let bar_str = bargraph(value as f64 / 255.0, bar_width.max(1));
    let (text_style, bar_color) = if selected {
        (
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            Color::Yellow,
        )
    } else {
        (Style::default().fg(Color::DarkGray), Color::DarkGray)
    };
    let line = Line::from(vec![
        Span::styled(format!("{label:<4}"), text_style),
        Span::raw(" "),
        Span::styled(bar_str, Style::default().fg(bar_color)),
        Span::raw(" "),
        Span::styled(value_str, text_style),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

/// TX-side meter column: `PWR: <power_control>W` plus the full 6-way
/// `MS`-selected meter bank (COMP/ALC/PO/SWR/ID/VDD, batch 9's `RM`/`MS`).
/// `PWR` stays a separate row above the bank — it reads `power_control`
/// (watts, `PC`'s own field) rather than `po_meter` (`RM`'s raw 0-255 PO
/// reading), a distinct value on a distinct scale, so it is not folded into
/// the bank as a seventh row.
fn draw_tx_meter(f: &mut Frame, area: Rect, state: &RadioState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // PWR label + value
            Constraint::Length(1), // PWR bargraph
            Constraint::Length(1), // PWR tick labels
            Constraint::Length(1), // blank padding
            Constraint::Length(1), // 6-way meter bank header (MS-selected)
            Constraint::Length(1), // COMP
            Constraint::Length(1), // ALC
            Constraint::Length(1), // PO
            Constraint::Length(1), // SWR
            Constraint::Length(1), // ID
            Constraint::Length(1), // VDD
            Constraint::Min(0),    // filler
        ])
        .split(area);

    let width = area.width as usize;

    let pwr_ratio = state.power_control as f64 / 100.0;
    let pwr_bar_color = if pwr_ratio <= 0.5 {
        Color::Green
    } else if pwr_ratio <= 0.8 {
        Color::Yellow
    } else {
        Color::Red
    };
    let pwr_value = format!("{}W", state.power_control);
    let pwr_pad = width.saturating_sub("PWR".len() + pwr_value.len());
    let pwr_label_line = Line::from(vec![
        Span::styled("PWR", Style::default().fg(Color::DarkGray)),
        Span::raw(" ".repeat(pwr_pad)),
        Span::styled(
            pwr_value,
            Style::default()
                .fg(pwr_bar_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let pwr_bar_str = bargraph(pwr_ratio, width.max(1));
    let pwr_bar_line = Line::from(Span::styled(
        pwr_bar_str,
        Style::default().fg(pwr_bar_color),
    ));
    let pwr_ticks: &[(usize, &str)] = &[
        (width / 4, "25W"),
        (width / 2, "50W"),
        (width * 3 / 4, "75W"),
        (width.saturating_sub(4), "100W"),
    ];
    let pwr_tick_str = tick_label_line(width, pwr_ticks);

    if rows[0].height > 0 {
        f.render_widget(Paragraph::new(pwr_label_line), rows[0]);
    }
    if rows[1].height > 0 {
        f.render_widget(Paragraph::new(pwr_bar_line), rows[1]);
    }
    if rows[2].height > 0 {
        f.render_widget(
            Paragraph::new(Span::styled(
                pwr_tick_str,
                Style::default().fg(Color::DarkGray),
            )),
            rows[2],
        );
    }
    // rows[3]: blank padding — render nothing

    if rows[4].height > 0 {
        let header = format!("6-WAY  MS={}", meter_select_label(state.meter_select));
        f.render_widget(
            Paragraph::new(Span::styled(header, Style::default().fg(Color::DarkGray))),
            rows[4],
        );
    }
    draw_meter_row(
        f,
        rows[5],
        "COMP",
        state.comp_meter,
        state.meter_select == 0,
    );
    draw_meter_row(f, rows[6], "ALC", state.alc_meter, state.meter_select == 1);
    draw_meter_row(f, rows[7], "PO", state.po_meter, state.meter_select == 2);
    draw_meter_row(f, rows[8], "SWR", state.swr_meter, state.meter_select == 3);
    draw_meter_row(f, rows[9], "ID", state.id_meter, state.meter_select == 4);
    draw_meter_row(f, rows[10], "VDD", state.vdd_meter, state.meter_select == 5);
    // rows[11]: filler — render nothing
}

// ─────────────────────────────────────────────────────────────────────────────
//  COL 2 — Main LCD: annunciators + freq + mode
// ─────────────────────────────────────────────────────────────────────────────

fn draw_lcd_main(f: &mut Frame, area: Rect, state: &RadioState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Ann line 1 (active items only, may be empty)
            Constraint::Length(1), // Ann line 2 (active items only, may be empty)
            Constraint::Length(1), // Ann line 3 (active items + always-on AGC label)
            Constraint::Length(1), // Secondary VFO B readout (dim, always shown)
            Constraint::Length(5), // Large frequency display (VFO A, 5-row block glyphs)
            Constraint::Length(1), // Mode row
            Constraint::Min(0),    // remaining padding
        ])
        .split(area);

    draw_ann_line1(f, rows[0], state);
    draw_ann_line2(f, rows[1], state);
    draw_ann_line3(f, rows[2], state);
    draw_vfo_b_line(f, rows[3], state);
    draw_freq_block(f, rows[4], state);
    draw_mode_row(f, rows[5], state);
}

/// Build a space-joined string of only the active labels from a list.
fn active_ann_str(items: &[(&str, bool)]) -> String {
    items
        .iter()
        .filter_map(|&(label, active)| if active { Some(label) } else { None })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `PA`'s pre-amp/IPO annunciator — `None` when `Ipo` (bypass; a real front
/// panel lights neither `AMP1` nor `AMP2` in that state, so nothing is
/// shown, same "off means hidden" idiom as the plain-bool annunciators).
fn preamp_ann_label(raw: u8) -> Option<&'static str> {
    match PreampMode::try_from(raw) {
        Ok(PreampMode::Amp1) => Some("AMP1"),
        Ok(PreampMode::Amp2) => Some("AMP2"),
        _ => None,
    }
}

/// `SC`'s scan-direction annunciator — `None` when scan is off.
fn scan_ann_label(raw: u8) -> Option<&'static str> {
    match ScanState::try_from(raw) {
        Ok(ScanState::Up) => Some("SCAN\u{25b2}"),
        Ok(ScanState::Down) => Some("SCAN\u{25bc}"),
        _ => None,
    }
}

/// `GT`'s AGC-mode label (manual p.10; see the module docs' "GT, AGC's
/// write/report domain mismatch" section for the 7-valued `P3` domain this
/// reads). Unlike the other annunciators here, AGC always has *some* active
/// setting (`OFF` is itself a real, meaningful state to show on a debugging
/// screen) — so line 3 always shows this label rather than hiding it when
/// "off", the same always-visible idiom `draw_mode_row` already uses for
/// the operating mode.
fn agc_mode_label(raw: u8) -> &'static str {
    match AgcMode::try_from(raw) {
        Ok(AgcMode::Off) => "OFF",
        Ok(AgcMode::Fast) => "FAST",
        Ok(AgcMode::Mid) => "MID",
        Ok(AgcMode::Slow) => "SLOW",
        Ok(AgcMode::AutoFast) => "AUTO-F",
        Ok(AgcMode::AutoMid) => "AUTO-M",
        Ok(AgcMode::AutoSlow) => "AUTO-S",
        Err(_) => "?",
    }
}

/// Annunciator line 1 — core operating state: `power_on`/`cat_tx` (as
/// before), plus `mox_on`/`lock_on`/`menu_mode`/`pll_unlocked` (`IF`/`RS`/
/// `UL`, batch 9/10).
fn draw_ann_line1(f: &mut Frame, area: Rect, state: &RadioState) {
    let items: &[(&str, bool)] = &[
        ("PWR", state.power_on),
        ("TX", state.cat_tx == 1),
        ("RX", state.cat_tx == 0),
        ("MOX", state.mox_on),
        ("LOCK", state.lock_on),
        ("MENU", state.menu_mode),
        ("PLL-UNLK", state.pll_unlocked),
    ];
    let text = active_ann_str(items);
    if !text.is_empty() {
        f.render_widget(Paragraph::new(Span::styled(text, on_style())), area);
    }
}

/// Annunciator line 2 — front-end/audio processing toggles: clarifier
/// RX/TX-on (`IF`'s P4/P5, written by `RT`/`XT`), attenuator (`RA`), preamp
/// (`PA`), noise blanker/reduction (`NB`/`NR`), auto notch (`BC`), narrow
/// (`NA`).
fn draw_ann_line2(f: &mut Frame, area: Rect, state: &RadioState) {
    let mut labels: Vec<&str> = Vec::new();
    if state.rx_clarifier_on {
        labels.push("CLAR-R");
    }
    if state.tx_clarifier_on {
        labels.push("CLAR-T");
    }
    if state.attenuator_on {
        labels.push("ATT");
    }
    if let Some(l) = preamp_ann_label(state.preamp_mode) {
        labels.push(l);
    }
    if state.noise_blanker_on {
        labels.push("NB");
    }
    if state.noise_reduction_on {
        labels.push("NR");
    }
    if state.auto_notch_on {
        labels.push("NOTCH");
    }
    if state.narrow_on {
        labels.push("NAR");
    }
    let text = labels.join(" ");
    if !text.is_empty() {
        f.render_widget(Paragraph::new(Span::styled(text, on_style())), area);
    }
}

/// Annunciator line 3 — keying/scan/AGC: keyer enabled (`KR`), scan
/// direction (`SC`), VOX on (`VX`), break-in on (`BI`), plus the always-on
/// AGC mode label (`GT`, see [`agc_mode_label`]'s doc comment for why it is
/// not gated behind an "active" check like the rest of this line).
fn draw_ann_line3(f: &mut Frame, area: Rect, state: &RadioState) {
    let mut labels: Vec<&str> = Vec::new();
    if state.keyer_on {
        labels.push("KYR");
    }
    if let Some(l) = scan_ann_label(state.scan_state) {
        labels.push(l);
    }
    if state.vox_on {
        labels.push("VOX");
    }
    if state.break_in_on {
        labels.push("B-IN");
    }
    let agc = format!("AGC:{}", agc_mode_label(state.agc_mode));
    let text = if labels.is_empty() {
        agc
    } else {
        format!("{}  {agc}", labels.join(" "))
    };
    f.render_widget(Paragraph::new(Span::styled(text, on_style())), area);
}

/// Secondary, dim readout of VFO B's frequency. `Ft991aState` has no
/// "active VFO" selector field in this slice, so there is no toggle badge
/// (unlike ts570d's `◄A►`/`◄B►`) — VFO A is always the primary big-digit
/// display and VFO B is always shown here as a plain secondary line.
fn draw_vfo_b_line(f: &mut Frame, area: Rect, state: &RadioState) {
    let text = format!("VFO-B  {}", format_freq_ascii(state.vfo_b_hz));
    f.render_widget(
        Paragraph::new(Span::styled(text, Style::default().fg(Color::DarkGray))),
        area,
    );
}

// ─── 5-row large frequency display (VFO A) ───────────────────────────────────

/// Clarifier offset readout shown next to the big-digit VFO A frequency
/// (`IF`'s P3, written by batch 3's `RD`/`RU`, cleared by `RC`; shared by RX
/// and TX per `clarifier_offset_hz`'s own doc comment — no separate
/// per-direction offset value exists). `None` when neither `rx_clarifier_on`
/// nor `tx_clarifier_on` is set, so the readout is hidden exactly when a
/// real front panel would hide it. `Ft991aState` has no "active VFO"
/// selector (see this file's header doc comment / `RadioState`'s Wave 2
/// finding), so unlike ts570d's RIT/XIT sub-display next to a VFO A/B
/// badge, this has no badge to sit beside — it is simply appended to the
/// big-digit block's middle row.
fn clarifier_readout(state: &RadioState) -> Option<String> {
    if !state.rx_clarifier_on && !state.tx_clarifier_on {
        return None;
    }
    let dir = match (state.rx_clarifier_on, state.tx_clarifier_on) {
        (true, true) => "R/T",
        (true, false) => "RX",
        (false, true) => "TX",
        (false, false) => unreachable!("guarded by the early return above"),
    };
    Some(format!("CLAR {dir} {:+05}Hz", state.clarifier_offset_hz))
}

fn draw_freq_block(f: &mut Frame, area: Rect, state: &RadioState) {
    if area.height < 5 {
        return;
    }

    let freq_str = format_freq_ascii(state.vfo_a_hz);
    let rows = render_big_freq(&freq_str);
    let clarifier = clarifier_readout(state);

    let row_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    for (i, row_text) in rows.iter().enumerate() {
        let row_area = row_areas[i];
        let mut spans = vec![
            Span::raw("    "),
            Span::styled(row_text.clone(), on_style()),
        ];
        // Middle row: append the clarifier readout, if active, after the
        // big-digit glyphs.
        if i == 2 {
            if let Some(ref c) = clarifier {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    c.clone(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ));
            }
        }
        f.render_widget(Paragraph::new(Line::from(spans)), row_area);
    }
}

// ─── Mode row ─────────────────────────────────────────────────────────────
// FT-991A's hex-nibble `Mode` (`radio::Mode::name()`, 14 values) replaces
// ts570d's 1-9 `match`.

fn draw_mode_row(f: &mut Frame, area: Rect, state: &RadioState) {
    let mode_label = Mode::try_from(state.mode).map(Mode::name).unwrap_or("---");

    let spans = vec![Span::styled(mode_label, on_style())];
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ─────────────────────────────────────────────────────────────────────────────
//  COL 3 — Command/status panel
// ─────────────────────────────────────────────────────────────────────────────

fn draw_command_panel(f: &mut Frame, area: Rect, port: &str, log: &[String]) {
    let panel_block = Block::default()
        .borders(Borders::ALL)
        .title(" Commands ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = panel_block.inner(area);
    f.render_widget(panel_block, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // PORT line
            Constraint::Min(1),    // scrolling command log
            Constraint::Length(1), // controls
        ])
        .split(inner);

    // PORT line.
    let port_line = Line::from(vec![
        Span::styled("PORT: ", Style::default().fg(Color::Cyan)),
        Span::styled(
            port,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    f.render_widget(Paragraph::new(port_line), sections[0]);

    // COMMANDS section — show last N lines that fit.
    let log_area = sections[1];
    let max_lines = log_area.height as usize;

    let visible: Vec<Line> = if log.len() > max_lines {
        log[log.len() - max_lines..]
            .iter()
            .map(|s| format_log_line(s))
            .collect()
    } else {
        log.iter().map(|s| format_log_line(s)).collect()
    };

    f.render_widget(Paragraph::new(visible).wrap(Wrap { trim: false }), log_area);

    // CONTROLS line.
    let controls_line = Line::from(vec![Span::styled(
        "[q] quit",
        Style::default().fg(Color::DarkGray),
    )]);
    f.render_widget(Paragraph::new(controls_line), sections[2]);
}

/// Extract a 2-character CAT command code from a log entry string.
///
/// Log entries have the form `"→ FA;"` or `"← FA014000000;"`.
/// The arrow character is a multi-byte UTF-8 sequence, so we skip to the
/// first ASCII alphabetic character after the prefix.
fn extract_command_code(s: &str) -> Option<&str> {
    // Find the first ASCII letter — command codes always start there.
    let start = s
        .char_indices()
        .find(|(_, c)| c.is_ascii_alphabetic())
        .map(|(i, _)| i)?;
    let rest = &s[start..];
    if rest.len() >= 2 && rest.as_bytes()[..2].iter().all(|b| b.is_ascii_alphabetic()) {
        Some(&rest[..2])
    } else {
        None
    }
}

/// Style a single log entry line with an optional gray description comment.
/// Lines starting with "→" are incoming (yellow), "←" are responses (green).
fn format_log_line(s: &str) -> Line<'static> {
    let owned = s.to_owned();
    let code = extract_command_code(&owned).map(str::to_ascii_uppercase);
    let description = code
        .as_deref()
        .and_then(lookup_description)
        .map(|d| format!("  // {d}"));

    if owned.starts_with('→') {
        let mut spans = vec![Span::styled(owned, Style::default().fg(Color::Yellow))];
        if let Some(desc) = description {
            spans.push(Span::styled(desc, Style::default().fg(Color::DarkGray)));
        }
        Line::from(spans)
    } else if owned.starts_with('←') {
        let mut spans = vec![Span::styled(owned, Style::default().fg(Color::Green))];
        if let Some(desc) = description {
            spans.push(Span::styled(desc, Style::default().fg(Color::DarkGray)));
        }
        Line::from(spans)
    } else {
        Line::from(Span::raw(owned))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_freq_ascii_full_range() {
        // MHz is space-padded to 3 chars (matching ts570d's convention of
        // space- not zero-padding the leading MHz field).
        assert_eq!(format_freq_ascii(14_000_000), " 14.000.00");
        assert_eq!(format_freq_ascii(470_000_000), "470.000.00");
        assert_eq!(format_freq_ascii(30_000), "  0.030.00");
    }

    #[test]
    fn test_smeter_label_boundaries() {
        assert_eq!(smeter_label(0), "S1");
        assert_eq!(smeter_label(143), "S9");
        assert_eq!(smeter_label(144), "S9+10");
        assert_eq!(smeter_label(255), "S9+40");
    }

    #[test]
    fn test_lookup_description_known_and_unknown() {
        assert_eq!(lookup_description("FA"), Some("VFO A Frequency"));
        assert_eq!(lookup_description("ZZ"), None);
        // Not just the original 11-command first slice — Wave 3 landed the
        // remaining batches, so a batch-6 (`GT`, AGC) and the `EX` menu
        // entry point must also resolve now that `lookup_description`
        // reads the full `FT991A_COMMAND_TABLE`.
        assert_eq!(lookup_description("GT"), Some("AGC Function"));
        assert!(lookup_description("EX").is_some());
    }

    #[test]
    fn test_command_table_fully_wired_wave4() {
        // Regression guard for the Wave 4 §11.5 retarget: `tui.rs`'s
        // `lookup_description` is expected to cover the now-full 91-entry
        // table, not a stale first-slice subset. This doesn't hardcode 91
        // (that number belongs to `radio`'s own tests) — it just asserts
        // `tui.rs` sees more than the original Wave 2 slice of 11.
        assert!(FT991A_COMMAND_TABLE.definitions().len() > 11);
    }

    #[test]
    fn test_extract_command_code() {
        assert_eq!(extract_command_code("→ FA;"), Some("FA"));
        assert_eq!(extract_command_code("← FA014000000;"), Some("FA"));
        assert_eq!(extract_command_code(""), None);
    }

    #[test]
    fn test_meter_select_label() {
        assert_eq!(meter_select_label(0), "COMP");
        assert_eq!(meter_select_label(3), "SWR");
        assert_eq!(meter_select_label(5), "VDD");
        assert_eq!(meter_select_label(9), "?");
    }

    #[test]
    fn test_agc_mode_label_full_7valued_domain() {
        assert_eq!(agc_mode_label(0), "OFF");
        assert_eq!(agc_mode_label(1), "FAST");
        assert_eq!(agc_mode_label(2), "MID");
        assert_eq!(agc_mode_label(3), "SLOW");
        assert_eq!(agc_mode_label(4), "AUTO-F");
        assert_eq!(agc_mode_label(5), "AUTO-M");
        assert_eq!(agc_mode_label(6), "AUTO-S");
        assert_eq!(agc_mode_label(9), "?");
    }

    #[test]
    fn test_preamp_ann_label_ipo_hidden() {
        assert_eq!(preamp_ann_label(0), None); // IPO — no annunciator lit
        assert_eq!(preamp_ann_label(1), Some("AMP1"));
        assert_eq!(preamp_ann_label(2), Some("AMP2"));
    }

    #[test]
    fn test_scan_ann_label_off_hidden() {
        assert_eq!(scan_ann_label(0), None); // scan off — no annunciator lit
        assert_eq!(scan_ann_label(1), Some("SCAN\u{25b2}"));
        assert_eq!(scan_ann_label(2), Some("SCAN\u{25bc}"));
    }

    #[test]
    fn test_clarifier_readout_hidden_when_both_off() {
        let state = RadioState::default();
        assert_eq!(clarifier_readout(&state), None);
    }

    #[test]
    fn test_clarifier_readout_shows_direction_and_signed_offset() {
        let mut state = RadioState {
            rx_clarifier_on: true,
            clarifier_offset_hz: 250,
            ..Default::default()
        };
        assert_eq!(
            clarifier_readout(&state),
            Some("CLAR RX +0250Hz".to_string())
        );

        state.tx_clarifier_on = true;
        assert_eq!(
            clarifier_readout(&state),
            Some("CLAR R/T +0250Hz".to_string())
        );

        state.rx_clarifier_on = false;
        state.clarifier_offset_hz = -9999;
        assert_eq!(
            clarifier_readout(&state),
            Some("CLAR TX -9999Hz".to_string())
        );
    }
}
