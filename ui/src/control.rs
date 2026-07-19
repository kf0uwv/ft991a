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

//! Interactive control state machine for keyboard-driven radio commands.
//!
//! Flat two-level design (`Normal` -> `{TextInput, ListSelect}`), per
//! `planning/architect/task_plan.md` §6.4 — no `GroupMenu` layer, unlike
//! `ts570d/ui`'s three-level `Menu` -> `GroupMenu` -> `{TextInput,
//! ListSelect}` hierarchy. This slice only has 9 write-capable commands, so
//! a single flat descriptor list (`command_table`, below) drives both the
//! keymap and the `[key] label` rendering in `layout::draw_control_panel`.

use crossterm::event::{KeyCode, KeyEvent};

use radio::{Frequency, Mode, TxState};

use crate::Ft991aDisplay;

// ---------------------------------------------------------------------------
// State types
// ---------------------------------------------------------------------------

/// What radio action to perform when text input is confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputAction {
    SetVfoA,
    SetVfoB,
    SetAfGain,
    SetRfGain,
    SetSquelch,
    SetPower,
}

/// What radio action to perform when a list selection is confirmed.
///
/// Only one variant this slice (`SetMode`) — kept as an enum for symmetry
/// with `InputAction` and so a later wave can grow it without reshaping
/// `ControlState::ListSelect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectAction {
    SetMode,
}

/// A validated radio command ready to execute.
#[derive(Debug, Clone, PartialEq)]
pub enum ExecuteAction {
    SetVfoA(u64),
    SetVfoB(u64),
    SetMode(Mode),
    /// Toggle CAT TX/RX. Carries the *last polled* [`TxState`] so the
    /// executor knows whether to send `transmit()` or `receive()` — see
    /// §6.5's 3-valued `TxState` handling.
    ToggleTx(TxState),
    SetAfGain(u8),
    SetRfGain(u8),
    SetSquelch(u8),
    SetPower(u8),
    TogglePowerOn(bool),
}

/// The flat interactive control panel state machine.
///
/// Replaces ts570d's `Menu` -> `GroupMenu` -> `{TextInput, ListSelect}`
/// three-level hierarchy with a flat `Normal` -> `{TextInput, ListSelect}`
/// two-level machine (see module docs and §6.1/§6.4).
#[derive(Debug, Default, Clone, PartialEq)]
pub enum ControlState {
    /// Showing the flat keybinding list.
    #[default]
    Normal,
    /// User is typing text input.
    TextInput {
        prompt: String,
        buffer: String,
        error: Option<String>,
        action: InputAction,
    },
    /// User is selecting from a list (currently only mode selection).
    ListSelect {
        options: Vec<String>,
        cursor: usize,
        action: SelectAction,
    },
    /// Showing feedback after a command.
    Feedback { message: String, is_error: bool },
}

// ---------------------------------------------------------------------------
// KeyResult — returned by handle_key
// ---------------------------------------------------------------------------

/// The result of processing a key event.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyResult {
    /// Keep running — no radio command needed.
    Continue,
    /// Exit the UI.
    Quit,
    /// Execute a radio action with a validated value.
    Execute(ExecuteAction),
}

// ---------------------------------------------------------------------------
// Command descriptors — the single flat 9-entry table (§6.1, §6.5)
// ---------------------------------------------------------------------------

/// How a keybinding is activated. Reused from ts570d's
/// `CommandKind::{Text, List, Immediate}` pattern (`control.rs` lines
/// 219-237) — genuinely reusable at any command count, per §6.1.
enum CommandKind {
    /// Produces a `TextInput` state.
    Text {
        prompt: &'static str,
        action: InputAction,
    },
    /// Produces a `ListSelect` state.
    List {
        options: fn() -> Vec<String>,
        action: SelectAction,
    },
    /// Immediately produces an `ExecuteAction` (no input needed). Takes the
    /// current display state since `ToggleTx`/`TogglePowerOn` need it to
    /// decide which direction to toggle.
    Immediate(fn(&Ft991aDisplay) -> ExecuteAction),
}

struct Command {
    key: char,
    label: &'static str,
    kind: CommandKind,
}

fn mode_options() -> Vec<String> {
    vec![
        Mode::Lsb.name().to_string(),
        Mode::Usb.name().to_string(),
        Mode::CwU.name().to_string(),
        Mode::Fm.name().to_string(),
        Mode::Am.name().to_string(),
        Mode::RttyLsb.name().to_string(),
        Mode::CwL.name().to_string(),
        Mode::DataLsb.name().to_string(),
        Mode::RttyUsb.name().to_string(),
        Mode::DataFm.name().to_string(),
        Mode::FmN.name().to_string(),
        Mode::DataUsb.name().to_string(),
        Mode::AmN.name().to_string(),
        Mode::C4fm.name().to_string(),
    ]
}

/// The mode list in the same order as [`mode_options`], for cursor <->
/// `Mode` conversion.
const MODE_ORDER: [Mode; 14] = [
    Mode::Lsb,
    Mode::Usb,
    Mode::CwU,
    Mode::Fm,
    Mode::Am,
    Mode::RttyLsb,
    Mode::CwL,
    Mode::DataLsb,
    Mode::RttyUsb,
    Mode::DataFm,
    Mode::FmN,
    Mode::DataUsb,
    Mode::AmN,
    Mode::C4fm,
];

fn toggle_tx(display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::ToggleTx(display.tx_state)
}

fn toggle_power_on(display: &Ft991aDisplay) -> ExecuteAction {
    ExecuteAction::TogglePowerOn(display.power_on)
}

/// The single flat 9-entry command table (§6.5). Not directly exposed
/// outside this module — `layout.rs` renders the keybinding list via
/// [`keybinding_labels`] instead.
fn command_table() -> Vec<Command> {
    vec![
        Command {
            key: 'F',
            label: "Set VFO A",
            kind: CommandKind::Text {
                prompt: "Enter VFO A freq Hz (30000-470000000):",
                action: InputAction::SetVfoA,
            },
        },
        Command {
            key: 'B',
            label: "Set VFO B",
            kind: CommandKind::Text {
                prompt: "Enter VFO B freq Hz (30000-470000000):",
                action: InputAction::SetVfoB,
            },
        },
        Command {
            key: 'M',
            label: "Set mode",
            kind: CommandKind::List {
                options: mode_options,
                action: SelectAction::SetMode,
            },
        },
        Command {
            key: 'T',
            label: "Toggle CAT TX/RX",
            kind: CommandKind::Immediate(toggle_tx),
        },
        Command {
            key: 'A',
            label: "Set AF gain",
            kind: CommandKind::Text {
                prompt: "Enter AF gain (0-255):",
                action: InputAction::SetAfGain,
            },
        },
        Command {
            key: 'R',
            label: "Set RF gain",
            kind: CommandKind::Text {
                prompt: "Enter RF gain (0-255):",
                action: InputAction::SetRfGain,
            },
        },
        Command {
            key: 'S',
            label: "Set squelch",
            kind: CommandKind::Text {
                prompt: "Enter squelch (0-100):",
                action: InputAction::SetSquelch,
            },
        },
        Command {
            key: 'P',
            label: "Set TX power",
            kind: CommandKind::Text {
                prompt: "Enter TX power watts (5-100):",
                action: InputAction::SetPower,
            },
        },
        Command {
            key: 'O',
            label: "Toggle power on/off",
            kind: CommandKind::Immediate(toggle_power_on),
        },
    ]
}

fn find_command(key: char) -> Option<Command> {
    let key = key.to_ascii_uppercase();
    command_table().into_iter().find(|c| c.key == key)
}

/// Return the `(key, label)` pairs for rendering the flat keybinding list,
/// plus the fixed `[Q] Quit` entry (not part of `command_table` since it
/// has no `CommandKind` — it exits rather than transitioning state).
pub(crate) fn keybinding_labels() -> Vec<(char, &'static str)> {
    let mut v: Vec<(char, &'static str)> = command_table()
        .into_iter()
        .map(|c| (c.key, c.label))
        .collect();
    v.push(('Q', "Quit"));
    v
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate text input for the given [`InputAction`], returning a ready-to-
/// execute [`ExecuteAction`] or a human-readable error message.
///
/// Validation ranges per §6.5 — confirmed against `radio/src/radio_trait.rs`
/// and `radio/src/ft991a.rs` doc comments, NOT assumed from ts570d parity:
/// frequency uses `Frequency::MIN_HZ`/`MAX_HZ` (30 kHz-470 MHz, not
/// ts570d's 0.5-60 MHz), AF/RF gain 0-255, squelch 0-100 (not 255), TX
/// power 5-100 watts.
fn validate_text_input(action: InputAction, buffer: &str) -> Result<ExecuteAction, String> {
    match action {
        InputAction::SetVfoA | InputAction::SetVfoB => {
            let hz: u64 = buffer.trim().parse().map_err(|_| {
                format!(
                    "Enter a whole number of Hz ({}-{})",
                    Frequency::MIN_HZ,
                    Frequency::MAX_HZ
                )
            })?;
            match Frequency::new(hz) {
                Ok(freq) => Ok(if action == InputAction::SetVfoA {
                    ExecuteAction::SetVfoA(freq.hz())
                } else {
                    ExecuteAction::SetVfoB(freq.hz())
                }),
                Err(_) => Err(format!(
                    "Frequency must be {}-{} Hz",
                    Frequency::MIN_HZ,
                    Frequency::MAX_HZ
                )),
            }
        }
        InputAction::SetAfGain | InputAction::SetRfGain => {
            let v: u16 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 0-255".to_string())?;
            if v > 255 {
                return Err("Value must be 0-255".to_string());
            }
            Ok(if action == InputAction::SetAfGain {
                ExecuteAction::SetAfGain(v as u8)
            } else {
                ExecuteAction::SetRfGain(v as u8)
            })
        }
        InputAction::SetSquelch => {
            let v: u16 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 0-100".to_string())?;
            if v > 100 {
                return Err("Value must be 0-100".to_string());
            }
            Ok(ExecuteAction::SetSquelch(v as u8))
        }
        InputAction::SetPower => {
            let v: u16 = buffer
                .trim()
                .parse()
                .map_err(|_| "Enter a number 5-100".to_string())?;
            if !(5..=100).contains(&v) {
                return Err("Value must be 5-100".to_string());
            }
            Ok(ExecuteAction::SetPower(v as u8))
        }
    }
}

fn select_action_to_execute(action: SelectAction, cursor: usize) -> ExecuteAction {
    match action {
        SelectAction::SetMode => {
            let mode = MODE_ORDER.get(cursor).copied().unwrap_or(Mode::Usb);
            ExecuteAction::SetMode(mode)
        }
    }
}

/// Return the cursor index that should be pre-selected when a list opens,
/// based on the current radio state, so the highlight starts on the active
/// value.
fn initial_list_cursor(action: SelectAction, display: &Ft991aDisplay) -> usize {
    match action {
        SelectAction::SetMode => MODE_ORDER
            .iter()
            .position(|m| *m == display.mode)
            .unwrap_or(0),
    }
}

// ---------------------------------------------------------------------------
// handle_key — the main event handler
// ---------------------------------------------------------------------------

/// Process a key event and transition the control state.
///
/// Returns `KeyResult::Continue`, `KeyResult::Quit`, or `KeyResult::Execute`.
pub fn handle_key(key: KeyEvent, state: &mut ControlState, display: &Ft991aDisplay) -> KeyResult {
    match state {
        ControlState::Normal => match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => KeyResult::Quit,
            KeyCode::Char(c) => {
                let Some(cmd) = find_command(c) else {
                    return KeyResult::Continue;
                };
                match cmd.kind {
                    CommandKind::Text { prompt, action } => {
                        *state = ControlState::TextInput {
                            prompt: prompt.to_string(),
                            buffer: String::new(),
                            error: None,
                            action,
                        };
                        KeyResult::Continue
                    }
                    CommandKind::List { options, action } => {
                        let cursor = initial_list_cursor(action, display);
                        *state = ControlState::ListSelect {
                            options: options(),
                            cursor,
                            action,
                        };
                        KeyResult::Continue
                    }
                    CommandKind::Immediate(f) => {
                        let exec = f(display);
                        *state = ControlState::Feedback {
                            message: String::new(),
                            is_error: false,
                        };
                        KeyResult::Execute(exec)
                    }
                }
            }
            _ => KeyResult::Continue,
        },

        ControlState::TextInput {
            buffer,
            error,
            action,
            ..
        } => match key.code {
            KeyCode::Char(c) if c.is_ascii_graphic() => {
                buffer.push(c);
                *error = None;
                KeyResult::Continue
            }
            KeyCode::Backspace => {
                buffer.pop();
                *error = None;
                KeyResult::Continue
            }
            KeyCode::Enter => {
                let action = *action;
                let buf = buffer.clone();
                match validate_text_input(action, &buf) {
                    Ok(exec) => {
                        *state = ControlState::Feedback {
                            message: String::new(),
                            is_error: false,
                        };
                        KeyResult::Execute(exec)
                    }
                    Err(msg) => {
                        if let ControlState::TextInput { error, .. } = state {
                            *error = Some(msg);
                        }
                        KeyResult::Continue
                    }
                }
            }
            KeyCode::Esc => {
                *state = ControlState::Normal;
                KeyResult::Continue
            }
            _ => KeyResult::Continue,
        },

        ControlState::ListSelect {
            options,
            cursor,
            action,
        } => match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                if *cursor > 0 {
                    *cursor -= 1;
                }
                KeyResult::Continue
            }
            KeyCode::Right | KeyCode::Char('l') => {
                let max = options.len().saturating_sub(1);
                if *cursor < max {
                    *cursor += 1;
                }
                KeyResult::Continue
            }
            KeyCode::Enter => {
                let exec = select_action_to_execute(*action, *cursor);
                *state = ControlState::Feedback {
                    message: String::new(),
                    is_error: false,
                };
                KeyResult::Execute(exec)
            }
            KeyCode::Esc => {
                *state = ControlState::Normal;
                KeyResult::Continue
            }
            _ => KeyResult::Continue,
        },

        ControlState::Feedback { .. } => {
            *state = ControlState::Normal;
            KeyResult::Continue
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn display() -> Ft991aDisplay {
        Ft991aDisplay::default()
    }

    // --- Normal state dispatch ---

    #[test]
    fn test_normal_q_quits() {
        let mut state = ControlState::Normal;
        let result = handle_key(key(KeyCode::Char('q')), &mut state, &display());
        assert!(matches!(result, KeyResult::Quit));
    }

    #[test]
    fn test_normal_uppercase_q_quits() {
        let mut state = ControlState::Normal;
        let result = handle_key(key(KeyCode::Char('Q')), &mut state, &display());
        assert!(matches!(result, KeyResult::Quit));
    }

    #[test]
    fn test_normal_f_transitions_to_text_input_vfo_a() {
        let mut state = ControlState::Normal;
        let result = handle_key(key(KeyCode::Char('f')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetVfoA,
                ..
            }
        ));
    }

    #[test]
    fn test_normal_b_transitions_to_text_input_vfo_b() {
        let mut state = ControlState::Normal;
        handle_key(key(KeyCode::Char('B')), &mut state, &display());
        assert!(matches!(
            state,
            ControlState::TextInput {
                action: InputAction::SetVfoB,
                ..
            }
        ));
    }

    #[test]
    fn test_normal_m_transitions_to_list_select_with_14_modes() {
        let mut state = ControlState::Normal;
        handle_key(key(KeyCode::Char('m')), &mut state, &display());
        match state {
            ControlState::ListSelect {
                options, action, ..
            } => {
                assert_eq!(options.len(), 14);
                assert_eq!(action, SelectAction::SetMode);
            }
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_normal_m_preselects_current_mode() {
        let mut state = ControlState::Normal;
        let mut d = display();
        d.mode = Mode::CwU;
        handle_key(key(KeyCode::Char('m')), &mut state, &d);
        match state {
            ControlState::ListSelect { cursor, .. } => assert_eq!(cursor, 2),
            other => panic!("expected ListSelect, got {other:?}"),
        }
    }

    #[test]
    fn test_normal_t_is_immediate_toggle_tx() {
        let mut state = ControlState::Normal;
        let result = handle_key(key(KeyCode::Char('t')), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::ToggleTx(TxState::Off))
        ));
        assert!(matches!(state, ControlState::Feedback { .. }));
    }

    #[test]
    fn test_normal_o_is_immediate_toggle_power() {
        let mut state = ControlState::Normal;
        let mut d = display();
        d.power_on = true;
        let result = handle_key(key(KeyCode::Char('o')), &mut state, &d);
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::TogglePowerOn(true))
        ));
    }

    #[test]
    fn test_normal_unbound_key_continues() {
        let mut state = ControlState::Normal;
        let result = handle_key(key(KeyCode::Char('z')), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Normal));
    }

    #[test]
    fn test_all_nine_keys_present_and_unique() {
        let labels = keybinding_labels();
        assert_eq!(labels.len(), 10); // 9 commands + Quit
        let mut keys: Vec<char> = labels.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 10, "keybindings must be unique");
    }

    // --- TextInput: typing / backspace / escape ---

    #[test]
    fn test_text_input_typing_appends_to_buffer() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: String::new(),
            error: None,
            action: InputAction::SetVfoA,
        };
        handle_key(key(KeyCode::Char('1')), &mut state, &display());
        handle_key(key(KeyCode::Char('4')), &mut state, &display());
        if let ControlState::TextInput { buffer, .. } = &state {
            assert_eq!(buffer, "14");
        } else {
            panic!("expected TextInput");
        }
    }

    #[test]
    fn test_text_input_backspace_removes_last_char() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "14".to_string(),
            error: None,
            action: InputAction::SetVfoA,
        };
        handle_key(key(KeyCode::Backspace), &mut state, &display());
        if let ControlState::TextInput { buffer, .. } = &state {
            assert_eq!(buffer, "1");
        } else {
            panic!("expected TextInput");
        }
    }

    #[test]
    fn test_text_input_esc_returns_to_normal() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "14".to_string(),
            error: None,
            action: InputAction::SetVfoA,
        };
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Normal));
    }

    // --- TextInput validation: VFO A/B frequency range ---

    #[test]
    fn test_vfo_a_valid_frequency_min_boundary() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: Frequency::MIN_HZ.to_string(),
            error: None,
            action: InputAction::SetVfoA,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetVfoA(hz)) if hz == Frequency::MIN_HZ
        ));
    }

    #[test]
    fn test_vfo_a_valid_frequency_max_boundary() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: Frequency::MAX_HZ.to_string(),
            error: None,
            action: InputAction::SetVfoA,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetVfoA(hz)) if hz == Frequency::MAX_HZ
        ));
    }

    #[test]
    fn test_vfo_a_below_min_rejected() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: (Frequency::MIN_HZ - 1).to_string(),
            error: None,
            action: InputAction::SetVfoA,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        if let ControlState::TextInput { error, .. } = &state {
            assert!(error.is_some());
        } else {
            panic!("expected TextInput");
        }
    }

    #[test]
    fn test_vfo_a_above_max_rejected() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: (Frequency::MAX_HZ + 1).to_string(),
            error: None,
            action: InputAction::SetVfoA,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        if let ControlState::TextInput { error, .. } = &state {
            assert!(error.is_some());
        } else {
            panic!("expected TextInput");
        }
    }

    #[test]
    fn test_vfo_a_non_numeric_rejected() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "not_a_number".to_string(),
            error: None,
            action: InputAction::SetVfoA,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_vfo_b_uses_same_range_as_vfo_a() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: Frequency::MIN_HZ.to_string(),
            error: None,
            action: InputAction::SetVfoB,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetVfoB(hz)) if hz == Frequency::MIN_HZ
        ));
    }

    // --- TextInput validation: AF/RF gain 0-255 ---

    #[test]
    fn test_af_gain_max_255_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "255".to_string(),
            error: None,
            action: InputAction::SetAfGain,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetAfGain(255))
        ));
    }

    #[test]
    fn test_af_gain_256_rejected() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "256".to_string(),
            error: None,
            action: InputAction::SetAfGain,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_rf_gain_0_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "0".to_string(),
            error: None,
            action: InputAction::SetRfGain,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetRfGain(0))
        ));
    }

    // --- TextInput validation: squelch 0-100 (NOT 255) ---

    #[test]
    fn test_squelch_100_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "100".to_string(),
            error: None,
            action: InputAction::SetSquelch,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetSquelch(100))
        ));
    }

    #[test]
    fn test_squelch_101_rejected() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "101".to_string(),
            error: None,
            action: InputAction::SetSquelch,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_squelch_255_rejected_not_255_range() {
        // Regression guard: squelch is 0-100, NOT 0-255 like AF/RF gain.
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "255".to_string(),
            error: None,
            action: InputAction::SetSquelch,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        if let ControlState::TextInput { error, .. } = &state {
            assert!(error.is_some());
        } else {
            panic!("expected TextInput");
        }
    }

    // --- TextInput validation: TX power 5-100 ---

    #[test]
    fn test_power_5_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "5".to_string(),
            error: None,
            action: InputAction::SetPower,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetPower(5))
        ));
    }

    #[test]
    fn test_power_100_accepted() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "100".to_string(),
            error: None,
            action: InputAction::SetPower,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetPower(100))
        ));
    }

    #[test]
    fn test_power_4_rejected_below_min() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "4".to_string(),
            error: None,
            action: InputAction::SetPower,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    #[test]
    fn test_power_101_rejected_above_max() {
        let mut state = ControlState::TextInput {
            prompt: "p".to_string(),
            buffer: "101".to_string(),
            error: None,
            action: InputAction::SetPower,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
    }

    // --- ListSelect: navigation and confirm ---

    #[test]
    fn test_list_select_right_advances_cursor() {
        let mut state = ControlState::ListSelect {
            options: mode_options(),
            cursor: 0,
            action: SelectAction::SetMode,
        };
        handle_key(key(KeyCode::Right), &mut state, &display());
        if let ControlState::ListSelect { cursor, .. } = &state {
            assert_eq!(*cursor, 1);
        } else {
            panic!("expected ListSelect");
        }
    }

    #[test]
    fn test_list_select_left_at_zero_stays_zero() {
        let mut state = ControlState::ListSelect {
            options: mode_options(),
            cursor: 0,
            action: SelectAction::SetMode,
        };
        handle_key(key(KeyCode::Left), &mut state, &display());
        if let ControlState::ListSelect { cursor, .. } = &state {
            assert_eq!(*cursor, 0);
        } else {
            panic!("expected ListSelect");
        }
    }

    #[test]
    fn test_list_select_right_at_max_stays_max() {
        let mut state = ControlState::ListSelect {
            options: mode_options(),
            cursor: 13,
            action: SelectAction::SetMode,
        };
        handle_key(key(KeyCode::Right), &mut state, &display());
        if let ControlState::ListSelect { cursor, .. } = &state {
            assert_eq!(*cursor, 13);
        } else {
            panic!("expected ListSelect");
        }
    }

    #[test]
    fn test_list_select_enter_produces_execute_set_mode() {
        let mut state = ControlState::ListSelect {
            options: mode_options(),
            cursor: 13, // C4fm
            action: SelectAction::SetMode,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetMode(Mode::C4fm))
        ));
    }

    #[test]
    fn test_list_select_esc_returns_to_normal() {
        let mut state = ControlState::ListSelect {
            options: mode_options(),
            cursor: 0,
            action: SelectAction::SetMode,
        };
        let result = handle_key(key(KeyCode::Esc), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Normal));
    }

    // --- Feedback: any key returns to Normal ---

    #[test]
    fn test_feedback_any_key_returns_to_normal() {
        let mut state = ControlState::Feedback {
            message: "OK".to_string(),
            is_error: false,
        };
        let result = handle_key(key(KeyCode::Enter), &mut state, &display());
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Normal));
    }

    // --- 3-valued TxState toggle behavior (§6.5) ---

    #[test]
    fn test_toggle_tx_from_off_sends_transmit_direction() {
        // ToggleTx carries the last-polled state; the executor (terminal.rs)
        // decides transmit() vs receive() from it. Off/RadioKeyedNonCat -> transmit.
        let mut state = ControlState::Normal;
        let mut d = display();
        d.tx_state = TxState::Off;
        let result = handle_key(key(KeyCode::Char('t')), &mut state, &d);
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::ToggleTx(TxState::Off))
        );
    }

    #[test]
    fn test_toggle_tx_from_cat_keyed_carries_cat_keyed() {
        let mut state = ControlState::Normal;
        let mut d = display();
        d.tx_state = TxState::CatKeyed;
        let result = handle_key(key(KeyCode::Char('t')), &mut state, &d);
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::ToggleTx(TxState::CatKeyed))
        );
    }

    #[test]
    fn test_toggle_tx_from_radio_keyed_non_cat_carries_that_state() {
        let mut state = ControlState::Normal;
        let mut d = display();
        d.tx_state = TxState::RadioKeyedNonCat;
        let result = handle_key(key(KeyCode::Char('t')), &mut state, &d);
        assert_eq!(
            result,
            KeyResult::Execute(ExecuteAction::ToggleTx(TxState::RadioKeyedNonCat))
        );
    }
}
