//! The ARA Challenge Overlay as a companion app: downloaded from its GitHub releases into this
//! app's data folder, launched from there, and its settings.json under %APPDATA%\AraOverlay
//! edited in place. The overlay itself is Windows-only.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const EXE: &str = "AraChallengeOverlay.exe";
// ponytail: trusts HTTPS from the project's own release, no checksum; add one if releases move hosts.
const DOWNLOAD: &str =
    "https://github.com/Giorgio-Matessi/ara-challenge-overlay/releases/latest/download/AraChallengeOverlay.exe";

#[derive(Serialize)]
pub struct Status {
    supported: bool,
    installed: bool,
}

/// The overlay's own settings.json fields, which it writes in PascalCase and reads case-sensitively,
/// plus Start with Windows, which lives in the registry.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    left: f64,
    top: f64,
    locked: bool,
    start_minimized: bool,
    iracing_only: bool,
    start_with_windows: bool,
}

/// The overlay's own Run value; it ticks Start with Windows in its tray when this exists.
#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const RUN_VALUE: &str = "ARA Challenge Overlay";

#[cfg(windows)]
fn autostart() -> bool {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|k| k.get_raw_value(RUN_VALUE))
        .is_ok()
}

#[cfg(windows)]
fn set_autostart(app: &AppHandle, on: bool) -> Result<(), String> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let failed = |_| "Start with Windows could not be changed.".to_string();
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY).map_err(failed)?;
    if on {
        // Same quoted path the overlay writes, so its tray tick stays in step.
        key.set_value(RUN_VALUE, &format!("\"{}\"", exe(app)?.display())).map_err(failed)
    } else {
        match key.delete_value(RUN_VALUE) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(failed(e)),
            _ => Ok(()),
        }
    }
}

#[cfg(not(windows))]
fn autostart() -> bool {
    false
}

#[cfg(not(windows))]
fn set_autostart(_: &AppHandle, _: bool) -> Result<(), String> {
    Ok(())
}

fn exe(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_local_data_dir().map(|d| d.join(EXE)).map_err(|e| e.to_string())
}

/// %APPDATA%\AraOverlay\settings.json on Windows (config_dir is the roaming AppData folder).
fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().config_dir().map(|d| d.join("AraOverlay").join("settings.json")).map_err(|e| e.to_string())
}

fn read_settings(app: &AppHandle) -> Result<Value, String> {
    Ok(std::fs::read_to_string(settings_path(app)?)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .filter(Value::is_object)
        .unwrap_or(json!({})))
}

#[tauri::command]
pub fn overlay_status(app: AppHandle) -> Result<Status, String> {
    Ok(Status { supported: cfg!(windows), installed: exe(&app)?.is_file() })
}

/// Downloads the latest release, writing to a side file first so a cut-off download never counts as installed.
#[tauri::command]
pub async fn download_overlay(app: AppHandle) -> Result<(), String> {
    let failed = |_: reqwest::Error| "The overlay could not be downloaded.".to_string();
    let bytes = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(failed)?
        .get(DOWNLOAD)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(failed)?
        .bytes()
        .await
        .map_err(failed)?;

    let path = exe(&app)?;
    let partial = path.with_extension("download");
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&partial, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_overlay(app: AppHandle) -> Result<(), String> {
    std::process::Command::new(exe(&app)?)
        .spawn()
        .map(drop)
        .map_err(|_| "The overlay could not be started.".into())
}

/// The saved settings, with the overlay's own defaults where the file has nothing.
#[tauri::command]
pub fn overlay_settings(app: AppHandle) -> Result<Settings, String> {
    let doc = read_settings(&app)?;
    Ok(Settings {
        left: doc["Left"].as_f64().unwrap_or(40.0),
        top: doc["Top"].as_f64().unwrap_or(40.0),
        locked: doc["Locked"].as_bool().unwrap_or(true),
        start_minimized: doc["StartMinimized"].as_bool().unwrap_or(false),
        iracing_only: doc["IRacingOnly"].as_bool().unwrap_or(false),
        start_with_windows: autostart(),
    })
}

/// Writes our fields into settings.json, keeping any others the overlay stores there.
#[tauri::command]
pub fn save_overlay_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let path = settings_path(&app)?;
    let mut doc = read_settings(&app)?;
    doc["Left"] = settings.left.into();
    doc["Top"] = settings.top.into();
    doc["Locked"] = settings.locked.into();
    doc["StartMinimized"] = settings.start_minimized.into();
    doc["IRacingOnly"] = settings.iracing_only.into();
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap()).map_err(|e| e.to_string())?;
    set_autostart(&app, settings.start_with_windows)
}
