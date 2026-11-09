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

/// The overlay's own settings.json fields, which it writes in PascalCase and reads case-sensitively.
#[derive(Serialize, Deserialize)]
pub struct Settings {
    left: f64,
    top: f64,
    locked: bool,
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
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap()).map_err(|e| e.to_string())
}
