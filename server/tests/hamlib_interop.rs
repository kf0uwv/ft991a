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

//! What a real Hamlib client receives from **this** radio's bridge.
//!
//! `cat-rigctl` already proves, against a live client, that a capability
//! set can become a `\dump_state` reply Hamlib accepts. It cannot prove
//! that *this* one can, and this is the case where that gap is widest: the
//! FT-991A publishes eight tuning steps and **thirty-four** filter widths,
//! against the single step and single width upstream's fixture carries.
//!
//! Length is exactly what radio-cat-rs ADR 0005's bug was about. A
//! `\dump_state` reply Hamlib disagrees with about length makes
//! `netrigctl_open()` **block forever** rather than fail, and nothing in
//! the symptom points at the cause. Every unit test passed while that was
//! happening. So the check that matters is not what the string looks like;
//! it is whether a real client gets through the handshake.
//!
//! Linux-only: `server::run` is an `async fn` on monoio here (ADR 0006).

#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};

use cat_transport_core::test_support::{Exchange, ScriptedCatSession};
use server::ServerConfig;

/// Whether a real Hamlib client is available to test against.
///
/// Mirrors `cat-rigctl`'s rule, and for the reason recorded there: where
/// Hamlib was installed on purpose, a missing binary is a failure, not a
/// skip. The signal is `EXPECT_HAMLIB` and deliberately not `CI` — `CI` is
/// set on the Windows runner too, where this file does not even compile.
fn have_rigctl() -> bool {
    if std::process::Command::new("rigctl")
        .arg("--version")
        .output()
        .is_ok()
    {
        return true;
    }
    assert!(
        std::env::var_os("EXPECT_HAMLIB").is_none(),
        "EXPECT_HAMLIB is set but rigctl is not installed. Install libhamlib-utils."
    );
    eprintln!("SKIPPED: rigctl not installed (install libhamlib-utils).");
    false
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .unwrap()
        .port()
}

/// Bring up this crate's real server on a rigctl port, backed by a scripted
/// radio. Returns once the listener is accepting.
fn serve(script: Vec<Exchange>) -> u16 {
    let port = free_port();
    std::thread::spawn(move || {
        // `enable_timer` is not optional: `cat-rigctl`'s accept loop uses
        // monoio timers, and a runtime built without one panics inside the
        // driver rather than returning an error.
        let mut rt = monoio::RuntimeBuilder::<monoio::LegacyDriver>::new()
            .enable_timer()
            .build()
            .expect("monoio runtime");
        rt.block_on(async move {
            let _ = server::run(
                ScriptedCatSession::with_unordered_script(script),
                ServerConfig {
                    rigctl_port: Some(port),
                    ..Default::default()
                },
            )
            .await;
        });
    });
    // Poll rather than sleep a fixed amount: a fixed sleep is either flaky
    // or slow, and usually both on a loaded machine.
    for _ in 0..200 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return port;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("server did not start listening on {port}");
}

#[test]
fn a_real_hamlib_client_completes_the_handshake_with_this_radios_capabilities() {
    if !have_rigctl() {
        return;
    }
    // `netrigctl_open()` reads `\dump_state` before answering anything at
    // all, so getting a frequency back is proof the handshake completed.
    // The script is generous rather than exact: how many times Hamlib
    // probes during open is its business, not this test's.
    let port = serve(
        std::iter::repeat_with(|| Exchange::new("FA;", "FA014074000;"))
            .take(16)
            .collect(),
    );
    // Wrapped in `timeout` deliberately: the failure mode under test is a
    // hang, and a test that hangs is not a red test.
    let out = std::process::Command::new("timeout")
        .args([
            "20",
            "rigctl",
            "-m",
            "2",
            "-r",
            &format!("127.0.0.1:{port}"),
            "f",
        ])
        .output()
        .expect("run rigctl");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("14074000"),
        "Hamlib did not complete the handshake against the FT-991A's \
         capability set; got {stdout:?} / {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn the_client_is_told_this_radios_real_coverage_steps_and_widths() {
    if !have_rigctl() {
        return;
    }
    let port = serve(Vec::new());
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream.write_all(b"\\dump_state\n").expect("write");
    stream.flush().unwrap();

    // Read until the server stops talking. A fixed line count either stops
    // short of a tail this long or blocks waiting for a line that is not
    // coming -- the connection stays open after the reply, so EOF never
    // arrives. The read timeout is what terminates this.
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .expect("set read timeout");
    let mut reply = String::new();
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => reply.push_str(&line),
        }
    }
    assert!(!reply.is_empty(), "no `\\dump_state` reply at all");

    // UHF coverage, which a client has no way to guess from a
    // Kenwood-shaped protocol.
    assert!(
        reply.contains("30000 470000000"),
        "real coverage missing from {reply:?}"
    );
    // Steps and widths the placeholder never mentioned. 50 Hz is the CW
    // narrow filter -- the narrowest thing this radio has, and absent from
    // the hand-made list the capability fixture used to carry.
    for row in ["-1 6250\n", "-1 25000\n", "-1 50\n", "-1 3200\n"] {
        assert!(reply.contains(row), "{row:?} missing from {reply:?}");
    }
    // Compared line-exact, not as a substring. The RIT/XIT limits are
    // bare numeric lines, and this radio has a `-1 1200` *filter width* --
    // a substring test for "1200" matches that and reports a placeholder
    // that is not there.
    let lines: Vec<&str> = reply.lines().collect();
    assert!(
        lines.iter().filter(|l| **l == "9999").count() >= 2,
        "real clarifier limits missing (RIT and XIT): {reply:?}"
    );
    assert!(
        !lines.contains(&"1200"),
        "still sending the placeholder RIT limit: {reply:?}"
    );
    // The six trailing capability bitmasks Hamlib counts before it will
    // return. Short by one and `netrigctl_open()` blocks forever -- and
    // this reply is long enough that an off-by-one would be easy to miss.
    assert_eq!(
        reply.matches("0x0\n").count(),
        6,
        "capability tail is not six lines: {reply:?}"
    );
}
