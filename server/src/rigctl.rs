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

//! A Hamlib rigctld-compatible TCP listener, for WSJT-X's "Hamlib NET
//! rigctl" rig type (`planning/architect/task_plan.md` §12.2).
//!
//! **Not validated against a real WSJT-X instance in this session** (no
//! WSJT-X available in this sandbox). The command subset below (the short
//! single-letter commands `f`/`F`/`m`/`M`/`t`/`T`/`v`, plus `\dump_state`
//! and `\chk_vfo`) is what Hamlib's `netrigctl.c` backend actually issues —
//! this is not the same wire text as interactive `rigctl`'s long-form
//! `get_freq`/`set_freq`/... commands, which a human types at a REPL, not
//! what `netrigctl.c` sends automatically. `\dump_state`'s exact field
//! layout (frequency/mode/vfo range rows, tuning steps, filters, then a
//! fixed tail of capability numbers) was reconstructed from public
//! documentation of Hamlib's `rigctld.c`/`netrigctl.c` protocol, not from a
//! byte-for-byte spec transcription the way `EX_MENU_TABLE` was built from
//! the FT-991A manual — **treat this as a first cut to validate/iterate
//! against real WSJT-X**, especially if the initial connection handshake
//! fails (Hamlib's `rig_open()` calls `dump_state` and can abort the whole
//! connection if it can't parse the reply). Mode/range bitmasks are
//! deliberately permissive (`-1`, "any mode") rather than an attempt at
//! Hamlib's exact per-mode bit assignments, specifically to avoid a wrong
//! narrow mask silently rejecting operations WSJT-X needs.
//!
//! Also out of scope, deliberately: split VFO (`s`/`S`) and VFO selection
//! (`V`) are not backed by anything in `radio::Radio`/`Ft991aExtras` today,
//! so they are not implemented rather than silently faked — `RPRT -1`
//! (this bridge's one generic failure code — see [`RPRT_ERR`]) tells the
//! client honestly that the command isn't supported, rather than reporting
//! success for something that didn't happen.
//!
//! Delegates to `radio::Ft991a`'s existing typed methods via
//! [`crate::broker_session::BrokerCatSession`] — this module never
//! constructs a raw FT-991A wire frame itself.

use std::io;

use monoio::io::{AsyncReadRent, AsyncWriteRentExt};
use monoio::net::{TcpListener, TcpStream};

use cat_server::{BrokerHandle, ClientId};
use cat_transport_core::{CatSession, TransportError};
use radio::{Frequency, Ft991a, Mode, TxState};

use crate::broker_session::BrokerCatSession;

/// Accept loop, mirroring `cat_server::tcp::serve`'s shape: binding is the
/// caller's responsibility, one task per accepted connection, runs until
/// `accept()` itself fails.
pub async fn serve(listener: TcpListener, handle: BrokerHandle) -> io::Result<()> {
    let mut next_client_id: u64 = 0;
    loop {
        let (stream, _peer_addr) = listener.accept().await?;
        let client_id = ClientId::from_raw(next_client_id);
        next_client_id = next_client_id.wrapping_add(1);
        let handle = handle.clone();
        monoio::spawn(handle_connection(stream, handle, client_id));
    }
}

async fn handle_connection(mut stream: TcpStream, handle: BrokerHandle, client_id: ClientId) {
    let mut radio = Ft991a::new(BrokerCatSession::new(handle, client_id));
    let mut reader = LineReader::new();

    loop {
        let line = match reader.read_line(&mut stream).await {
            Ok(Some(line)) => line,
            Ok(None) | Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.eq_ignore_ascii_case("q") {
            break;
        }

        let response = dispatch(&mut radio, trimmed).await;
        let (result, _buf) = stream.write_all(response.into_bytes()).await;
        if result.is_err() {
            break;
        }
    }
}

/// A rigctld error report line: `RPRT <code>`. `-1` is used for every
/// failure here (this bridge does not attempt to reproduce Hamlib's
/// specific per-cause negative error codes — WSJT-X's own error handling
/// only distinguishes zero from non-zero).
const RPRT_OK: &str = "RPRT 0\n";
const RPRT_ERR: &str = "RPRT -1\n";

/// Dispatch one rigctld command line against `radio`, returning the full
/// response text (already newline-terminated). Generic over the same
/// `CatSession<Error = TransportError>` bound `Ft991a`'s own impl blocks
/// use, so this works against a real `BrokerCatSession` in production and
/// a `ScriptedCatSession` in tests without any test-specific branching.
async fn dispatch<S>(radio: &mut Ft991a<S>, line: &str) -> String
where
    S: CatSession<Error = TransportError>,
{
    let mut parts = line.split_whitespace();
    let Some(cmd) = parts.next() else {
        return RPRT_ERR.to_string();
    };
    let args: Vec<&str> = parts.collect();

    match cmd {
        "f" => match radio.get_vfo_a().await {
            Ok(freq) => format!("{}\n", freq.hz()),
            Err(_) => RPRT_ERR.to_string(),
        },
        "F" => {
            let Some(hz) = args.first().and_then(|s| s.parse::<u64>().ok()) else {
                return RPRT_ERR.to_string();
            };
            match Frequency::new(hz) {
                Ok(freq) => match radio.set_vfo_a(freq).await {
                    Ok(()) => RPRT_OK.to_string(),
                    Err(_) => RPRT_ERR.to_string(),
                },
                Err(_) => RPRT_ERR.to_string(),
            }
        }
        "m" => match radio.get_mode().await {
            // Passband is always reported as `0` ("use the rig's current
            // default") rather than a real bandwidth — see module docs on
            // why filter-width resolution is out of scope for this bridge.
            Ok(mode) => format!("{}\n0\n", hamlib_mode_name(mode)),
            Err(_) => RPRT_ERR.to_string(),
        },
        "M" => {
            let Some(mode_name) = args.first() else {
                return RPRT_ERR.to_string();
            };
            match hamlib_mode_from_name(mode_name) {
                Some(mode) => match radio.set_mode(mode).await {
                    Ok(()) => RPRT_OK.to_string(),
                    Err(_) => RPRT_ERR.to_string(),
                },
                None => RPRT_ERR.to_string(),
            }
        }
        "t" => match radio.get_tx_state().await {
            Ok(TxState::Off) => "0\n".to_string(),
            Ok(_) => "1\n".to_string(),
            Err(_) => RPRT_ERR.to_string(),
        },
        "T" => {
            let result = match args.first() {
                Some(&"1") => radio.transmit().await,
                Some(&"0") => radio.receive().await,
                _ => return RPRT_ERR.to_string(),
            };
            match result {
                Ok(()) => RPRT_OK.to_string(),
                Err(_) => RPRT_ERR.to_string(),
            }
        }
        // No VFO-B/split concept on `radio::Radio` today — always report
        // the single VFO this bridge controls (see module docs).
        "v" => "VFOA\n".to_string(),
        "\\chk_vfo" => "0\n".to_string(),
        "\\dump_state" => dump_state(),
        _ => RPRT_ERR.to_string(),
    }
}

/// Map a [`Mode`] to the Hamlib rig-mode name `m`/`M` exchange on the wire.
/// Best-effort for modes with no exact Hamlib counterpart (`C4fm`, `AmN`) —
/// documented per-arm below, not silently assumed correct.
fn hamlib_mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Lsb => "LSB",
        Mode::Usb => "USB",
        Mode::CwU => "CW",
        Mode::CwL => "CWR",
        Mode::Fm => "FM",
        Mode::FmN => "FMN",
        Mode::Am => "AM",
        // Hamlib has no distinct narrow-AM mode name in common use; `AM` is
        // the closest match.
        Mode::AmN => "AM",
        Mode::RttyLsb => "RTTY",
        Mode::RttyUsb => "RTTYR",
        Mode::DataLsb => "PKTLSB",
        Mode::DataUsb => "PKTUSB",
        Mode::DataFm => "PKTFM",
        // No Hamlib equivalent for Yaesu's C4FM digital voice mode; `USB`
        // is a safe, inert fallback (never actually selected by a WSJT-X
        // user, who has no reason to request C4FM over this bridge).
        Mode::C4fm => "USB",
    }
}

fn hamlib_mode_from_name(name: &str) -> Option<Mode> {
    match name.to_ascii_uppercase().as_str() {
        "LSB" => Some(Mode::Lsb),
        "USB" => Some(Mode::Usb),
        "CW" => Some(Mode::CwU),
        "CWR" => Some(Mode::CwL),
        "FM" => Some(Mode::Fm),
        "FMN" => Some(Mode::FmN),
        "AM" => Some(Mode::Am),
        "RTTY" => Some(Mode::RttyLsb),
        "RTTYR" => Some(Mode::RttyUsb),
        "PKTLSB" => Some(Mode::DataLsb),
        "PKTUSB" => Some(Mode::DataUsb),
        "PKTFM" => Some(Mode::DataFm),
        _ => None,
    }
}

/// The `\dump_state` capability handshake Hamlib's `netrigctl.c` client
/// sends once, right after connecting — see this module's doc comment for
/// the "not validated against real WSJT-X" caveat. Frequency range drawn
/// from [`Frequency::MIN_HZ`]/[`Frequency::MAX_HZ`] (real FT-991A-cited
/// values, unlike the invented mode/vfo/antenna bitmasks below).
fn dump_state() -> String {
    let mut s = String::new();
    // Protocol marker, rig model (0 = generic/unknown), ITU region (0 =
    // unspecified) — the three fixed header lines every `dump_state` reply
    // starts with.
    s.push_str("0\n0\n0\n");

    // RX range list: one row of `start end modes low_power high_power vfo
    // ant`, terminated by an all-zero sentinel row. `modes`/`vfo`/`ant` use
    // `-1` (all bits set) rather than an attempt at Hamlib's exact per-mode
    // bit assignments — deliberately permissive, see module docs.
    s.push_str(&format!(
        "{} {} -1 -1 -1 -1 -1\n0 0 0 0 0 0 0\n",
        Frequency::MIN_HZ,
        Frequency::MAX_HZ
    ));
    // TX range list — same shape, terminated the same way.
    s.push_str(&format!(
        "{} {} -1 -1 -1 -1 -1\n0 0 0 0 0 0 0\n",
        Frequency::MIN_HZ,
        Frequency::MAX_HZ
    ));
    // Tuning steps: `modes step_size`, terminated by an all-zero row. `10`
    // Hz is the FT-991A's finest documented step.
    s.push_str("-1 10\n0 0\n");
    // Filters: `modes width`, terminated by an all-zero row. `2400` Hz is a
    // conservative, universally-legal SSB bandwidth (see
    // `radio::SH_BANDWIDTH_TABLE`) — a placeholder, not a per-mode table,
    // since this bridge doesn't resolve real filter widths (module docs).
    s.push_str("-1 2400\n0 0\n");

    // Fixed capability tail: max RIT/XIT/IF-shift (Hz), announces bitmask,
    // preamp levels list (dB, zero-terminated), attenuator levels list (dB,
    // zero-terminated), then four zero (hex) capability bitmasks —
    // `has_get_func`/`has_set_func`/`has_get_level`/`has_set_level`. This
    // bridge does not claim any Hamlib "func"/"level" capabilities beyond
    // plain freq/mode/ptt, so all four are `0`.
    s.push_str("1200\n0\n1200\n0\n0\n0\n0x0\n0x0\n0x0\n0x0\n");
    s
}

/// Buffers partial reads and splits them into `\n`-terminated lines (with
/// an optional trailing `\r` trimmed) — rigctld's protocol is line-based
/// text, not `cat-transport-tcp`'s length-prefixed binary framing, so
/// neither that crate's frame codec nor `cat-server::tcp`'s accept loop
/// apply here; this is new, minimal line-buffering built directly on
/// monoio's owned-buffer `AsyncReadRentExt::read`.
struct LineReader {
    buf: Vec<u8>,
}

impl LineReader {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Returns `Ok(Some(line))` (no trailing `\n`/`\r`) for one complete
    /// line, `Ok(None)` on a clean disconnect with no partial line pending,
    /// or `Err` on any I/O failure. A partial line still in the buffer when
    /// the peer disconnects is returned once as a final "line" (mirrors
    /// how a real terminal client's last unterminated command would still
    /// be worth attempting) rather than silently discarded.
    async fn read_line(&mut self, stream: &mut TcpStream) -> io::Result<Option<String>> {
        loop {
            if let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
                let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
                line.pop(); // trailing '\n'
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
            }

            let chunk = vec![0u8; 4096];
            let (result, chunk) = stream.read(chunk).await;
            let n = result?;
            if n == 0 {
                if self.buf.is_empty() {
                    return Ok(None);
                }
                let line = std::mem::take(&mut self.buf);
                return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cat_transport_core::test_support::ScriptedCatSession;

    fn radio_with(session: ScriptedCatSession) -> Ft991a<ScriptedCatSession> {
        Ft991a::new(session)
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_f_reports_current_frequency() {
        let session =
            ScriptedCatSession::with_script(vec![cat_transport_core::test_support::Exchange::new(
                "FA;",
                "FA014250000;",
            )]);
        let mut radio = radio_with(session);
        assert_eq!(dispatch(&mut radio, "f").await, "14250000\n");
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_capital_f_sets_frequency() {
        let session =
            ScriptedCatSession::with_script(vec![cat_transport_core::test_support::Exchange::new(
                "FA014250000;",
                "",
            )]);
        let mut radio = radio_with(session);
        assert_eq!(dispatch(&mut radio, "F 14250000").await, RPRT_OK);
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_capital_f_rejects_non_numeric_argument() {
        let mut radio = radio_with(ScriptedCatSession::new());
        assert_eq!(dispatch(&mut radio, "F not-a-number").await, RPRT_ERR);
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_m_reports_mode_and_placeholder_passband() {
        let session =
            ScriptedCatSession::with_script(vec![cat_transport_core::test_support::Exchange::new(
                "MD0;", "MD02;",
            )]);
        let mut radio = radio_with(session);
        assert_eq!(dispatch(&mut radio, "m").await, "USB\n0\n");
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_capital_m_sets_mode() {
        let session =
            ScriptedCatSession::with_script(vec![cat_transport_core::test_support::Exchange::new(
                "MD02;", "",
            )]);
        let mut radio = radio_with(session);
        assert_eq!(dispatch(&mut radio, "M USB 0").await, RPRT_OK);
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_capital_m_rejects_unknown_mode_name() {
        let mut radio = radio_with(ScriptedCatSession::new());
        assert_eq!(dispatch(&mut radio, "M BOGUS 0").await, RPRT_ERR);
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_t_reports_ptt_off() {
        let session =
            ScriptedCatSession::with_script(vec![cat_transport_core::test_support::Exchange::new(
                "TX;", "TX0;",
            )]);
        let mut radio = radio_with(session);
        assert_eq!(dispatch(&mut radio, "t").await, "0\n");
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_capital_t_one_transmits() {
        let session =
            ScriptedCatSession::with_script(vec![cat_transport_core::test_support::Exchange::new(
                "TX1;", "",
            )]);
        let mut radio = radio_with(session);
        assert_eq!(dispatch(&mut radio, "T 1").await, RPRT_OK);
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_capital_t_zero_receives() {
        let session =
            ScriptedCatSession::with_script(vec![cat_transport_core::test_support::Exchange::new(
                "TX0;", "",
            )]);
        let mut radio = radio_with(session);
        assert_eq!(dispatch(&mut radio, "T 0").await, RPRT_OK);
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_v_reports_vfo_a() {
        let mut radio = radio_with(ScriptedCatSession::new());
        assert_eq!(dispatch(&mut radio, "v").await, "VFOA\n");
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_chk_vfo_reports_zero() {
        let mut radio = radio_with(ScriptedCatSession::new());
        assert_eq!(dispatch(&mut radio, "\\chk_vfo").await, "0\n");
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_unknown_command_is_rprt_err() {
        let mut radio = radio_with(ScriptedCatSession::new());
        assert_eq!(dispatch(&mut radio, "bogus").await, RPRT_ERR);
    }

    #[monoio::test(driver = "legacy")]
    async fn dispatch_dump_state_ends_every_list_with_a_zero_sentinel_row() {
        let mut radio = radio_with(ScriptedCatSession::new());
        let state = dispatch(&mut radio, "\\dump_state").await;
        let lines: Vec<&str> = state.lines().collect();
        // 3 header lines + rx range (1 row + 1 sentinel) + tx range (1 + 1)
        // + tuning steps (1 + 1) + filters (1 + 1) + 10 tail lines.
        assert_eq!(lines.len(), 3 + 2 + 2 + 2 + 2 + 10);
        assert_eq!(lines[4], "0 0 0 0 0 0 0");
        assert_eq!(lines[6], "0 0 0 0 0 0 0");
        assert_eq!(lines[8], "0 0");
        assert_eq!(lines[10], "0 0");
    }

    #[test]
    fn hamlib_mode_round_trips_for_every_supported_mode() {
        for mode in [
            Mode::Lsb,
            Mode::Usb,
            Mode::CwU,
            Mode::CwL,
            Mode::Fm,
            Mode::FmN,
            Mode::Am,
            Mode::RttyLsb,
            Mode::RttyUsb,
            Mode::DataLsb,
            Mode::DataUsb,
            Mode::DataFm,
        ] {
            let name = hamlib_mode_name(mode);
            assert_eq!(
                hamlib_mode_from_name(name),
                Some(mode),
                "mode {mode:?} -> {name} did not round-trip"
            );
        }
    }
}
