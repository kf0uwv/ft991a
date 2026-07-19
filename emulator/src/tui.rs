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

use radio::ft991a_radio::Ft991aState as RadioState;
use radio::{Mode, FT991A_COMMAND_TABLE};
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

/// TX-side meter column: `PWR: <power_control>W` only. No SWR bar — this
/// first-slice command table has no `RM` meter-read command to back an SWR
/// reading (unlike ts570d), so no SWR value is fabricated.
fn draw_tx_meter(f: &mut Frame, area: Rect, state: &RadioState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // PWR label + value
            Constraint::Length(1), // PWR bargraph
            Constraint::Length(1), // PWR tick labels
            Constraint::Length(1), // blank padding
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
}

// ─────────────────────────────────────────────────────────────────────────────
//  COL 2 — Main LCD: annunciators + freq + mode
// ─────────────────────────────────────────────────────────────────────────────

fn draw_lcd_main(f: &mut Frame, area: Rect, state: &RadioState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Ann line (active items only, may be empty)
            Constraint::Length(1), // Secondary VFO B readout (dim, always shown)
            Constraint::Length(5), // Large frequency display (VFO A, 5-row block glyphs)
            Constraint::Length(1), // Mode row
            Constraint::Min(0),    // remaining padding
        ])
        .split(area);

    draw_ann_line(f, rows[0], state);
    draw_vfo_b_line(f, rows[1], state);
    draw_freq_block(f, rows[2], state);
    draw_mode_row(f, rows[3], state);
}

/// Build a space-joined string of only the active labels from a list.
fn active_ann_str(items: &[(&str, bool)]) -> String {
    items
        .iter()
        .filter_map(|&(label, active)| if active { Some(label) } else { None })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Annunciator line — reduced to exactly what `Ft991aState` has this slice:
/// `power_on` and `cat_tx`. No RIT/XIT/split/antenna/AGC/noise-blanker/etc.
/// annunciators — none of those fields exist on `Ft991aState`.
fn draw_ann_line(f: &mut Frame, area: Rect, state: &RadioState) {
    let items: &[(&str, bool)] = &[
        ("PWR", state.power_on),
        ("TX", state.cat_tx == 1),
        ("RX", state.cat_tx == 0),
    ];
    let text = active_ann_str(items);
    if !text.is_empty() {
        f.render_widget(Paragraph::new(Span::styled(text, on_style())), area);
    }
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

fn draw_freq_block(f: &mut Frame, area: Rect, state: &RadioState) {
    if area.height < 5 {
        return;
    }

    let freq_str = format_freq_ascii(state.vfo_a_hz);
    let rows = render_big_freq(&freq_str);

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
        let spans = vec![
            Span::raw("    "),
            Span::styled(row_text.clone(), on_style()),
        ];
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
    }

    #[test]
    fn test_extract_command_code() {
        assert_eq!(extract_command_code("→ FA;"), Some("FA"));
        assert_eq!(extract_command_code("← FA014000000;"), Some("FA"));
        assert_eq!(extract_command_code(""), None);
    }
}
