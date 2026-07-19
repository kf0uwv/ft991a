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

//! Render functions for the FT-991A TUI.
//!
//! `split_areas`, `draw_header`, `draw_errors`, `draw_disconnected` are
//! ported near-verbatim from `ts570d/ui/src/layout.rs` — connection-health
//! display is command-count-independent (§6.1). `draw_status` is new
//! (collapsed 2-row body vs. ts570d's 5-row inner status layout, since this
//! slice has no receiver-features/flags row content — §6.3).
//! `draw_control_panel` now renders the grouped-menu skeleton's
//! `Menu`/`GroupMenu` states (§11.2, Wave 4 Task 2) in addition to the
//! `TextInput`/`ListSelect`/`Feedback` states carried over unchanged from
//! Wave 2 — still no `Diagnostic` arm (§6.1, reaffirmed §11.6 item 2).
//! `draw_status`'s row 1 also gains a small `RTS: ON/OFF` indicator (Wave 4
//! Task 4, §11.3 point 6) next to the existing `TX`/`RX` indicator —
//! reuses `tx_state_label`'s (label, color) rendering pattern via the new
//! `rts_label` helper, no new render function needed. `draw_control_panel`
//! gains one more arm for `ControlState::ExSubGroupMenu` (§11.4 path (a),
//! Wave 4 Task 9 — the wave's final task): `draw_ex_sub_group_menu` is the
//! first genuinely **scrolling** list in this crate (as opposed to
//! `GroupMenu`'s fixed, never-scrolled command column), needed because `EX`
//! sub-groups hold up to 45 items.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use radio::TxState;

use crate::control::{
    ex_theme_items, ex_theme_label, group_command_labels, group_label, menu_group_labels,
    ControlState, ExTheme,
};
use crate::Ft991aDisplay;

/// Format a frequency in Hz as "M.KKK.HHH MHz". Reused from ts570d's
/// `format_hz` unchanged — the FT-991A's wider 30 kHz-470 MHz range still
/// formats correctly (the MHz component is simply `0` for sub-1MHz
/// frequencies, e.g. `"0.030.000 MHz"` for the 30 kHz floor).
fn format_hz(hz: u64) -> String {
    let mhz = hz / 1_000_000;
    let khz = (hz % 1_000_000) / 1_000;
    let hz_rem = hz % 1_000;
    format!("{}.{:03}.{:03} MHz", mhz, khz, hz_rem)
}

/// Build an inline S-meter bargraph string (20 chars wide) from the raw
/// 0-255 `SM` reading (manual p.17). FT-991A-specific: ts570d's S-meter is
/// 0-30 with documented S-unit breakpoints; the FT-991A manual gives no
/// equivalent S-unit table for its 0-255 scale, so this shows a
/// proportional bar plus the raw numeric reading rather than inventing
/// unverified S-unit thresholds.
fn smeter_bar(smeter: u8, width: usize) -> String {
    let filled = (smeter as usize * width / 255).min(width);
    let empty = width - filled;
    let mut s = String::with_capacity(width + 2);
    s.push('▐');
    for _ in 0..filled {
        s.push('█');
    }
    for _ in 0..empty {
        s.push('░');
    }
    s.push('▌');
    s
}

/// Compact inline bargraph, `width` chars wide, fill 0.0-1.0. Ported
/// unchanged from ts570d's `mini_bar`.
fn mini_bar(ratio: f64, width: usize) -> String {
    let filled = ((ratio.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    let empty = width - filled;
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

/// The 3-valued `TxState` -> (label, color) rendering rule (§6.5). `Off`
/// renders green `RX`; `CatKeyed` (this session asserted PTT) renders red
/// `TX`; `RadioKeyedNonCat` (some other cause — front panel, VOX,
/// footswitch) renders a **distinct** yellow `TX (ext)` rather than
/// collapsing to the same red `TX` as CAT-keyed, since conflating the two
/// would hide that this session didn't key it.
fn tx_state_label(state: TxState) -> (&'static str, Color) {
    match state {
        TxState::Off => ("RX", Color::Green),
        TxState::CatKeyed => ("TX", Color::Red),
        TxState::RadioKeyedNonCat => ("TX (ext)", Color::Yellow),
    }
}

/// The [`Ft991aDisplay::rts_asserted`] -> (label, color) rendering rule,
/// mirroring [`tx_state_label`]'s pattern for group 5's real-time RTS
/// CW-keying toggle (§11.3 point 6). Red/bold when asserted (actively
/// keying, same visual weight as `TX`), dim gray when not.
fn rts_label(asserted: bool) -> (&'static str, Color) {
    if asserted {
        ("ON", Color::Red)
    } else {
        ("OFF", Color::DarkGray)
    }
}

// ---------------------------------------------------------------------------
// Top-level layout splitter
// ---------------------------------------------------------------------------

/// Split the full terminal area into (header, status, errors, controls)
/// areas. Ported unchanged from ts570d's `split_areas` — same 4-band
/// vertical split, sized down for the smaller status body (§6.3).
pub fn split_areas(area: Rect) -> (Rect, Rect, Rect, Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(4), // Status (2 content rows + border)
            Constraint::Length(5), // Errors (border + 3 lines)
            Constraint::Min(8),    // Controls
        ])
        .split(area);
    (chunks[0], chunks[1], chunks[2], chunks[3])
}

// ---------------------------------------------------------------------------
// draw_header
// ---------------------------------------------------------------------------

/// Draw the FT-991A title header block. Ported near-verbatim from ts570d's
/// `draw_header`, retitled.
pub fn draw_header(f: &mut Frame, area: Rect) {
    let block = Block::default().borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let title = Paragraph::new(" FT-991A RADIO CONTROL ")
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center);
    f.render_widget(title, inner);
}

// ---------------------------------------------------------------------------
// draw_errors — poll error panel
// ---------------------------------------------------------------------------

/// Draw the poll error panel. Ported unchanged from ts570d's `draw_errors`
/// — connection-health display is command-count-independent (§6.1).
pub fn draw_errors(f: &mut Frame, area: Rect, state: &Ft991aDisplay) {
    let block = Block::default().title(" Errors ").borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if state.poll_errors.is_empty() {
        let no_err = Paragraph::new(Line::from(Span::styled(
            "No errors",
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(no_err, inner);
    } else {
        let lines: Vec<Line> = state
            .poll_errors
            .iter()
            .take(3)
            .map(|e| Line::from(Span::styled(e.as_str(), Style::default().fg(Color::Red))))
            .collect();
        f.render_widget(Paragraph::new(lines), inner);
    }
}

// ---------------------------------------------------------------------------
// draw_disconnected — connection-lost overlay (replaces control panel)
// ---------------------------------------------------------------------------

/// Draw a full-panel overlay when the radio is unreachable or still
/// connecting. Ported unchanged from ts570d's `draw_disconnected`.
pub fn draw_disconnected(f: &mut Frame, area: Rect, errors: &[String], initializing: bool) {
    let lines: Vec<Line> = if initializing {
        vec![
            Line::from(Span::styled(
                "Connecting to radio...",
                Style::default().fg(Color::Yellow),
            )),
            Line::from(""),
            Line::from("Waiting for response. This may take a few seconds."),
            Line::from(""),
            Line::from(Span::styled("[Q] Quit", Style::default().fg(Color::White))),
        ]
    } else {
        let mut v: Vec<Line> = vec![
            Line::from(Span::styled(
                "CONNECTION LOST",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("The radio is not responding."),
            Line::from("Reconnect the cable or restart the radio."),
            Line::from("The UI will recover automatically when contact is restored."),
            Line::from(""),
        ];
        for e in errors.iter().take(8) {
            v.push(Line::from(Span::styled(
                e.as_str(),
                Style::default().fg(Color::Yellow),
            )));
        }
        v.push(Line::from(""));
        v.push(Line::from(Span::styled(
            "[Q] Quit",
            Style::default().fg(Color::White),
        )));
        v
    };

    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Radio Status "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

// ---------------------------------------------------------------------------
// draw_status — collapsed 2-row status body (§6.3)
// ---------------------------------------------------------------------------

/// Draw the status panel: VFO A/mode/S-meter/TX-RX on row 1, VFO B plus
/// gain/squelch/power/power-on mini-bars on row 2. Collapsed from ts570d's
/// 5-row inner layout since this slice has no receiver-features/flags row
/// content yet (§6.3). `ID` (fetched once, never changes) is shown small,
/// right-aligned on this row rather than a dedicated row.
pub fn draw_status(f: &mut Frame, area: Rect, state: &Ft991aDisplay) {
    let outer_block = Block::default().title(" Status ").borders(Borders::ALL);
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    // -----------------------------------------------------------------
    // Row 1 — VFO A, mode, S-meter, TX/RX indicator, ID
    // -----------------------------------------------------------------

    let (tx_text, tx_color) = tx_state_label(state.tx_state);
    let (rts_text, rts_color) = rts_label(state.rts_asserted);

    let line1 = Line::from(vec![
        Span::styled("VFO A  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format_hz(state.vfo_a_hz),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{:<8}", state.mode.name()),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw("S "),
        Span::styled(
            smeter_bar(state.smeter, 20),
            Style::default().fg(Color::Green),
        ),
        Span::raw(" "),
        Span::styled(
            format!("{:>3}/255  ", state.smeter),
            Style::default().fg(Color::Green),
        ),
        Span::styled(
            tx_text,
            Style::default().fg(tx_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("RTS:", Style::default().fg(Color::DarkGray)),
        Span::styled(
            rts_text,
            Style::default().fg(rts_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("ID:{}", state.id),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    f.render_widget(Paragraph::new(line1), rows[0]);

    // -----------------------------------------------------------------
    // Row 2 — VFO B, AF/RF/SQL/PWR/PS mini-bar row
    // -----------------------------------------------------------------

    let label_style = Style::default().fg(Color::DarkGray);
    let value_style = Style::default().fg(Color::White);
    let bracket_style = Style::default().fg(Color::DarkGray);
    let filled_style = Style::default().fg(Color::Yellow);
    let empty_style = Style::default().fg(Color::DarkGray);

    let af_bar = mini_bar(state.af_gain as f64 / 255.0, 10);
    let rf_bar = mini_bar(state.rf_gain as f64 / 255.0, 10);
    let af_filled: String = af_bar.chars().filter(|&c| c == '█').collect();
    let af_empty: String = af_bar.chars().filter(|&c| c == '░').collect();
    let rf_filled: String = rf_bar.chars().filter(|&c| c == '█').collect();
    let rf_empty: String = rf_bar.chars().filter(|&c| c == '░').collect();

    let ps_style = if state.power_on {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let ps_text = if state.power_on { "ON" } else { "OFF" };

    let line2 = Line::from(vec![
        Span::styled("VFO B  ", Style::default().fg(Color::DarkGray)),
        Span::styled(format_hz(state.vfo_b_hz), Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled("AF:", label_style),
        Span::styled("[", bracket_style),
        Span::styled(af_filled, filled_style),
        Span::styled(af_empty, empty_style),
        Span::styled("]", bracket_style),
        Span::raw("  "),
        Span::styled("RF:", label_style),
        Span::styled("[", bracket_style),
        Span::styled(rf_filled, filled_style),
        Span::styled(rf_empty, empty_style),
        Span::styled("]", bracket_style),
        Span::raw("  "),
        Span::styled("SQL:", label_style),
        Span::styled(format!("{:>3}", state.squelch), value_style),
        Span::raw("  "),
        Span::styled("PWR:", label_style),
        Span::styled(format!("{:>3}W", state.power_watts), value_style),
        Span::raw("  "),
        Span::styled("PS:", label_style),
        Span::styled(ps_text, ps_style),
    ]);

    f.render_widget(Paragraph::new(line2), rows[1]);
}

// ---------------------------------------------------------------------------
// draw_control_panel
// ---------------------------------------------------------------------------

/// Build a column of menu lines from `(key, label)` pairs. Ported from
/// ts570d's `build_menu_column`.
fn build_menu_column(items: &[(char, &'static str)]) -> Vec<Line<'static>> {
    let key_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    items
        .iter()
        .map(|(key, label)| {
            Line::from(vec![
                Span::styled(format!("[{}]", key), key_style),
                Span::raw(format!(" {}", label)),
            ])
        })
        .collect()
}

/// Draw the interactive control panel: the top-level `Menu` group list, a
/// `GroupMenu`'s own command list (or a "no commands yet" placeholder for
/// groups not yet populated — §11.2, Wave 4 Task 2), or the `TextInput`/
/// `ListSelect`/`Feedback` 3-line layout carried over unchanged from Wave 2
/// (§6.3).
pub fn draw_control_panel(f: &mut Frame, area: Rect, state: &ControlState) {
    let outer_block = Block::default().title(" Controls ").borders(Borders::ALL);
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    match state {
        ControlState::Menu => {
            let mut items = menu_group_labels();
            items.push(('Q', "Quit"));
            f.render_widget(Paragraph::new(build_menu_column(&items)), inner);
        }

        ControlState::GroupMenu { group, .. } => {
            let labels = group_command_labels(*group);
            let mut lines: Vec<Line> = if labels.is_empty() {
                vec![Line::from(Span::styled(
                    format!("{} — no commands yet", group_label(*group)),
                    Style::default().fg(Color::DarkGray),
                ))]
            } else {
                build_menu_column(&labels)
            };
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "[Esc] Back",
                Style::default().fg(Color::DarkGray),
            )));
            f.render_widget(Paragraph::new(lines), inner);
        }

        // `EX` themed sub-group browsing (§11.4 path (a), Wave 4 Task 9).
        // Unlike `GroupMenu`'s fixed, unscrolled command column, sub-groups
        // can hold up to 45 items — genuinely scrolled around `cursor`, not
        // just listed, since a typical terminal's control-panel area can't
        // show that many rows at once.
        ControlState::ExSubGroupMenu { theme, cursor } => {
            draw_ex_sub_group_menu(f, inner, *theme, *cursor);
        }

        // For input/selection/feedback states, use the same 3-line layout
        // ts570d uses (state-shape-driven, not group-count-driven — reused
        // verbatim per §6.3). `ExNumberEntry` (§11.4, path (b), Wave 4 Task
        // 8) joins this group too — per the architect's own design, it
        // "reuses the `TextInput` rendering shell" rather than getting a
        // distinct visual treatment.
        ControlState::TextInput { .. }
        | ControlState::ListSelect { .. }
        | ControlState::Feedback { .. }
        | ControlState::ExNumberEntry { .. } => {
            let lines = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // Line 1: hints / prompt
                    Constraint::Length(1), // Line 2: error / blank
                    Constraint::Min(1),    // Line 3: input / cursor
                ])
                .split(inner);

            match state {
                ControlState::ExNumberEntry { buffer, error } => {
                    f.render_widget(Paragraph::new("EX menu item number (001-153):"), lines[0]);
                    if let Some(err) = error {
                        let err_line = Line::from(vec![Span::styled(
                            format!("⚠ {}", err),
                            Style::default().fg(Color::Red),
                        )]);
                        f.render_widget(Paragraph::new(err_line), lines[1]);
                    }
                    let input_line = Line::from(vec![
                        Span::raw("> "),
                        Span::raw(buffer.as_str()),
                        Span::styled("_", Style::default().fg(Color::Yellow)),
                    ]);
                    f.render_widget(Paragraph::new(input_line), lines[2]);
                }

                ControlState::TextInput {
                    prompt,
                    buffer,
                    error,
                    ..
                } => {
                    f.render_widget(Paragraph::new(prompt.as_str()), lines[0]);
                    if let Some(err) = error {
                        let err_line = Line::from(vec![Span::styled(
                            format!("⚠ {}", err),
                            Style::default().fg(Color::Red),
                        )]);
                        f.render_widget(Paragraph::new(err_line), lines[1]);
                    }
                    let input_line = Line::from(vec![
                        Span::raw("> "),
                        Span::raw(buffer.as_str()),
                        Span::styled("_", Style::default().fg(Color::Yellow)),
                    ]);
                    f.render_widget(Paragraph::new(input_line), lines[2]);
                }

                ControlState::ListSelect {
                    options, cursor, ..
                } => {
                    let hint = Line::from("< > to select, Enter to confirm, Esc to cancel");
                    f.render_widget(Paragraph::new(hint), lines[0]);

                    let mut option_spans: Vec<Span> = vec![Span::raw("> ")];
                    for (i, opt) in options.iter().enumerate() {
                        if i == *cursor {
                            option_spans.push(Span::styled(
                                format!("[{}]", opt),
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            ));
                        } else {
                            option_spans.push(Span::raw(format!(" {} ", opt)));
                        }
                        if i + 1 < options.len() {
                            option_spans.push(Span::raw("  "));
                        }
                    }
                    f.render_widget(Paragraph::new(Line::from(option_spans)), lines[2]);
                }

                ControlState::Feedback { message, is_error } => {
                    let msg_style = if *is_error {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default().fg(Color::Green)
                    };
                    f.render_widget(
                        Paragraph::new(Line::from(Span::styled(message.as_str(), msg_style))),
                        lines[1],
                    );
                    f.render_widget(Paragraph::new("Press any key to continue"), lines[2]);
                }

                // Menu, GroupMenu, and ExSubGroupMenu are handled above.
                ControlState::Menu
                | ControlState::GroupMenu { .. }
                | ControlState::ExSubGroupMenu { .. } => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// draw_ex_sub_group_menu — §11.4 path (a), Wave 4 Task 9
// ---------------------------------------------------------------------------

/// Draw one `EX` themed sub-group's scrollable item list
/// (`ControlState::ExSubGroupMenu`).
///
/// Unlike `GroupMenu`'s command column (fixed-size, never scrolled — see
/// `control.rs`'s `ControlState::GroupMenu` doc comment on why its own
/// `cursor` field stays vestigial), this **is** a genuinely scrolling view:
/// `ExTheme::GeneralAgcCw` alone holds 45 items, well past what a typical
/// terminal's control-panel area (`split_areas`' `Constraint::Min(8)` —
/// often well under 20 rows once header/status/errors take their fixed
/// share) can show at once. A fixed, unscrolled 45-line list would run off
/// the bottom of the panel on any ordinary terminal size, so `cursor`
/// drives a sliding window (centered on `cursor` where the list is longer
/// than the available height) instead.
fn draw_ex_sub_group_menu(f: &mut Frame, area: Rect, theme: ExTheme, cursor: usize) {
    let items = ex_theme_items(theme);

    let header_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let key_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let hint_style = Style::default().fg(Color::DarkGray);
    let selected_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            format!(
                "{} — item {} of {}",
                ex_theme_label(theme),
                items.len().min(cursor + 1),
                items.len()
            ),
            header_style,
        )),
        Line::from(""),
    ];

    // Reserve the 2 header lines above plus a trailing blank + hint line
    // below from the scrolling window's own height budget.
    let visible = (area.height as usize).saturating_sub(4).max(1);
    let start = if items.len() <= visible {
        0
    } else {
        cursor
            .saturating_sub(visible / 2)
            .min(items.len() - visible)
    };
    let end = (start + visible).min(items.len());

    for (offset, item) in items[start..end].iter().enumerate() {
        let idx = start + offset;
        let text = format!("{:03} {}", item.p1, item.name);
        if idx == cursor {
            lines.push(Line::from(Span::styled(
                format!("> {text}"),
                selected_style,
            )));
        } else {
            lines.push(Line::from(Span::raw(format!("  {text}"))));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("[Up/Down]", key_style),
        Span::styled(" scroll  ", hint_style),
        Span::styled("[Enter]", key_style),
        Span::styled(" select  ", hint_style),
        Span::styled("[Esc]", key_style),
        Span::styled(" back", hint_style),
    ]));

    f.render_widget(Paragraph::new(lines), area);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_hz_14mhz() {
        assert_eq!(format_hz(14_000_000), "14.000.000 MHz");
    }

    #[test]
    fn test_format_hz_min_boundary_030khz() {
        assert_eq!(format_hz(30_000), "0.030.000 MHz");
    }

    #[test]
    fn test_format_hz_max_boundary_470mhz() {
        assert_eq!(format_hz(470_000_000), "470.000.000 MHz");
    }

    #[test]
    fn test_smeter_bar_zero_is_empty() {
        let bar = smeter_bar(0, 20);
        assert!(!bar.contains('█'));
    }

    #[test]
    fn test_smeter_bar_max_is_full() {
        let bar = smeter_bar(255, 20);
        assert_eq!(bar.chars().filter(|&c| c == '█').count(), 20);
    }

    #[test]
    fn test_smeter_bar_mid_is_partial() {
        let bar = smeter_bar(128, 20);
        let filled = bar.chars().filter(|&c| c == '█').count();
        assert!(filled > 0 && filled < 20);
    }

    #[test]
    fn test_tx_state_label_off_is_green_rx() {
        assert_eq!(tx_state_label(TxState::Off), ("RX", Color::Green));
    }

    #[test]
    fn test_tx_state_label_cat_keyed_is_red_tx() {
        assert_eq!(tx_state_label(TxState::CatKeyed), ("TX", Color::Red));
    }

    #[test]
    fn test_tx_state_label_radio_keyed_non_cat_is_distinct_yellow() {
        let (text, color) = tx_state_label(TxState::RadioKeyedNonCat);
        assert_eq!(text, "TX (ext)");
        assert_eq!(color, Color::Yellow);
        // Must be visually distinct from CAT-keyed TX (same "TX" text but
        // different color/label would hide the non-CAT cause).
        assert_ne!(
            tx_state_label(TxState::RadioKeyedNonCat),
            tx_state_label(TxState::CatKeyed)
        );
    }

    #[test]
    fn test_ft991a_display_default_smoke() {
        let d = Ft991aDisplay::default();
        assert_eq!(d.vfo_a_hz, 14_000_000);
    }

    #[test]
    fn test_rts_label_off_is_dark_gray() {
        assert_eq!(rts_label(false), ("OFF", Color::DarkGray));
    }

    #[test]
    fn test_rts_label_on_is_red() {
        assert_eq!(rts_label(true), ("ON", Color::Red));
    }
}
