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
//! Wave 2. `draw_status`'s row 1 also gains a small `RTS: ON/OFF` indicator
//! (Wave 4 Task 4, §11.3 point 6) next to the existing `TX`/`RX` indicator —
//! reuses `tx_state_label`'s (label, color) rendering pattern via the new
//! `rts_label` helper, no new render function needed. `draw_control_panel`
//! gains one more arm for `ControlState::ExSubGroupMenu` (§11.4 path (a),
//! Wave 4 Task 9 — the wave's final task): `draw_ex_sub_group_menu` is the
//! first genuinely **scrolling** list in this crate (as opposed to
//! `GroupMenu`'s fixed, never-scrolled command column), needed because `EX`
//! sub-groups hold up to 45 items.
//!
//! A `Diagnostics` arm was added in the original `docs/adr/0004-shared-
//! diagnostics-screen.md` and reworked in `docs/adr/0006-hand-coded-full-
//! parity-diagnostics.md` once the engine itself moved from a wrapped
//! `cat-diagnostics` read-only probe to a hand-coded, ts570d-parity
//! test-and-restore engine living in this crate: `draw_diagnostics_panel`
//! is the same scrolling-list shape as `draw_ex_sub_group_menu`/
//! `draw_profile_list`, plus `draw_diagnostics_live` (a thin wrapper adding
//! the outer " Controls " block) for `terminal.rs` to call directly while a
//! run is still in progress, before any `ControlState::Diagnostics` exists
//! yet to dispatch through `draw_control_panel` normally.
//! `draw_diag_warning_panel` (ADR 0006) is the pre-run transmit-safety
//! gate's own distinctly red-bordered screen, replacing the whole panel
//! rather than sharing the generic " Controls " block.

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

// Shared console logic and terminal widgets (radio-cat-rs ADR 0011 rev 4).
use cat_framework::capabilities::MeterKind;
use cat_ui::{format_hz, MeterReading};
use cat_ui_ratatui::{
    bar_spans, error_panel, header, link_panel, menu_column, meter_spans, ErrorPanelStyles,
    LinkState,
};

/// This radio's S-meter, with the raw value the last poll returned.
///
/// Goes through `from_meters` rather than being built by hand so the
/// reading arrives carrying its own 0-255 range. That range is the whole
/// point: raw 15 is mid-scale on a TS-570D and under 6% here, and the
/// shared widgets draw both correctly precisely because they are never
/// told which radio they are drawing.
///
/// No S-unit table comes with it, deliberately — see
/// `radio::capabilities`. The manual gives no S-unit breakpoints for this
/// scale, so the readout stays a bar plus the raw number rather than an
/// invented calibration.
fn smeter_reading(state: &Ft991aDisplay) -> Option<MeterReading> {
    MeterReading::from_meters(
        &radio::capabilities::FT991A.meters,
        MeterKind::S,
        u16::from(state.smeter),
    )
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
    header(
        " FT-991A RADIO CONTROL ",
        Alignment::Center,
        area,
        f.buffer_mut(),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
}

// ---------------------------------------------------------------------------
// draw_errors — poll error panel
// ---------------------------------------------------------------------------

/// Draw the poll error panel.
///
/// The slot is reserved whether or not anything went wrong, so an empty
/// list draws "No errors" rather than nothing — an empty bordered box
/// reads as a panel that has failed, not one with nothing to say.
///
/// One thing changed when this moved onto the shared widget: the three
/// errors shown are now the **most recent** three rather than the first
/// three. A radio failing in a loop used to pin this panel to its oldest
/// failures and never show the current one. Recorded in
/// `docs/renderer-parity.md`.
pub fn draw_errors(f: &mut Frame, area: Rect, state: &Ft991aDisplay) {
    error_panel(
        &state.poll_errors,
        "Errors",
        ErrorPanelStyles {
            error: Style::default().fg(Color::Red),
            quiet: Some(("No errors", Style::default().fg(Color::DarkGray))),
        },
        area,
        f.buffer_mut(),
    );
}

// ---------------------------------------------------------------------------
// draw_disconnected — connection-lost overlay (replaces control panel)
// ---------------------------------------------------------------------------

/// Draw a full-panel overlay when the radio is unreachable or still connecting.
///
/// This replaces the control panel outright, so the `[Q] Quit` footer is
/// the only thing on screen telling the operator which key still works.
pub fn draw_disconnected(f: &mut Frame, area: Rect, errors: &[String], initializing: bool) {
    let state = if initializing {
        LinkState::Connecting
    } else {
        LinkState::Lost
    };
    link_panel(
        state,
        errors,
        "Radio Status",
        Some(Span::styled("[Q] Quit", Style::default().fg(Color::White))),
        area,
        f.buffer_mut(),
    );
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

    let smeter = smeter_reading(state);
    let mut line1_spans = vec![
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
        Span::styled("▐", Style::default().fg(Color::Green)),
    ];
    // The end caps are layout and stay here; the 20 cells between them are
    // the shared bar. Both halves keep the green this panel has always
    // used -- the block characters carry the contrast -- so the only thing
    // an operator sees change is that the bar now resolves eight sub-levels
    // per cell instead of whole cells.
    line1_spans.extend(match smeter {
        Some(r) => meter_spans(
            r,
            20,
            Style::default().fg(Color::Green),
            Style::default().fg(Color::Green),
        ),
        None => bar_spans(
            0.0,
            20,
            Style::default().fg(Color::DarkGray),
            Style::default().fg(Color::DarkGray),
        ),
    });
    line1_spans.extend([
        Span::styled("▌", Style::default().fg(Color::Green)),
        Span::raw(" "),
        // The raw reading stays beside the bar. This radio publishes no
        // S-unit table, so the number is the only precise thing on the
        // row -- and it is what makes a miscalibrated meter diagnosable
        // rather than merely wrong.
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
    let line1 = Line::from(line1_spans);

    f.render_widget(Paragraph::new(line1), rows[0]);

    // -----------------------------------------------------------------
    // Row 2 — VFO B, AF/RF/SQL/PWR/PS mini-bar row
    // -----------------------------------------------------------------

    let label_style = Style::default().fg(Color::DarkGray);
    let value_style = Style::default().fg(Color::White);
    let bracket_style = Style::default().fg(Color::DarkGray);
    let filled_style = Style::default().fg(Color::Yellow);
    let empty_style = Style::default().fg(Color::DarkGray);

    // These used to build a bar string and then filter it character by
    // character back into the two halves the line needs. `bar_spans`
    // returns those halves directly -- it is the same bar `meter_bar`
    // draws, in the shape this panel composes in.
    let af = bar_spans(state.af_gain as f32 / 255.0, 10, filled_style, empty_style);
    let rf = bar_spans(state.rf_gain as f32 / 255.0, 10, filled_style, empty_style);

    let ps_style = if state.power_on {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let ps_text = if state.power_on { "ON" } else { "OFF" };

    let mut line2_spans = vec![
        Span::styled("VFO B  ", Style::default().fg(Color::DarkGray)),
        Span::styled(format_hz(state.vfo_b_hz), Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled("AF:", label_style),
        Span::styled("[", bracket_style),
    ];
    line2_spans.extend(af);
    line2_spans.extend([
        Span::styled("]", bracket_style),
        Span::raw("  "),
        Span::styled("RF:", label_style),
        Span::styled("[", bracket_style),
    ]);
    line2_spans.extend(rf);
    line2_spans.extend([
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
    let line2 = Line::from(line2_spans);

    f.render_widget(Paragraph::new(line2), rows[1]);
}

// ---------------------------------------------------------------------------
// draw_control_panel
// ---------------------------------------------------------------------------

/// Build a column of menu lines from `(key, label)` pairs. Ported from
/// `cat_ui_ratatui::menu_column`.
/// The yellow-key styling the menu columns use.
///
/// The columns themselves come from `cat_ui_ratatui::menu_column`, which
/// is generic over the key type — this crate keyed menus by `char` and
/// `ts570d` by `&'static str`, which was the only reason the two could
/// not share the function.
fn menu_key_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

/// Draw the interactive control panel: the top-level `Menu` group list, a
/// `GroupMenu`'s own command list (or a "no commands yet" placeholder for
/// groups not yet populated — §11.2, Wave 4 Task 2), or the `TextInput`/
/// `ListSelect`/`Feedback` 3-line layout carried over unchanged from Wave 2
/// (§6.3).
pub fn draw_control_panel(f: &mut Frame, area: Rect, state: &ControlState) {
    // The transmit-safety warning gate replaces the whole panel with its
    // own distinctly red-bordered block, not the generic " Controls "
    // frame every other state shares — mirrors `ts570d`'s own
    // `draw_diag_warning_panel` call site exactly
    // (`docs/adr/0006-hand-coded-full-parity-diagnostics.md`).
    if let ControlState::DiagWarning = state {
        draw_diag_warning_panel(f, area);
        return;
    }

    let outer_block = Block::default().title(" Controls ").borders(Borders::ALL);
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    match state {
        ControlState::Menu => {
            let mut items = menu_group_labels();
            items.push((crate::control::PROFILE_LIST_KEY, "Profiles"));
            items.push((crate::control::DIAGNOSTICS_KEY, "Diagnostics"));
            items.push(('Q', "Quit"));
            f.render_widget(
                Paragraph::new(menu_column(&items, menu_key_style(), Style::default())),
                inner,
            );
        }

        // Handled by the early return above — never reached from here.
        ControlState::DiagWarning => {}

        ControlState::GroupMenu { group, .. } => {
            let labels = group_command_labels(*group);
            let mut lines: Vec<Line> = if labels.is_empty() {
                vec![Line::from(Span::styled(
                    format!("{} — no commands yet", group_label(*group)),
                    Style::default().fg(Color::DarkGray),
                ))]
            } else {
                menu_column(&labels, menu_key_style(), Style::default())
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

        // Profile list (§12.3) — same scrolling-list shape as
        // `ExSubGroupMenu`, since a profile directory can hold an arbitrary
        // number of entries.
        ControlState::ProfileList {
            profiles,
            cursor,
            error,
        } => {
            draw_profile_list(f, inner, profiles, *cursor, error.as_deref());
        }

        // Diagnostics (`docs/adr/0004-shared-diagnostics-screen.md`) — the
        // completed-report browsing view. Live in-progress rendering
        // during the run itself is drawn directly by `terminal.rs` (which
        // calls `draw_diagnostics_panel` itself, with `cursor: None`,
        // before `ControlState::Diagnostics` even exists) — see that
        // function's own doc comment.
        ControlState::Diagnostics { summary, cursor } => {
            draw_diagnostics_panel(f, inner, &summary.outcomes, summary.total(), Some(*cursor));
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

                // Menu, DiagWarning, GroupMenu, ExSubGroupMenu, ProfileList,
                // and Diagnostics are handled above.
                ControlState::Menu
                | ControlState::DiagWarning
                | ControlState::GroupMenu { .. }
                | ControlState::ExSubGroupMenu { .. }
                | ControlState::ProfileList { .. }
                | ControlState::Diagnostics { .. } => {}
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
// draw_profile_list — §12.3
// ---------------------------------------------------------------------------

/// Draw the profile list (`ControlState::ProfileList`) — same scrolling-list
/// shape as [`draw_ex_sub_group_menu`], since a profile directory can hold
/// an arbitrary number of files.
fn draw_profile_list(
    f: &mut Frame,
    area: Rect,
    profiles: &[(String, radio::Profile)],
    cursor: usize,
    error: Option<&str>,
) {
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

    let mut lines: Vec<Line> = vec![Line::from(Span::styled(
        format!("Profiles — {} found", profiles.len()),
        header_style,
    ))];
    if let Some(err) = error {
        lines.push(Line::from(Span::styled(
            format!("⚠ {}", err),
            Style::default().fg(Color::Red),
        )));
    } else {
        lines.push(Line::from(""));
    }

    let visible = (area.height as usize).saturating_sub(4).max(1);
    let start = if profiles.len() <= visible {
        0
    } else {
        cursor
            .saturating_sub(visible / 2)
            .min(profiles.len() - visible)
    };
    let end = (start + visible).min(profiles.len());

    for (idx, (name, _)) in profiles[start..end].iter().enumerate() {
        let idx = start + idx;
        if idx == cursor {
            lines.push(Line::from(Span::styled(
                format!("> {name}"),
                selected_style,
            )));
        } else {
            lines.push(Line::from(Span::raw(format!("  {name}"))));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("[Up/Down]", key_style),
        Span::styled(" scroll  ", hint_style),
        Span::styled("[Enter]", key_style),
        Span::styled(" apply  ", hint_style),
        Span::styled("[Esc]", key_style),
        Span::styled(" back", hint_style),
    ]));

    f.render_widget(Paragraph::new(lines), area);
}

// ---------------------------------------------------------------------------
// draw_diag_warning_panel — pre-diagnostic TX safety gate
// (`docs/adr/0006-hand-coded-full-parity-diagnostics.md`)
// ---------------------------------------------------------------------------

/// Draw the hard-to-miss warning shown before a diagnostic run starts.
/// Mirrors `ts570d::ui::layout::draw_diag_warning_panel` closely (same
/// wording, same red-bordered treatment) for cross-repo consistency.
///
/// The diagnostic run genuinely keys the transmitter (PTT, and CW if a
/// callsign is supplied on the next screen). Transmitting into an open or
/// mismatched load can damage the transceiver's final amplifier stage, so
/// this screen requires an explicit acknowledgment before anything is sent
/// to the radio.
fn draw_diag_warning_panel(f: &mut Frame, area: Rect) {
    let outer_block = Block::default()
        .title(" \u{26a0} DIAGNOSTICS \u{2014} TRANSMIT WARNING \u{26a0} ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD));
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "This diagnostic run will KEY THE TRANSMITTER.",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("It briefly transmits PTT, and sends a real CW test"),
        Line::from("message (via a keyer memory channel) if you supply a"),
        Line::from("callsign on the next screen."),
        Line::from(""),
        Line::from(Span::styled(
            "The radio MUST be connected to a proper antenna or dummy load.",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("Transmitting into an open or mismatched load can damage"),
        Line::from("the transceiver's final amplifier stage."),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "[Enter/Y]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" I have a load connected, proceed   "),
            Span::styled(
                "[Esc]",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" cancel"),
        ]),
    ];

    f.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false }),
        inner,
    );
}

// ---------------------------------------------------------------------------
// draw_diagnostics_panel — `docs/adr/0006-hand-coded-full-parity-
// diagnostics.md`
// ---------------------------------------------------------------------------

/// One-line (status glyph, color) pair for a [`crate::diagnostics::DiagResult`],
/// mirroring [`tx_state_label`]/[`rts_label`]'s own (text, color) idiom.
fn diagnostic_result_label(result: &crate::diagnostics::DiagResult) -> (&'static str, Color) {
    use crate::diagnostics::DiagResult;
    match result {
        DiagResult::Success { .. } => ("OK", Color::Green),
        DiagResult::Failure { .. } => ("FAIL", Color::Red),
        DiagResult::Skipped { .. } => ("SKIP", Color::DarkGray),
    }
}

/// Draw the diagnostics screen: same scrolling-list shape as
/// [`draw_profile_list`]/[`draw_ex_sub_group_menu`], since this engine has
/// 100+ steps.
///
/// Serves **two** call sites with one shared rendering function:
/// - **Live progress**, called directly by `terminal.rs`'s diagnostics
///   runner (`cursor: None`) once per [`crate::diagnostics::DiagOutcome`] as
///   the run proceeds — `outcomes` is a growing prefix of the final list,
///   `total` is the engine's total step count (known up-front, unlike
///   `outcomes.len()`), and the view auto-scrolls to follow the most recent
///   result.
/// - **The completed report**, via `draw_control_panel`'s
///   `ControlState::Diagnostics` arm (`cursor: Some(n)`) — `outcomes` is
///   now the final, complete list (`outcomes.len() == total`), and the
///   view scrolls around `cursor` like [`draw_profile_list`] instead of
///   auto-following the tail.
pub(crate) fn draw_diagnostics_panel(
    f: &mut Frame,
    area: Rect,
    outcomes: &[crate::diagnostics::DiagOutcome],
    total: usize,
    cursor: Option<usize>,
) {
    use crate::diagnostics::DiagResult;

    let header_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let key_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let hint_style = Style::default().fg(Color::DarkGray);
    let selected_style = Style::default().add_modifier(Modifier::BOLD);

    let running = outcomes.len() < total;
    let header = if running {
        format!("Running diagnostics... ({}/{total})", outcomes.len())
    } else {
        let passed = crate::diagnostics::count_passed(outcomes);
        let failed = crate::diagnostics::count_failed(outcomes);
        let skipped = crate::diagnostics::count_skipped(outcomes);
        format!(
            "Diagnostics — {passed} passed / {failed} failed / {skipped} skipped / {total} total"
        )
    };
    let mut lines: Vec<Line> = vec![Line::from(Span::styled(header, header_style))];
    lines.push(Line::from(""));

    let visible = (area.height as usize).saturating_sub(4).max(1);
    let start = match cursor {
        // Completed report: scroll around the cursor, like `draw_profile_list`.
        Some(cursor) if outcomes.len() > visible => cursor
            .saturating_sub(visible / 2)
            .min(outcomes.len() - visible),
        Some(_) => 0,
        // Live progress: always follow the tail (most recent outcome).
        None => outcomes.len().saturating_sub(visible),
    };
    let end = (start + visible).min(outcomes.len());

    for (idx, outcome) in outcomes[start..end].iter().enumerate() {
        let idx = start + idx;
        let (status_text, status_color) = diagnostic_result_label(&outcome.result);
        let is_selected = cursor == Some(idx);
        let prefix = if is_selected { "> " } else { "  " };
        let mut spans = vec![
            Span::raw(prefix),
            Span::styled(
                format!("[{status_text:>7}]"),
                Style::default().fg(status_color),
            ),
            Span::raw(format!(" {:<3} {}", outcome.code, outcome.name)),
        ];
        if is_selected {
            spans = spans
                .into_iter()
                .map(|s| {
                    let style = s.style.patch(selected_style);
                    s.style(style)
                })
                .collect();
        }
        lines.push(Line::from(spans));
    }

    // Detail line for the currently-selected outcome (completed report
    // only — nothing to show yet while a live run is still in progress and
    // no row is selectable).
    if let Some(cursor) = cursor {
        if let Some(outcome) = outcomes.get(cursor) {
            lines.push(Line::from(""));
            let detail = match &outcome.result {
                DiagResult::Success { detail } => {
                    format!("{detail}  ({:.0?})", outcome.duration)
                }
                DiagResult::Failure { message } => format!("Error: {message}"),
                DiagResult::Skipped { reason } => format!("Skipped: {reason}"),
            };
            lines.push(Line::from(Span::styled(detail, hint_style)));
        }
    }

    lines.push(Line::from(""));
    lines.push(if running {
        Line::from(Span::styled(
            "Please wait — exercising every CAT command...",
            hint_style,
        ))
    } else {
        Line::from(vec![
            Span::styled("[Up/Down]", key_style),
            Span::styled(" scroll  ", hint_style),
            Span::styled("[Esc]", key_style),
            Span::styled(" back", hint_style),
        ])
    });

    f.render_widget(Paragraph::new(lines), area);
}

/// Draw the diagnostics screen's live-progress frame, including the same
/// bordered " Controls " outer block [`draw_control_panel`] itself draws —
/// called directly by `terminal.rs`'s diagnostics runner while a run is
/// still in progress (before any `ControlState::Diagnostics` exists to
/// dispatch through [`draw_control_panel`] normally). A few lines of
/// duplication against that function's own outer-block setup, traded for
/// keeping all rendering logic (including this "how do we draw a running
/// diagnostics screen" concern) inside this module rather than leaking
/// `ratatui::widgets::Block` construction into `terminal.rs`.
pub(crate) fn draw_diagnostics_live(
    f: &mut Frame,
    area: Rect,
    outcomes: &[crate::diagnostics::DiagOutcome],
    total: usize,
) {
    let outer_block = Block::default().title(" Controls ").borders(Borders::ALL);
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);
    draw_diagnostics_panel(f, inner, outcomes, total, None);
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

    /// The S-meter bar as the status row now composes it: through this
    /// radio's own capabilities, not a local formula.
    fn bar_text(raw: u8) -> String {
        let state = Ft991aDisplay {
            smeter: raw,
            ..Default::default()
        };
        let reading = smeter_reading(&state).expect("this radio has an S meter");
        meter_spans(reading, 20, Style::default(), Style::default())
            .iter()
            .map(|s| s.content.to_string())
            .collect()
    }

    #[test]
    fn test_smeter_bar_zero_is_empty() {
        assert!(!bar_text(0).contains('█'));
    }

    #[test]
    fn test_smeter_bar_max_is_full() {
        assert_eq!(bar_text(255).chars().filter(|&c| c == '█').count(), 20);
    }

    #[test]
    fn test_smeter_bar_mid_is_partial() {
        let filled = bar_text(128).chars().filter(|&c| c == '█').count();
        assert!(filled > 0 && filled < 20);
    }

    #[test]
    fn the_bar_is_scaled_by_this_radios_range_and_not_another_ones() {
        // Raw 15 is mid-scale on a TS-570D and under 6% here. The shared
        // widget draws both correctly precisely because it is never told
        // which radio it is drawing -- the range arrives with the reading.
        let state = Ft991aDisplay {
            smeter: 15,
            ..Default::default()
        };
        let reading = smeter_reading(&state).unwrap();
        assert_eq!(reading.range.max, 255);
        assert!(reading.fraction() < 0.06);
    }

    #[test]
    fn no_s_unit_is_claimed_for_a_scale_nobody_calibrated() {
        // The CAT manual gives `SM`'s wire format (p.17, `000 - 255`) and
        // no S-unit breakpoints at all, so this radio publishes no table.
        // Inventing one here would be a claim about hardware.
        let state = Ft991aDisplay::default();
        let reading = smeter_reading(&state).unwrap();
        assert!(reading.s_units.is_none());

        // This test used to stop there, and its comment claimed "the row
        // shows the raw number instead". It does not: `s_unit()` falls
        // back to the generic formula, so the console has always shown an
        // S-unit for this radio, derived from a curve nobody measured. The
        // fallback is deliberate -- a bar with no number was worse -- but
        // the console must not present it as a reading. It is marked.
        assert!(!reading.s_unit_is_measured());
        assert_eq!(reading.s_unit_display(), format!("~{}", reading.s_unit()));
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
