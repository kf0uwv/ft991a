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

//! `ft991a-gui` — the GPU console.
//!
//! Network-only by ADR 0008: there is no `--port`, and this binary depends
//! on no transport crate. Running against a local radio means running
//! `ft991a server` locally, which is the same shape people already run for
//! WSJT-X.

use gui::{app::Console, Window};

const DEFAULT_ADDRESS: &str = "127.0.0.1:4532";

fn main() -> eframe::Result<()> {
    let address = std::env::args()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .unwrap_or_else(|| DEFAULT_ADDRESS.to_string());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 560.0])
            .with_title("FT-991A"),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "ft991a-gui",
        options,
        Box::new(move |cc| {
            gui::theme::install(&cc.egui_ctx);
            let mut console = Console::new(address);
            // Try immediately: the common case is a server already running,
            // and making the operator click "connect" every time would be
            // ceremony.
            console.connect();
            Ok(Box::new(Window(console)))
        }),
    )
}
