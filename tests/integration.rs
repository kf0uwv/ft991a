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

//! Integration tests for `Ft991a<SerialPort>` against the built-in emulator.
//!
//! Mirrors `ts570d/tests/integration.rs`'s shape exactly (that repo has a
//! 100-test suite of this kind; this repo previously had none — every test
//! elsewhere in this workspace runs against a fake `CatSession`/`MockRadio`,
//! never a real PTY-hosted emulator). Each test starts a fresh emulator
//! instance (backed by a new PTY pair), opens a real `SerialPort` on the
//! slave PTY, and exercises the full command/response round-trip through
//! the io_uring serial driver — genuine two-process-shaped CAT protocol
//! traffic, not an in-process fake.
//!
//! ## Why each test owns its emulator
//!
//! Starting a fresh emulator per test gives clean, default `Ft991aState`
//! and avoids ordering dependencies between tests.
//!
//! ## Set-then-get pattern
//!
//! Most commands here are tested by setting a value then reading it back
//! and asserting an exact match — this is the strong check (it proves the
//! SET wire command actually reached and mutated the emulator's state, not
//! just that GET parses), and doesn't require knowing the emulator's exact
//! default state. Read-only commands are checked either against a value
//! this crate's own doc comments guarantee (e.g. `get_rx_busy` is
//! documented to always report `false` on this emulator) or with a
//! validity/range assertion when no such guarantee exists.
//!
//! ## Coverage
//!
//! Every `radio::Radio` and `radio::Ft991aExtras` trait method with a real
//! (non-`NotImplemented`) `Ft991a` implementation gets its own test,
//! organized into the same batches `radio/src/radio_trait.rs`'s own doc
//! comments use, for easy cross-reference. The 151-item `EX` menu is
//! covered by one generic, table-driven test (`ex_menu_round_trips_every_landed_item`)
//! iterating `EX_MENU_TABLE` rather than 151 hand-written functions —
//! ft991a's EX menu has no ts570d equivalent to mirror the style of.

//! Linux-only: exercises the io_uring serial driver directly (`monoio`,
//! target-gated to Linux) against a PTY-backed emulator (pseudo-terminals
//! are a Unix concept). Mirrors `ts570d/tests/integration.rs`'s own gate.
//! A Windows port of this suite would need a Windows-side fake/loopback
//! transport, which no ADR has scoped.
#![cfg(target_os = "linux")]

use std::time::Duration;

use cat_transport_serial::{SerialCatSession, SerialConfig, SerialPort};
use emulator::emulator::Emulator;
use monoio::RuntimeBuilder;
use radio::ft991a_radio::ExMenuValueKind;
use radio::{
    Band, ChannelStatusFields, EncoderSelector, Frequency, Ft991a, KeyerPlaybackMode,
    MemoryChannelEntry, MemoryTag, Meter, Mode, PreampMode, RadioIndicator, RepeaterShift,
    ScanState, TaggedMemoryChannel, ToneSquelchMode, TxState,
};
use radio::{CTCSS_TONES_DECIHZ, DCS_CODES, EX_MENU_TABLE};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Start an emulator in a background thread and return the slave PTY path.
///
/// Waits 150 ms for the emulator thread to enter its read loop before
/// returning.
fn start_emulator() -> String {
    let mut emu = Emulator::new().expect("Emulator::new failed");
    let slave_path = emu.slave_path().to_string();
    std::thread::spawn(move || {
        let _ = emu.run();
    });
    std::thread::sleep(Duration::from_millis(150));
    slave_path
}

/// Open a `Ft991a<SerialCatSession<SerialPort>>` on the given slave PTY path
/// at 9600 baud (8N2, the FT-991A's documented default — see
/// `planning/architect/task_plan.md` §1).
///
/// Must be called from within an active monoio runtime context.
fn open_radio(slave_path: &str) -> Ft991a<SerialCatSession<SerialPort>> {
    let cfg = SerialConfig {
        baud_rate: 9600,
        ..SerialConfig::default()
    };
    let port = SerialPort::open(slave_path, cfg)
        .unwrap_or_else(|e| panic!("SerialPort::open({}) failed: {}", slave_path, e));
    Ft991a::new(SerialCatSession::new(port))
}

/// Build a monoio IoUring runtime for use in tests.
fn make_runtime() -> monoio::Runtime<monoio::IoUringDriver> {
    RuntimeBuilder::<monoio::IoUringDriver>::new()
        .build()
        .expect("monoio IoUring runtime build failed")
}

/// Run an async test body inside a monoio io_uring runtime.
macro_rules! async_test {
    ($body:expr) => {
        make_runtime().block_on($body)
    };
}

// ---------------------------------------------------------------------------
// VFO A frequency
// ---------------------------------------------------------------------------

#[test]
fn test_get_vfo_a() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let freq = radio.get_vfo_a().await.expect("get_vfo_a");
        assert_eq!(freq.hz(), 14_000_000, "expected VFO A default=14.000 MHz");
    });
}

#[test]
fn test_set_vfo_a() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let target = Frequency::new(14_195_000).expect("Frequency::new");
        radio.set_vfo_a(target).await.expect("set_vfo_a");
        let got = radio.get_vfo_a().await.expect("get_vfo_a after set");
        assert_eq!(got.hz(), 14_195_000, "VFO A mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// VFO B frequency
// ---------------------------------------------------------------------------

#[test]
fn test_get_vfo_b() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let freq = radio.get_vfo_b().await.expect("get_vfo_b");
        assert_eq!(freq.hz(), 14_100_000, "expected VFO B default=14.100 MHz");
    });
}

#[test]
fn test_set_vfo_b() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let target = Frequency::new(7_100_000).expect("Frequency::new");
        radio.set_vfo_b(target).await.expect("set_vfo_b");
        let got = radio.get_vfo_b().await.expect("get_vfo_b after set");
        assert_eq!(got.hz(), 7_100_000, "VFO B mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Mode
// ---------------------------------------------------------------------------

#[test]
fn test_get_mode() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let mode = radio.get_mode().await.expect("get_mode");
        assert_eq!(mode, Mode::Usb, "expected mode default=USB");
    });
}

#[test]
fn test_set_mode() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_mode(Mode::CwU).await.expect("set_mode(CwU)");
        let got = radio.get_mode().await.expect("get_mode after set");
        assert_eq!(got, Mode::CwU, "mode mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// PTT / TX state
// ---------------------------------------------------------------------------

#[test]
fn test_get_tx_state_default_off() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let state = radio.get_tx_state().await.expect("get_tx_state");
        assert_eq!(state, TxState::Off, "expected TX state default=Off");
    });
}

#[test]
fn test_transmit_then_receive() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.transmit().await.expect("transmit");
        let state = radio
            .get_tx_state()
            .await
            .expect("get_tx_state after transmit");
        assert_eq!(state, TxState::CatKeyed, "expected CatKeyed after transmit");

        radio.receive().await.expect("receive");
        let state = radio
            .get_tx_state()
            .await
            .expect("get_tx_state after receive");
        assert_eq!(state, TxState::Off, "expected Off after receive");
    });
}

// ---------------------------------------------------------------------------
// S-meter
// ---------------------------------------------------------------------------

#[test]
fn test_get_smeter() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_smeter().await.expect("get_smeter");
        assert_eq!(v, 0, "expected S-meter default=0");
    });
}

// ---------------------------------------------------------------------------
// Meter select (MS) / direct meter read (RM)
// ---------------------------------------------------------------------------

#[test]
fn test_select_meter_and_get_selected_meter_round_trips_every_variant() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        for meter in [
            Meter::Comp,
            Meter::Alc,
            Meter::Po,
            Meter::Swr,
            Meter::Id,
            Meter::Vdd,
        ] {
            radio.select_meter(meter).await.expect("select_meter");
            let got = radio
                .get_selected_meter()
                .await
                .expect("get_selected_meter");
            assert_eq!(got, meter, "selected meter mismatch for {meter:?}");
        }
    });
}

#[test]
fn test_get_meter_reads_every_variant_in_range() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        for meter in [
            Meter::Comp,
            Meter::Alc,
            Meter::Po,
            Meter::Swr,
            Meter::Id,
            Meter::Vdd,
        ] {
            radio
                .get_meter(meter)
                .await
                .unwrap_or_else(|e| panic!("get_meter({meter:?}) failed: {e}"));
        }
    });
}

// ---------------------------------------------------------------------------
// Power on/off (PS)
// ---------------------------------------------------------------------------

#[test]
fn test_get_power_on_default_true() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_power_on().await.expect("get_power_on");
        assert!(v, "expected power_on default=true");
    });
}

#[test]
fn test_set_power_on() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_power_on(false)
            .await
            .expect("set_power_on(false)");
        let v = radio.get_power_on().await.expect("get_power_on after set");
        assert!(!v, "expected power_on=false after set");

        radio.set_power_on(true).await.expect("set_power_on(true)");
        let v = radio
            .get_power_on()
            .await
            .expect("get_power_on after set back");
        assert!(v, "expected power_on=true after set back");
    });
}

// ---------------------------------------------------------------------------
// AF / RF gain, squelch
// ---------------------------------------------------------------------------

#[test]
fn test_get_af_gain() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_af_gain().await.expect("get_af_gain");
        assert_eq!(v, 128, "expected af_gain default=128");
    });
}

#[test]
fn test_set_af_gain() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_af_gain(50).await.expect("set_af_gain(50)");
        let v = radio.get_af_gain().await.expect("get_af_gain after set");
        assert_eq!(v, 50, "af_gain mismatch after set");
    });
}

#[test]
fn test_get_rf_gain() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_rf_gain().await.expect("get_rf_gain");
        assert_eq!(v, 255, "expected rf_gain default=255");
    });
}

#[test]
fn test_set_rf_gain() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_rf_gain(100).await.expect("set_rf_gain(100)");
        let v = radio.get_rf_gain().await.expect("get_rf_gain after set");
        assert_eq!(v, 100, "rf_gain mismatch after set");
    });
}

#[test]
fn test_get_squelch() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_squelch().await.expect("get_squelch");
        assert_eq!(v, 0, "expected squelch default=0");
    });
}

#[test]
fn test_set_squelch() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_squelch(30).await.expect("set_squelch(30)");
        let v = radio.get_squelch().await.expect("get_squelch after set");
        assert_eq!(v, 30, "squelch mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Transmit power
// ---------------------------------------------------------------------------

#[test]
fn test_get_power() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_power().await.expect("get_power");
        assert_eq!(v, 100, "expected power default=100W");
    });
}

#[test]
fn test_set_power() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_power(50).await.expect("set_power(50)");
        let v = radio.get_power().await.expect("get_power after set");
        assert_eq!(v, 50, "power mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Radio ID
// ---------------------------------------------------------------------------

#[test]
fn test_get_id() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let id = radio.get_id().await.expect("get_id");
        assert_eq!(id, "0670", "expected FT-991A model ID 0670");
    });
}

// ---------------------------------------------------------------------------
// Memory channels (MC, MR, MW, MT)
// ---------------------------------------------------------------------------

#[test]
fn test_get_and_set_memory_channel() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_memory_channel(5)
            .await
            .expect("set_memory_channel(5)");
        let ch = radio
            .get_memory_channel()
            .await
            .expect("get_memory_channel");
        assert_eq!(ch, 5, "memory channel mismatch after set");
    });
}

fn sample_memory_entry(channel: u8) -> MemoryChannelEntry {
    MemoryChannelEntry {
        channel,
        frequency_hz: 21_250_000,
        clarifier_offset_hz: 0,
        rx_clarifier_on: false,
        tx_clarifier_on: false,
        mode: Mode::Usb,
        tone_status: 0,
        offset_type: 0,
    }
}

#[test]
fn test_write_then_read_memory_channel() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let entry = sample_memory_entry(10);
        radio
            .write_memory_channel(entry)
            .await
            .expect("write_memory_channel");
        let got = radio
            .read_memory_channel(10)
            .await
            .expect("read_memory_channel");
        assert_eq!(
            got.frequency_hz, entry.frequency_hz,
            "channel freq mismatch"
        );
        assert_eq!(got.mode, entry.mode, "channel mode mismatch");
    });
}

#[test]
fn test_write_then_read_memory_channel_tag() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let tagged = TaggedMemoryChannel {
            entry: sample_memory_entry(11),
            tag: MemoryTag::new("TESTTAG").expect("MemoryTag::new"),
        };
        radio
            .write_memory_channel_tag(tagged.clone())
            .await
            .expect("write_memory_channel_tag");
        let got = radio
            .read_memory_channel_tag(11)
            .await
            .expect("read_memory_channel_tag");
        assert_eq!(
            got.entry.frequency_hz, tagged.entry.frequency_hz,
            "tagged channel freq mismatch"
        );
        assert_eq!(got.tag, tagged.tag, "tagged channel tag mismatch");
    });
}

// ---------------------------------------------------------------------------
// VFO A/B copy, swap, memory quick-ops (AB BA AM MA CH)
// ---------------------------------------------------------------------------

#[test]
fn test_copy_vfo_a_to_b() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let a = radio.get_vfo_a().await.expect("get_vfo_a");
        radio.copy_vfo_a_to_b().await.expect("copy_vfo_a_to_b");
        let b = radio.get_vfo_b().await.expect("get_vfo_b after copy");
        assert_eq!(
            b.hz(),
            a.hz(),
            "VFO B should equal VFO A after copy_vfo_a_to_b"
        );
    });
}

#[test]
fn test_copy_vfo_b_to_a() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let b = radio.get_vfo_b().await.expect("get_vfo_b");
        radio.copy_vfo_b_to_a().await.expect("copy_vfo_b_to_a");
        let a = radio.get_vfo_a().await.expect("get_vfo_a after copy");
        assert_eq!(
            a.hz(),
            b.hz(),
            "VFO A should equal VFO B after copy_vfo_b_to_a"
        );
    });
}

#[test]
fn test_swap_vfos() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let a_before = radio.get_vfo_a().await.expect("get_vfo_a");
        let b_before = radio.get_vfo_b().await.expect("get_vfo_b");
        radio.swap_vfos().await.expect("swap_vfos");
        let a_after = radio.get_vfo_a().await.expect("get_vfo_a after swap");
        let b_after = radio.get_vfo_b().await.expect("get_vfo_b after swap");
        assert_eq!(
            a_after.hz(),
            b_before.hz(),
            "VFO A should hold old VFO B after swap"
        );
        assert_eq!(
            b_after.hz(),
            a_before.hz(),
            "VFO B should hold old VFO A after swap"
        );
    });
}

#[test]
fn test_store_vfo_to_memory_then_recall() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_memory_channel(20)
            .await
            .expect("set_memory_channel(20)");
        let target = Frequency::new(28_500_000).expect("Frequency::new");
        radio.set_vfo_a(target).await.expect("set_vfo_a");
        radio
            .store_vfo_to_memory()
            .await
            .expect("store_vfo_to_memory");

        // Change VFO A away, then recall and confirm it comes back.
        radio
            .set_vfo_a(Frequency::new(7_000_000).unwrap())
            .await
            .expect("set_vfo_a (perturb)");
        radio
            .recall_memory_to_vfo()
            .await
            .expect("recall_memory_to_vfo");
        let got = radio.get_vfo_a().await.expect("get_vfo_a after recall");
        assert_eq!(got.hz(), 28_500_000, "VFO A mismatch after store+recall");
    });
}

#[test]
fn test_memory_channel_up_and_down() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_memory_channel(10)
            .await
            .expect("set_memory_channel(10)");
        radio.memory_channel_up().await.expect("memory_channel_up");
        let up = radio
            .get_memory_channel()
            .await
            .expect("get_memory_channel after up");
        assert_eq!(
            up, 11,
            "expected channel 11 after memory_channel_up from 10"
        );

        radio
            .memory_channel_down()
            .await
            .expect("memory_channel_down");
        let down = radio
            .get_memory_channel()
            .await
            .expect("get_memory_channel after down");
        assert_eq!(
            down, 10,
            "expected channel 10 after memory_channel_down from 11"
        );
    });
}

// ---------------------------------------------------------------------------
// Clarifier / RIT-XIT (RT RC RD RU XT)
// ---------------------------------------------------------------------------

#[test]
fn test_rx_clarifier_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_rx_clarifier_on(true)
            .await
            .expect("set_rx_clarifier_on(true)");
        let v = radio
            .get_rx_clarifier_on()
            .await
            .expect("get_rx_clarifier_on");
        assert!(v, "expected RX clarifier on after set");
        radio
            .set_rx_clarifier_on(false)
            .await
            .expect("set_rx_clarifier_on(false)");
        let v = radio
            .get_rx_clarifier_on()
            .await
            .expect("get_rx_clarifier_on after clear");
        assert!(!v, "expected RX clarifier off after clear");
    });
}

#[test]
fn test_tx_clarifier_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_tx_clarifier_on(true)
            .await
            .expect("set_tx_clarifier_on(true)");
        let v = radio
            .get_tx_clarifier_on()
            .await
            .expect("get_tx_clarifier_on");
        assert!(v, "expected TX clarifier on after set");
    });
}

#[test]
fn test_clarifier_down_then_up_then_clear() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .clarifier_down(500)
            .await
            .expect("clarifier_down(500)");
        radio.clarifier_up(200).await.expect("clarifier_up(200)");
        radio.clarifier_clear().await.expect("clarifier_clear");
        // No direct getter for the raw offset on `Radio` (only via `IF`),
        // so this is a smoke test that all three calls succeed cleanly in
        // sequence -- confirmed against `IF`'s clarifier_offset_hz field.
        let info = radio.get_information().await.expect("get_information");
        assert_eq!(info.clarifier_offset_hz, 0, "expected offset cleared to 0");
    });
}

// ---------------------------------------------------------------------------
// IF-shift (IS)
// ---------------------------------------------------------------------------

#[test]
fn test_if_shift_hz_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_if_shift_hz(400)
            .await
            .expect("set_if_shift_hz(400)");
        let v = radio.get_if_shift_hz().await.expect("get_if_shift_hz");
        assert_eq!(v, 400, "if_shift mismatch after set");

        radio
            .set_if_shift_hz(-400)
            .await
            .expect("set_if_shift_hz(-400)");
        let v = radio
            .get_if_shift_hz()
            .await
            .expect("get_if_shift_hz negative");
        assert_eq!(v, -400, "if_shift mismatch after negative set");
    });
}

// ---------------------------------------------------------------------------
// Tone squelch mode + CTCSS/DCS (CT CN)
// ---------------------------------------------------------------------------

#[test]
fn test_tone_squelch_mode_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_tone_squelch_mode(ToneSquelchMode::CtcssEncDec)
            .await
            .expect("set_tone_squelch_mode");
        let v = radio
            .get_tone_squelch_mode()
            .await
            .expect("get_tone_squelch_mode");
        assert_eq!(
            v,
            ToneSquelchMode::CtcssEncDec,
            "tone squelch mode mismatch after set"
        );
    });
}

#[test]
fn test_ctcss_tone_hz_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let hz = CTCSS_TONES_DECIHZ[0] as f32 / 10.0;
        radio
            .set_ctcss_tone_hz(hz)
            .await
            .expect("set_ctcss_tone_hz");
        let got = radio.get_ctcss_tone_hz().await.expect("get_ctcss_tone_hz");
        assert!(
            (got - hz).abs() < 0.05,
            "CTCSS tone mismatch: got {got}, expected {hz}"
        );
    });
}

#[test]
fn test_dcs_code_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let code = DCS_CODES[0];
        radio.set_dcs_code(code).await.expect("set_dcs_code");
        let got = radio.get_dcs_code().await.expect("get_dcs_code");
        assert_eq!(got, code, "DCS code mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// CW keyer speed/pitch/on-off, break-in, CW spot, zero-in (KM KP KR KS KY CS
// ZI BI SD -- KM/KY are Ft991aExtras, tested in that section below)
// ---------------------------------------------------------------------------

#[test]
fn test_break_in_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_break_in_on(true)
            .await
            .expect("set_break_in_on(true)");
        let v = radio.get_break_in_on().await.expect("get_break_in_on");
        assert!(v, "expected break-in on after set");
    });
}

#[test]
fn test_semi_break_in_delay_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_semi_break_in_delay(500)
            .await
            .expect("set_semi_break_in_delay(500)");
        let v = radio
            .get_semi_break_in_delay()
            .await
            .expect("get_semi_break_in_delay");
        assert_eq!(v, 500, "semi break-in delay mismatch after set");
    });
}

#[test]
fn test_cw_spot_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_cw_spot_on(true)
            .await
            .expect("set_cw_spot_on(true)");
        let v = radio.get_cw_spot_on().await.expect("get_cw_spot_on");
        assert!(v, "expected CW spot on after set");
    });
}

#[test]
fn test_keyer_enabled_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_keyer_enabled(false)
            .await
            .expect("set_keyer_enabled(false)");
        let v = radio.get_keyer_enabled().await.expect("get_keyer_enabled");
        assert!(!v, "expected keyer disabled after set");
    });
}

#[test]
fn test_keyer_speed_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_keyer_speed(25)
            .await
            .expect("set_keyer_speed(25)");
        let v = radio.get_keyer_speed().await.expect("get_keyer_speed");
        assert_eq!(v, 25, "keyer speed mismatch after set");
    });
}

#[test]
fn test_keyer_pitch_hz_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_keyer_pitch_hz(700)
            .await
            .expect("set_keyer_pitch_hz(700)");
        let v = radio
            .get_keyer_pitch_hz()
            .await
            .expect("get_keyer_pitch_hz");
        assert_eq!(v, 700, "keyer pitch mismatch after set");
    });
}

#[test]
fn test_zero_in() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.zero_in().await.expect("zero_in");
    });
}

// ---------------------------------------------------------------------------
// Scan / VOX / busy (SC VX VD VG BY)
// ---------------------------------------------------------------------------

#[test]
fn test_scan_state_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_scan_state(ScanState::Up)
            .await
            .expect("set_scan_state(Up)");
        let v = radio.get_scan_state().await.expect("get_scan_state");
        assert_eq!(v, ScanState::Up, "scan state mismatch after set");
        radio
            .set_scan_state(ScanState::Off)
            .await
            .expect("set_scan_state(Off)");
    });
}

#[test]
fn test_vox_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_vox_on(true).await.expect("set_vox_on(true)");
        let v = radio.get_vox_on().await.expect("get_vox_on");
        assert!(v, "expected VOX on after set");
    });
}

#[test]
fn test_vox_gain_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_vox_gain(60).await.expect("set_vox_gain(60)");
        let v = radio.get_vox_gain().await.expect("get_vox_gain");
        assert_eq!(v, 60, "VOX gain mismatch after set");
    });
}

#[test]
fn test_vox_delay_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_vox_delay(200).await.expect("set_vox_delay(200)");
        let v = radio.get_vox_delay().await.expect("get_vox_delay");
        assert_eq!(v, 200, "VOX delay mismatch after set");
    });
}

#[test]
fn test_get_rx_busy_always_false() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_rx_busy().await.expect("get_rx_busy");
        // Documented emulator behavior: no simulated squelch-open
        // condition, so this always reports false (see `Radio::
        // get_rx_busy`'s doc comment).
        assert!(
            !v,
            "expected rx_busy=false (documented emulator simplification)"
        );
    });
}

// ---------------------------------------------------------------------------
// Attenuator / preamp / noise / AGC / notch / filter-width
// (RA PA NB NL NR RL GT BC NA SH)
// ---------------------------------------------------------------------------

#[test]
fn test_attenuator_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_attenuator_on(true)
            .await
            .expect("set_attenuator_on(true)");
        let v = radio.get_attenuator_on().await.expect("get_attenuator_on");
        assert!(v, "expected attenuator on after set");
    });
}

#[test]
fn test_preamp_mode_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_preamp_mode(PreampMode::Amp1)
            .await
            .expect("set_preamp_mode(Amp1)");
        let v = radio.get_preamp_mode().await.expect("get_preamp_mode");
        assert_eq!(v, PreampMode::Amp1, "preamp mode mismatch after set");
    });
}

#[test]
fn test_noise_blanker_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_noise_blanker_on(true)
            .await
            .expect("set_noise_blanker_on(true)");
        let v = radio
            .get_noise_blanker_on()
            .await
            .expect("get_noise_blanker_on");
        assert!(v, "expected noise blanker on after set");
    });
}

#[test]
fn test_noise_blanker_level_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_noise_blanker_level(7)
            .await
            .expect("set_noise_blanker_level(7)");
        let v = radio
            .get_noise_blanker_level()
            .await
            .expect("get_noise_blanker_level");
        assert_eq!(v, 7, "noise blanker level mismatch after set");
    });
}

#[test]
fn test_noise_reduction_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_noise_reduction_on(true)
            .await
            .expect("set_noise_reduction_on(true)");
        let v = radio
            .get_noise_reduction_on()
            .await
            .expect("get_noise_reduction_on");
        assert!(v, "expected noise reduction on after set");
    });
}

#[test]
fn test_noise_reduction_level_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_noise_reduction_level(10)
            .await
            .expect("set_noise_reduction_level(10)");
        let v = radio
            .get_noise_reduction_level()
            .await
            .expect("get_noise_reduction_level");
        assert_eq!(v, 10, "noise reduction level mismatch after set");
    });
}

#[test]
fn test_agc_mode_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_agc_mode(radio::AgcMode::Fast)
            .await
            .expect("set_agc_mode(Fast)");
        let v = radio.get_agc_mode().await.expect("get_agc_mode");
        assert_eq!(v, radio::AgcMode::Fast, "AGC mode mismatch after set");
    });
}

#[test]
fn test_auto_notch_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_auto_notch_on(true)
            .await
            .expect("set_auto_notch_on(true)");
        let v = radio.get_auto_notch_on().await.expect("get_auto_notch_on");
        assert!(v, "expected auto notch on after set");
    });
}

#[test]
fn test_narrow_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_narrow_on(true)
            .await
            .expect("set_narrow_on(true)");
        let v = radio.get_narrow_on().await.expect("get_narrow_on");
        assert!(v, "expected narrow filter on after set");
    });
}

#[test]
fn test_filter_width_index_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_filter_width_index(5)
            .await
            .expect("set_filter_width_index(5)");
        let v = radio
            .get_filter_width_index()
            .await
            .expect("get_filter_width_index");
        assert_eq!(v, 5, "filter width index mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Speech processor / mic / monitor (MG PL PR ML)
// ---------------------------------------------------------------------------

#[test]
fn test_mic_gain_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_mic_gain(80).await.expect("set_mic_gain(80)");
        let v = radio.get_mic_gain().await.expect("get_mic_gain");
        assert_eq!(v, 80, "mic gain mismatch after set");
    });
}

#[test]
fn test_speech_processor_level_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_speech_processor_level(60)
            .await
            .expect("set_speech_processor_level(60)");
        let v = radio
            .get_speech_processor_level()
            .await
            .expect("get_speech_processor_level");
        assert_eq!(v, 60, "speech processor level mismatch after set");
    });
}

#[test]
fn test_speech_processor_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_speech_processor_on(true)
            .await
            .expect("set_speech_processor_on(true)");
        let v = radio
            .get_speech_processor_on()
            .await
            .expect("get_speech_processor_on");
        assert!(v, "expected speech processor on after set");
    });
}

#[test]
fn test_monitor_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_monitor_on(true)
            .await
            .expect("set_monitor_on(true)");
        let v = radio.get_monitor_on().await.expect("get_monitor_on");
        assert!(v, "expected monitor on after set");
    });
}

#[test]
fn test_monitor_level_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_monitor_level(70)
            .await
            .expect("set_monitor_level(70)");
        let v = radio.get_monitor_level().await.expect("get_monitor_level");
        assert_eq!(v, 70, "monitor level mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Band/step/encoder front-panel controls (BS BU BD FS DN UP)
// ---------------------------------------------------------------------------

#[test]
fn test_set_band() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_band(Band::FourteenMHz)
            .await
            .expect("set_band(FourteenMHz)");
    });
}

#[test]
fn test_band_up_and_down() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.band_up().await.expect("band_up");
        radio.band_down().await.expect("band_down");
    });
}

#[test]
fn test_fine_step_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_fine_step(true)
            .await
            .expect("set_fine_step(true)");
        let v = radio.get_fine_step().await.expect("get_fine_step");
        assert!(v, "expected fine step on after set");
    });
}

#[test]
fn test_mic_up_and_down() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.mic_up().await.expect("mic_up");
        radio.mic_down().await.expect("mic_down");
    });
}

// ---------------------------------------------------------------------------
// Misc system: auto-info, freq lock, repeater shift, TX VFO, MOX
// (AI LK OS FT MX)
// ---------------------------------------------------------------------------

#[test]
fn test_auto_info_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_auto_info_on(true)
            .await
            .expect("set_auto_info_on(true)");
        let v = radio.get_auto_info_on().await.expect("get_auto_info_on");
        assert!(v, "expected auto-info on after set");
    });
}

#[test]
fn test_frequency_lock_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_frequency_lock(true)
            .await
            .expect("set_frequency_lock(true)");
        let v = radio
            .get_frequency_lock()
            .await
            .expect("get_frequency_lock");
        assert!(v, "expected frequency lock on after set");
    });
}

#[test]
fn test_repeater_shift_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_repeater_shift(RepeaterShift::Plus)
            .await
            .expect("set_repeater_shift(Plus)");
        let v = radio
            .get_repeater_shift()
            .await
            .expect("get_repeater_shift");
        assert_eq!(v, RepeaterShift::Plus, "repeater shift mismatch after set");
    });
}

#[test]
fn test_tx_vfo_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_tx_vfo(1).await.expect("set_tx_vfo(1)");
        let v = radio.get_tx_vfo().await.expect("get_tx_vfo");
        assert_eq!(v, 1, "TX VFO mismatch after set");
    });
}

#[test]
fn test_mox_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_mox_on(true).await.expect("set_mox_on(true)");
        let v = radio.get_mox_on().await.expect("get_mox_on");
        assert!(v, "expected MOX on after set");
    });
}

// ---------------------------------------------------------------------------
// Ft991aExtras: composite status payloads (IF, OI)
// ---------------------------------------------------------------------------

#[test]
fn test_get_information() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let info = radio.get_information().await.expect("get_information");
        assert_eq!(
            info.frequency_hz, 14_000_000,
            "IF frequency should match VFO A default"
        );
    });
}

#[test]
fn test_get_opposite_band_information() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let info: ChannelStatusFields = radio
            .get_opposite_band_information()
            .await
            .expect("get_opposite_band_information");
        assert_eq!(
            info.frequency_hz, 14_100_000,
            "OI frequency should match VFO B default"
        );
    });
}

// ---------------------------------------------------------------------------
// Ft991aExtras: meters/status (RM direct, RI, RS, UL)
// ---------------------------------------------------------------------------

#[test]
fn test_get_active_meter_reading() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .get_active_meter_reading()
            .await
            .expect("get_active_meter_reading");
    });
}

#[test]
fn test_get_radio_indicator_every_variant() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        for indicator in [
            RadioIndicator::HiSwr,
            RadioIndicator::Rec,
            RadioIndicator::Play,
            RadioIndicator::VfoATx,
            RadioIndicator::VfoBTx,
            RadioIndicator::VfoARx,
            RadioIndicator::TxLed,
        ] {
            radio
                .get_radio_indicator(indicator)
                .await
                .unwrap_or_else(|e| panic!("get_radio_indicator({indicator:?}) failed: {e}"));
        }
    });
}

#[test]
fn test_get_menu_mode_active() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio
            .get_menu_mode_active()
            .await
            .expect("get_menu_mode_active");
        assert!(!v, "expected menu mode inactive by default");
    });
}

#[test]
fn test_get_pll_unlocked() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let v = radio.get_pll_unlocked().await.expect("get_pll_unlocked");
        assert!(!v, "expected PLL locked (not unlocked) by default");
    });
}

// ---------------------------------------------------------------------------
// Ft991aExtras: VFO/memory quick-ops with no generic-Radio home
// (VM, QI, QR, QS)
// ---------------------------------------------------------------------------

#[test]
fn test_toggle_vfo_memory_mode() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .toggle_vfo_memory_mode()
            .await
            .expect("toggle_vfo_memory_mode");
    });
}

#[test]
fn test_qmb_store_then_recall() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let target = Frequency::new(18_100_000).expect("Frequency::new");
        radio.set_vfo_a(target).await.expect("set_vfo_a");
        radio.qmb_store().await.expect("qmb_store");

        radio
            .set_vfo_a(Frequency::new(7_000_000).unwrap())
            .await
            .expect("set_vfo_a (perturb)");
        radio.qmb_recall().await.expect("qmb_recall");
        let got = radio.get_vfo_a().await.expect("get_vfo_a after qmb_recall");
        assert_eq!(
            got.hz(),
            18_100_000,
            "VFO A mismatch after qmb_store+qmb_recall"
        );
    });
}

#[test]
fn test_quick_split() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.quick_split().await.expect("quick_split");
    });
}

// ---------------------------------------------------------------------------
// Ft991aExtras: keyer memory store/playback (KM, KY)
// ---------------------------------------------------------------------------

#[test]
fn test_write_then_read_keyer_memory() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .write_keyer_memory(1, "CQ CQ DE TEST")
            .await
            .expect("write_keyer_memory");
        let msg = radio.read_keyer_memory(1).await.expect("read_keyer_memory");
        assert_eq!(
            msg, "CQ CQ DE TEST",
            "keyer memory message mismatch after write"
        );
    });
}

#[test]
fn test_play_keyer_memory() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .write_keyer_memory(1, "TEST")
            .await
            .expect("write_keyer_memory");
        radio
            .play_keyer_memory(1, KeyerPlaybackMode::KeyerMemory)
            .await
            .expect("play_keyer_memory");
    });
}

// ---------------------------------------------------------------------------
// Ft991aExtras: Contour/APF (CO) and manual notch (BP)
// ---------------------------------------------------------------------------

#[test]
fn test_contour_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_contour_on(true)
            .await
            .expect("set_contour_on(true)");
        let v = radio.get_contour_on().await.expect("get_contour_on");
        assert!(v, "expected contour on after set");
    });
}

#[test]
fn test_contour_frequency_hz_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_contour_frequency_hz(1500)
            .await
            .expect("set_contour_frequency_hz(1500)");
        let v = radio
            .get_contour_frequency_hz()
            .await
            .expect("get_contour_frequency_hz");
        assert_eq!(v, 1500, "contour frequency mismatch after set");
    });
}

#[test]
fn test_apf_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_apf_on(true).await.expect("set_apf_on(true)");
        let v = radio.get_apf_on().await.expect("get_apf_on");
        assert!(v, "expected APF on after set");
    });
}

#[test]
fn test_apf_frequency_hz_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_apf_frequency_hz(100)
            .await
            .expect("set_apf_frequency_hz(100)");
        let v = radio
            .get_apf_frequency_hz()
            .await
            .expect("get_apf_frequency_hz");
        assert_eq!(v, 100, "APF frequency mismatch after set");
    });
}

#[test]
fn test_manual_notch_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_manual_notch_on(true)
            .await
            .expect("set_manual_notch_on(true)");
        let v = radio
            .get_manual_notch_on()
            .await
            .expect("get_manual_notch_on");
        assert!(v, "expected manual notch on after set");
    });
}

#[test]
fn test_manual_notch_frequency_hz_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_manual_notch_frequency_hz(1200)
            .await
            .expect("set_manual_notch_frequency_hz(1200)");
        let v = radio
            .get_manual_notch_frequency_hz()
            .await
            .expect("get_manual_notch_frequency_hz");
        assert_eq!(v, 1200, "manual notch frequency mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Ft991aExtras: Parametric Microphone Equalizer (PR P1=1)
// ---------------------------------------------------------------------------

#[test]
fn test_parametric_mic_eq_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_parametric_mic_eq_on(true)
            .await
            .expect("set_parametric_mic_eq_on(true)");
        let v = radio
            .get_parametric_mic_eq_on()
            .await
            .expect("get_parametric_mic_eq_on");
        assert!(v, "expected parametric mic EQ on after set");
    });
}

// ---------------------------------------------------------------------------
// Ft991aExtras: front-panel encoder/key emulation (ED, EU, EK)
// ---------------------------------------------------------------------------

#[test]
fn test_encoder_down_and_up() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .encoder_down(EncoderSelector::Main, 5)
            .await
            .expect("encoder_down");
        radio
            .encoder_up(EncoderSelector::Main, 5)
            .await
            .expect("encoder_up");
    });
}

#[test]
fn test_ent_key() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.ent_key().await.expect("ent_key");
    });
}

// ---------------------------------------------------------------------------
// Ft991aExtras: antenna tuner (AC)
// ---------------------------------------------------------------------------

#[test]
fn test_antenna_tuner_state_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .set_antenna_tuner_state(1)
            .await
            .expect("set_antenna_tuner_state(1)");
        let v = radio
            .get_antenna_tuner_state()
            .await
            .expect("get_antenna_tuner_state");
        assert_eq!(v, 1, "antenna tuner state mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Ft991aExtras: dimmer (DA)
// ---------------------------------------------------------------------------

#[test]
fn test_dimmer_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_dimmer(2, 10).await.expect("set_dimmer(2, 10)");
        let (led, tft) = radio.get_dimmer().await.expect("get_dimmer");
        assert_eq!((led, tft), (2, 10), "dimmer mismatch after set");
    });
}

// ---------------------------------------------------------------------------
// Ft991aExtras: date/time/time-zone (DT)
// ---------------------------------------------------------------------------

#[test]
fn test_date_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.write_date(2026, 7, 26).await.expect("write_date");
        let (y, m, d) = radio.read_date().await.expect("read_date");
        assert_eq!((y, m, d), (2026, 7, 26), "date mismatch after write");
    });
}

#[test]
fn test_time_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.write_time(13, 45, 30).await.expect("write_time");
        let (h, m, s) = radio.read_time().await.expect("read_time");
        assert_eq!((h, m, s), (13, 45, 30), "time mismatch after write");
    });
}

#[test]
fn test_time_zone_offset_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .write_time_zone_offset(-300)
            .await
            .expect("write_time_zone_offset(-300)");
        let v = radio
            .read_time_zone_offset()
            .await
            .expect("read_time_zone_offset");
        assert_eq!(v, -300, "time zone offset mismatch after write");
    });
}

// ---------------------------------------------------------------------------
// Ft991aExtras: "TXW" (TS)
// ---------------------------------------------------------------------------

#[test]
fn test_txw_on_round_trips() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio.set_txw_on(true).await.expect("set_txw_on(true)");
        let v = radio.get_txw_on().await.expect("get_txw_on");
        assert!(v, "expected TXW on after set");
    });
}

// ---------------------------------------------------------------------------
// Ft991aExtras: DVS record/playback (LM, PB)
// ---------------------------------------------------------------------------

#[test]
fn test_dvs_recording_start_reports_channel_then_stop_reports_none() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .start_dvs_recording(2)
            .await
            .expect("start_dvs_recording(2)");
        let ch = radio
            .get_dvs_recording_channel()
            .await
            .expect("get_dvs_recording_channel");
        assert_eq!(ch, Some(2), "expected recording channel 2 active");

        radio
            .stop_dvs_recording()
            .await
            .expect("stop_dvs_recording");
        let ch = radio
            .get_dvs_recording_channel()
            .await
            .expect("get_dvs_recording_channel after stop");
        assert_eq!(ch, None, "expected recording stopped");
    });
}

#[test]
fn test_dvs_playback_start_reports_channel_then_stop_reports_none() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        radio
            .start_dvs_playback(3)
            .await
            .expect("start_dvs_playback(3)");
        let ch = radio
            .get_dvs_playback_channel()
            .await
            .expect("get_dvs_playback_channel");
        assert_eq!(ch, Some(3), "expected playback channel 3 active");

        radio.stop_dvs_playback().await.expect("stop_dvs_playback");
        let ch = radio
            .get_dvs_playback_channel()
            .await
            .expect("get_dvs_playback_channel after stop");
        assert_eq!(ch, None, "expected playback stopped");
    });
}

// ---------------------------------------------------------------------------
// Ft991aExtras: EX menu escape hatch -- one generic, table-driven test over
// every landed EX_MENU_TABLE row, rather than 151 hand-written functions
// (this menu has no ts570d equivalent to mirror the per-command style of).
// ---------------------------------------------------------------------------

/// Compute two distinct, legal wire values for `item` to round-trip against
/// the emulator -- generically, from the item's own [`radio::ExMenuValueKind`],
/// with no per-item special-casing.
fn two_legal_values(item: &radio::ExMenuItem) -> (i32, i32) {
    match item.kind {
        ExMenuValueKind::Enumerated(pairs) => {
            let first: i32 = pairs[0].0.parse().unwrap_or_else(|_| {
                panic!(
                    "EX item {} ({}): non-numeric wire value {:?}",
                    item.p1, item.name, pairs[0]
                )
            });
            if pairs.len() > 1 {
                let second: i32 = pairs[1].0.parse().unwrap_or_else(|_| {
                    panic!(
                        "EX item {} ({}): non-numeric wire value {:?}",
                        item.p1, item.name, pairs[1]
                    )
                });
                (first, second)
            } else {
                (first, first)
            }
        }
        ExMenuValueKind::Range { min, max, step, .. } => {
            let second = if min + step <= max { min + step } else { max };
            (min, second)
        }
    }
}

#[test]
fn ex_menu_round_trips_every_landed_item() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        for item in EX_MENU_TABLE.iter() {
            let (first, second) = two_legal_values(item);

            radio
                .set_ex_menu_item(item.p1, first)
                .await
                .unwrap_or_else(|e| {
                    panic!(
                        "EX item {:03} ({}): set_ex_menu_item({first}) failed: {e}",
                        item.p1, item.name
                    )
                });
            let got = radio.get_ex_menu_item(item.p1).await.unwrap_or_else(|e| {
                panic!(
                    "EX item {:03} ({}): get_ex_menu_item failed: {e}",
                    item.p1, item.name
                )
            });
            assert_eq!(
                got, first,
                "EX item {:03} ({}): round-trip mismatch for value {first}",
                item.p1, item.name
            );

            if second != first {
                radio
                    .set_ex_menu_item(item.p1, second)
                    .await
                    .unwrap_or_else(|e| {
                        panic!(
                            "EX item {:03} ({}): set_ex_menu_item({second}) failed: {e}",
                            item.p1, item.name
                        )
                    });
                let got = radio.get_ex_menu_item(item.p1).await.unwrap_or_else(|e| {
                    panic!(
                        "EX item {:03} ({}): get_ex_menu_item (2nd value) failed: {e}",
                        item.p1, item.name
                    )
                });
                assert_eq!(
                    got, second,
                    "EX item {:03} ({}): round-trip mismatch for second value {second}",
                    item.p1, item.name
                );
            }
        }
    });
}

#[test]
fn ex_menu_rejects_unknown_item() {
    let slave = start_emulator();
    async_test!(async move {
        let mut radio = open_radio(&slave);
        let err = radio
            .get_ex_menu_item(999)
            .await
            .expect_err("P1=999 is not a landed EX_MENU_TABLE row");
        assert!(
            matches!(err, radio::RadioError::UnknownExMenuItem(999)),
            "expected UnknownExMenuItem(999), got {err:?}"
        );
    });
}
