// SPDX-License-Identifier: GPL-3.0-only
//! Native startup mode. Audio ownership changes only through ordinary app shutdown/restart;
//! the webview cannot create a second radio loop or override admitted fixture isolation.

use serde::{Deserialize, Serialize};
use std::{fs, path::Path, sync::OnceLock};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProductMode {
    #[default]
    Basic,
    Digi,
}

static STARTUP_MODE: OnceLock<ProductMode> = OnceLock::new();

fn load(path: &Path) -> ProductMode {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save(path: &Path, mode: ProductMode) -> Result<(), String> {
    let parent = path.parent().ok_or("product mode directory unavailable")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    // Settings uses the same sibling-temp + rename convention. This file contains only
    // the mode, never operator identity, device names, credentials, or fixture arguments.
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&tmp, serde_json::to_vec(&mode).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn startup() -> ProductMode {
    *STARTUP_MODE.get_or_init(|| load(&super::config_dir().join("product-mode.json")))
}

#[cfg(feature = "radio")]
pub fn audio_policy(
    mode: ProductMode,
    fixture_audio: Option<bool>,
) -> tempo_audio::service::AudioStartup {
    use tempo_audio::service::AudioStartup;
    match fixture_audio {
        Some(true) => AudioStartup::Fixture,
        Some(false) => AudioStartup::Disabled,
        None if mode == ProductMode::Digi => AudioStartup::Device,
        None => AudioStartup::Disabled,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeStatus {
    mode: ProductMode,
    isolated: bool,
    restart_required: bool,
}

#[tauri::command]
pub fn shackcq_product_mode() -> ModeStatus {
    let isolated = super::acceptance::active();
    let mode = if isolated {
        load(&super::config_dir().join("product-mode.json"))
    } else {
        startup()
    };
    ModeStatus {
        mode,
        isolated,
        restart_required: false,
    }
}

#[tauri::command]
pub fn shackcq_set_product_mode(
    mode: ProductMode,
    app: tauri::AppHandle,
) -> Result<ModeStatus, String> {
    let isolated = super::acceptance::active();
    let current = startup();
    save(&super::config_dir().join("product-mode.json"), mode)?;
    let restart_required = !isolated && mode != current;
    if restart_required {
        // Delivers ExitRequested/Exit: quit_cleanup unkeys, flushes journals, and stops
        // the existing owner before the new process opens the selected audio lifecycle.
        app.request_restart();
    }
    Ok(ModeStatus {
        mode,
        isolated,
        restart_required,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_selection_persists_and_malformed_selection_stays_basic() {
        let root = std::env::temp_dir().join(format!(
            "shackcq-mode-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = root.join("product-mode.json");
        assert_eq!(load(&path), ProductMode::Basic);
        save(&path, ProductMode::Digi).unwrap();
        assert_eq!(load(&path), ProductMode::Digi);
        save(&path, ProductMode::Basic).unwrap();
        assert_eq!(load(&path), ProductMode::Basic);
        fs::write(&path, b"invalid").unwrap();
        assert_eq!(load(&path), ProductMode::Basic);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(feature = "radio")]
    #[test]
    fn native_mode_selects_audio_but_never_overrides_a_fixture() {
        use tempo_audio::service::AudioStartup;
        assert_eq!(
            audio_policy(ProductMode::Basic, None),
            AudioStartup::Disabled
        );
        assert_eq!(audio_policy(ProductMode::Digi, None), AudioStartup::Device);
        for mode in [ProductMode::Basic, ProductMode::Digi] {
            assert_eq!(audio_policy(mode, Some(false)), AudioStartup::Disabled);
            assert_eq!(audio_policy(mode, Some(true)), AudioStartup::Fixture);
        }
    }
}
