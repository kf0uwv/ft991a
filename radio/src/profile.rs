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

//! Named settings profiles: bundles of radio settings applied together
//! (e.g. mode + filter bandwidth for a particular band/activity), loaded
//! from human-editable TOML files.
//!
//! Per `planning/architect/task_plan.md` §12.3: a [`Profile`] is a plain
//! "apply only what's `Some`" bag of already-typed values — it holds no
//! `CatSession`/transport concept and never touches the wire directly, only
//! the same [`crate::Radio`]/[`crate::Ft991aExtras`] trait methods the `ui`
//! crate's keybindings already call. `Mode`/[`crate::PreampMode`] are not
//! given `serde` derives themselves (that would be a wire-format change to
//! shared protocol types for a file-format concern) — instead, a private
//! [`RawProfile`] deserializes plain strings/numbers, and [`Profile::parse`]
//! resolves those against the same name tables `Mode::name()`/
//! [`crate::PreampMode`] already expose.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::ft991a_radio::EX_MENU_TABLE;
use crate::radio_trait::{Ft991aExtras, Mode, PreampMode, Radio, RadioError};

/// Errors loading or applying a [`Profile`].
#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("failed to read profile file {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse profile TOML ({path}): {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error(
        "unknown mode {0:?} in profile (expected one of: LSB, USB, CW-U, CW-L, FM, FM-N, AM, \
         AM-N, RTTY-LSB, RTTY-USB, DATA-LSB, DATA-USB, DATA-FM, C4FM)"
    )]
    UnknownMode(String),
    #[error("unknown pre-amp mode {0:?} in profile (expected one of: IPO, AMP1, AMP2)")]
    UnknownPreampMode(String),
    #[error(
        "unknown EX menu item {0:?} in profile (not found in EX_MENU_TABLE by number or name)"
    )]
    UnknownExMenuItem(String),
    #[error("radio error while applying profile: {0}")]
    Radio(#[from] RadioError),
}

/// The plain-data shape deserialized straight from TOML, before resolving
/// strings against [`Mode`]/[`PreampMode`]/[`EX_MENU_TABLE`]. Not public —
/// [`Profile`] is the type the rest of the crate/`ui`/`app` consume.
#[derive(Debug, Clone, Default, Deserialize)]
struct RawProfile {
    mode: Option<String>,
    filter_width_index: Option<u8>,
    narrow: Option<bool>,
    af_gain: Option<u8>,
    rf_gain: Option<u8>,
    squelch: Option<u8>,
    power_watts: Option<u8>,
    attenuator_on: Option<bool>,
    preamp_mode: Option<String>,
    noise_blanker_on: Option<bool>,
    noise_blanker_level: Option<u8>,
    contour_on: Option<bool>,
    vox_on: Option<bool>,
    #[serde(default)]
    ex_menu: HashMap<String, i32>,
}

/// Successfully-loaded `(name, profile)` pairs — see
/// [`Profile::load_all_from_dir`].
pub type LoadedProfiles = Vec<(String, Profile)>;
/// `(name, error)` pairs for files that failed to load — see
/// [`Profile::load_all_from_dir`].
pub type FailedProfiles = Vec<(String, ProfileError)>;

/// A named bundle of radio settings, applied together. Every field is
/// optional — only the fields present in the source TOML file are applied;
/// everything else is left as the radio already has it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Profile {
    pub mode: Option<Mode>,
    /// Raw `SH` table index, `0..=21` — see [`crate::filter_bandwidth_hz`]
    /// for resolving this to an actual Hz value.
    pub filter_width_index: Option<u8>,
    /// `NA` — narrow filter on/off.
    pub narrow: Option<bool>,
    pub af_gain: Option<u8>,
    pub rf_gain: Option<u8>,
    pub squelch: Option<u8>,
    pub power_watts: Option<u8>,
    pub attenuator_on: Option<bool>,
    pub preamp_mode: Option<PreampMode>,
    pub noise_blanker_on: Option<bool>,
    pub noise_blanker_level: Option<u8>,
    pub contour_on: Option<bool>,
    pub vox_on: Option<bool>,
    /// Arbitrary `EX` menu items, resolved at parse time to `(p1, value)`
    /// pairs so a bad item name/number fails fast on load, not on apply.
    pub ex_menu: Vec<(u16, i32)>,
}

/// Look up an `EX` menu item by its 3-digit `P1` number (as a decimal
/// string, e.g. `"047"` or `"47"`) or by its manual name, case-insensitively
/// (e.g. `"pc keying"` matches `"PC KEYING"`).
fn resolve_ex_menu_key(key: &str) -> Option<u16> {
    if let Ok(p1) = key.trim().parse::<u16>() {
        if EX_MENU_TABLE.iter().any(|item| item.p1 == p1) {
            return Some(p1);
        }
        return None;
    }
    EX_MENU_TABLE
        .iter()
        .find(|item| item.name.eq_ignore_ascii_case(key.trim()))
        .map(|item| item.p1)
}

/// Resolve a human-readable mode name (matching [`Mode::name`], plus a few
/// common spelling variants) to a [`Mode`].
fn resolve_mode(name: &str) -> Option<Mode> {
    const ALL_MODES: [Mode; 14] = [
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
    let trimmed = name.trim();
    ALL_MODES
        .into_iter()
        .find(|m| m.name().eq_ignore_ascii_case(trimmed))
}

fn resolve_preamp_mode(name: &str) -> Option<PreampMode> {
    match name.trim().to_ascii_uppercase().as_str() {
        "IPO" => Some(PreampMode::Ipo),
        "AMP1" => Some(PreampMode::Amp1),
        "AMP2" => Some(PreampMode::Amp2),
        _ => None,
    }
}

impl Profile {
    /// Resolve a [`RawProfile`] (plain deserialized data) into a typed
    /// [`Profile`], failing fast on any unknown mode/pre-amp/`EX` item name.
    fn from_raw(raw: RawProfile) -> Result<Self, ProfileError> {
        let mode = raw
            .mode
            .as_deref()
            .map(|s| resolve_mode(s).ok_or_else(|| ProfileError::UnknownMode(s.to_string())))
            .transpose()?;
        let preamp_mode = raw
            .preamp_mode
            .as_deref()
            .map(|s| {
                resolve_preamp_mode(s).ok_or_else(|| ProfileError::UnknownPreampMode(s.to_string()))
            })
            .transpose()?;
        let mut ex_menu = Vec::with_capacity(raw.ex_menu.len());
        for (key, value) in &raw.ex_menu {
            let p1 = resolve_ex_menu_key(key)
                .ok_or_else(|| ProfileError::UnknownExMenuItem(key.clone()))?;
            ex_menu.push((p1, *value));
        }
        Ok(Profile {
            mode,
            filter_width_index: raw.filter_width_index,
            narrow: raw.narrow,
            af_gain: raw.af_gain,
            rf_gain: raw.rf_gain,
            squelch: raw.squelch,
            power_watts: raw.power_watts,
            attenuator_on: raw.attenuator_on,
            preamp_mode,
            noise_blanker_on: raw.noise_blanker_on,
            noise_blanker_level: raw.noise_blanker_level,
            contour_on: raw.contour_on,
            vox_on: raw.vox_on,
            ex_menu,
        })
    }

    /// Parse a profile from a TOML string (no file I/O — see
    /// [`Self::load_from_file`] for the file-reading counterpart).
    pub fn parse(toml_text: &str) -> Result<Self, ProfileError> {
        let raw: RawProfile = toml::from_str(toml_text).map_err(|source| ProfileError::Parse {
            path: "<string>".to_string(),
            source,
        })?;
        Self::from_raw(raw)
    }

    /// Load and parse one profile from a TOML file.
    pub fn load_from_file(path: &Path) -> Result<Self, ProfileError> {
        let text = std::fs::read_to_string(path).map_err(|source| ProfileError::Io {
            path: path.display().to_string(),
            source,
        })?;
        let raw: RawProfile = toml::from_str(&text).map_err(|source| ProfileError::Parse {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_raw(raw)
    }

    /// Load every `*.toml` file in `dir`, keyed by file stem (the profile's
    /// name). A directory that doesn't exist yields an empty list rather
    /// than an error (no profiles configured yet is a normal state, not a
    /// failure) — only per-file read/parse errors are reported, alongside
    /// the profiles that *did* load, so one bad file doesn't hide the rest.
    pub fn load_all_from_dir(dir: &Path) -> (LoadedProfiles, FailedProfiles) {
        let mut loaded = Vec::new();
        let mut errors = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => return (loaded, errors),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            match Self::load_from_file(&path) {
                Ok(profile) => loaded.push((stem.to_string(), profile)),
                Err(err) => errors.push((stem.to_string(), err)),
            }
        }
        loaded.sort_by(|a, b| a.0.cmp(&b.0));
        (loaded, errors)
    }

    /// Apply every field this profile has set, against a live radio. Order:
    /// mode first (filter bandwidth resolution is mode-family-dependent —
    /// see [`crate::filter_bandwidth_hz`]), then the rest in field order.
    /// Stops at the first error (partial application is reported via the
    /// `RadioResult`'s `Err`, not silently swallowed or rolled back — there
    /// is no transactional multi-command primitive in the CAT protocol).
    pub async fn apply<R: Radio + Ft991aExtras + ?Sized>(
        &self,
        radio: &mut R,
    ) -> Result<(), ProfileError> {
        if let Some(mode) = self.mode {
            radio.set_mode(mode).await?;
        }
        if let Some(index) = self.filter_width_index {
            radio.set_filter_width_index(index).await?;
        }
        if let Some(af_gain) = self.af_gain {
            radio.set_af_gain(af_gain).await?;
        }
        if let Some(rf_gain) = self.rf_gain {
            radio.set_rf_gain(rf_gain).await?;
        }
        if let Some(squelch) = self.squelch {
            radio.set_squelch(squelch).await?;
        }
        if let Some(power_watts) = self.power_watts {
            radio.set_power(power_watts).await?;
        }
        if let Some(on) = self.attenuator_on {
            radio.set_attenuator_on(on).await?;
        }
        if let Some(mode) = self.preamp_mode {
            radio.set_preamp_mode(mode).await?;
        }
        if let Some(on) = self.noise_blanker_on {
            radio.set_noise_blanker_on(on).await?;
        }
        if let Some(level) = self.noise_blanker_level {
            radio.set_noise_blanker_level(level).await?;
        }
        if let Some(on) = self.contour_on {
            radio.set_contour_on(on).await?;
        }
        if let Some(on) = self.vox_on {
            radio.set_vox_on(on).await?;
        }
        for (p1, value) in &self.ex_menu {
            radio.set_ex_menu_item(*p1, *value).await?;
        }
        Ok(())
    }
}

/// The default per-platform profile directory:
/// `$XDG_CONFIG_HOME/ft991a/profiles` (falling back to `~/.config/ft991a/
/// profiles`) on Linux, `%APPDATA%\ft991a\profiles` on Windows. A small
/// local helper rather than a new `dirs`-crate dependency — this repo only
/// needs one directory, not the full per-OS config/cache/data taxonomy.
pub fn default_profile_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|appdata| Path::new(&appdata).join("ft991a/profiles"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return Some(Path::new(&xdg).join("ft991a/profiles"));
        }
        std::env::var_os("HOME").map(|home| Path::new(&home).join(".config/ft991a/profiles"))
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut fields = Vec::new();
        if let Some(mode) = self.mode {
            fields.push(format!("mode={mode}"));
        }
        if let Some(idx) = self.filter_width_index {
            fields.push(format!("filter_width_index={idx}"));
        }
        if !self.ex_menu.is_empty() {
            fields.push(format!("ex_menu={} item(s)", self.ex_menu.len()));
        }
        write!(f, "Profile({})", fields.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Records every setter call, in order, against `Radio`/`Ft991aExtras`'s
    /// default (`NotImplemented`) bodies — a fake per `CLAUDE.md`'s testing
    /// rule ("radio tests use an in-crate fake", mirrored here for `Profile`
    /// tests instead of a fake `CatSession`, since `Profile::apply` is
    /// generic over `Radio + Ft991aExtras`, not over `CatSession`).
    #[derive(Default)]
    struct RecordingRadio {
        calls: Rc<RefCell<Vec<String>>>,
    }

    #[async_trait(?Send)]
    impl Radio for RecordingRadio {
        async fn set_mode(&mut self, mode: Mode) -> crate::RadioResult<()> {
            self.calls.borrow_mut().push(format!("set_mode({mode})"));
            Ok(())
        }
        async fn set_filter_width_index(&mut self, index: u8) -> crate::RadioResult<()> {
            self.calls
                .borrow_mut()
                .push(format!("set_filter_width_index({index})"));
            Ok(())
        }
        async fn set_af_gain(&mut self, level: u8) -> crate::RadioResult<()> {
            self.calls
                .borrow_mut()
                .push(format!("set_af_gain({level})"));
            Ok(())
        }
        async fn set_squelch(&mut self, level: u8) -> crate::RadioResult<()> {
            self.calls
                .borrow_mut()
                .push(format!("set_squelch({level})"));
            Ok(())
        }
        async fn set_attenuator_on(&mut self, on: bool) -> crate::RadioResult<()> {
            self.calls
                .borrow_mut()
                .push(format!("set_attenuator_on({on})"));
            Ok(())
        }
    }

    #[async_trait(?Send)]
    impl Ft991aExtras for RecordingRadio {
        async fn set_ex_menu_item(&mut self, p1: u16, value: i32) -> crate::RadioResult<()> {
            self.calls
                .borrow_mut()
                .push(format!("set_ex_menu_item({p1:03},{value})"));
            Ok(())
        }
    }

    #[test]
    fn parse_empty_profile_has_no_fields() {
        let profile = Profile::parse("").unwrap();
        assert_eq!(profile, Profile::default());
    }

    #[test]
    fn parse_resolves_mode_case_insensitively() {
        let profile = Profile::parse("mode = \"usb\"").unwrap();
        assert_eq!(profile.mode, Some(Mode::Usb));
    }

    #[test]
    fn parse_rejects_unknown_mode() {
        let err = Profile::parse("mode = \"whatever\"").unwrap_err();
        assert!(matches!(err, ProfileError::UnknownMode(m) if m == "whatever"));
    }

    #[test]
    fn parse_resolves_preamp_mode() {
        let profile = Profile::parse("preamp_mode = \"amp1\"").unwrap();
        assert_eq!(profile.preamp_mode, Some(PreampMode::Amp1));
    }

    #[test]
    fn parse_resolves_ex_menu_by_number_and_name() {
        let item = EX_MENU_TABLE[0];
        let toml_text = format!("[ex_menu]\n\"{:03}\" = 1\n\"{}\" = 2\n", item.p1, item.name);
        let profile = Profile::parse(&toml_text).unwrap();
        assert_eq!(profile.ex_menu.len(), 2);
        assert!(profile.ex_menu.iter().all(|(p1, _)| *p1 == item.p1));
    }

    #[test]
    fn parse_rejects_unknown_ex_menu_item() {
        let err = Profile::parse("[ex_menu]\n\"999\" = 1\n").unwrap_err();
        assert!(matches!(err, ProfileError::UnknownExMenuItem(k) if k == "999"));
    }

    #[test]
    fn parse_rejects_malformed_toml() {
        let err = Profile::parse("mode = ").unwrap_err();
        assert!(matches!(err, ProfileError::Parse { .. }));
    }

    #[monoio::test(driver = "legacy")]
    async fn apply_only_calls_setters_for_present_fields() {
        let profile = Profile {
            mode: Some(Mode::Usb),
            squelch: Some(20),
            ..Default::default()
        };
        let mut radio = RecordingRadio::default();
        let calls = Rc::clone(&radio.calls);
        profile.apply(&mut radio).await.unwrap();
        assert_eq!(*calls.borrow(), vec!["set_mode(USB)", "set_squelch(20)"]);
    }

    #[monoio::test(driver = "legacy")]
    async fn apply_sets_mode_before_filter_width() {
        let profile = Profile {
            mode: Some(Mode::CwU),
            filter_width_index: Some(5),
            ..Default::default()
        };
        let mut radio = RecordingRadio::default();
        let calls = Rc::clone(&radio.calls);
        profile.apply(&mut radio).await.unwrap();
        assert_eq!(
            *calls.borrow(),
            vec!["set_mode(CW-U)", "set_filter_width_index(5)"]
        );
    }

    #[monoio::test(driver = "legacy")]
    async fn apply_applies_ex_menu_items() {
        let item = EX_MENU_TABLE[0];
        let profile = Profile {
            ex_menu: vec![(item.p1, 3)],
            ..Default::default()
        };
        let mut radio = RecordingRadio::default();
        let calls = Rc::clone(&radio.calls);
        profile.apply(&mut radio).await.unwrap();
        assert_eq!(
            *calls.borrow(),
            vec![format!("set_ex_menu_item({:03},3)", item.p1)]
        );
    }

    #[monoio::test(driver = "legacy")]
    async fn apply_of_empty_profile_calls_nothing() {
        let profile = Profile::default();
        let mut radio = RecordingRadio::default();
        let calls = Rc::clone(&radio.calls);
        profile.apply(&mut radio).await.unwrap();
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn load_from_file_reads_and_parses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dx.toml");
        std::fs::write(&path, "mode = \"usb\"\nsquelch = 10\n").unwrap();
        let profile = Profile::load_from_file(&path).unwrap();
        assert_eq!(profile.mode, Some(Mode::Usb));
        assert_eq!(profile.squelch, Some(10));
    }

    #[test]
    fn load_from_file_missing_file_is_io_error() {
        let err = Profile::load_from_file(Path::new("/nonexistent/path/x.toml")).unwrap_err();
        assert!(matches!(err, ProfileError::Io { .. }));
    }

    #[test]
    fn load_all_from_dir_collects_names_and_skips_non_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("dx.toml"), "mode = \"usb\"\n").unwrap();
        std::fs::write(dir.path().join("contest.toml"), "mode = \"cw-u\"\n").unwrap();
        std::fs::write(dir.path().join("readme.txt"), "not a profile\n").unwrap();

        let (loaded, errors) = Profile::load_all_from_dir(dir.path());
        assert!(errors.is_empty());
        let names: Vec<&str> = loaded.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["contest", "dx"]);
    }

    #[test]
    fn load_all_from_dir_reports_per_file_errors_without_hiding_good_ones() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("good.toml"), "mode = \"usb\"\n").unwrap();
        std::fs::write(dir.path().join("bad.toml"), "mode = \"nonsense\"\n").unwrap();

        let (loaded, errors) = Profile::load_all_from_dir(dir.path());
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].0, "good");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, "bad");
    }

    #[test]
    fn load_all_from_dir_missing_dir_is_empty_not_error() {
        let (loaded, errors) = Profile::load_all_from_dir(Path::new("/nonexistent/dir"));
        assert!(loaded.is_empty());
        assert!(errors.is_empty());
    }
}
