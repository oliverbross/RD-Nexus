//! Managed Neural DX Watcher runtime.
//!
//! The pinned upstream Flask application is packaged as a self-contained local
//! executable for each desktop target. It binds only to loopback, uses a private
//! app-data directory, and receives station/logbook/spot snapshots through an
//! authenticated in-process bridge. Nexus remains the sole radio and logbook
//! authority; the companion process is display and analysis only.

use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State};
use tempo_app::engine::engine_lock;

use crate::log_queries::LogQueries;
use crate::SharedEngine;

const VERSION: &str = "12.7";
const MAX_SYNC_BYTES: usize = 16 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NeuralDxStatus {
    phase: &'static str,
    url: Option<String>,
    version: &'static str,
    detail: Option<String>,
}

struct Runtime {
    child: Option<Child>,
    status: NeuralDxStatus,
    token: String,
}

#[derive(Debug, Deserialize)]
struct ReadyFile {
    port: u16,
    version: String,
    identity: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NeuralDxAction {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    freq_mhz: f64,
    call: String,
    comment: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionReceipt {
    action: NeuralDxAction,
    phase: String,
    detail: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NeuralDxExecution {
    id: String,
    state: String,
    detail: Option<String>,
}

pub struct NeuralDxState(Mutex<Runtime>);

impl Default for NeuralDxState {
    fn default() -> Self {
        Self(Mutex::new(Runtime {
            child: None,
            status: NeuralDxStatus {
                phase: "starting",
                url: None,
                version: VERSION,
                detail: None,
            },
            token: String::new(),
        }))
    }
}

impl Drop for NeuralDxState {
    fn drop(&mut self) {
        let runtime = self.0.get_mut().unwrap_or_else(|e| e.into_inner());
        if let Some(child) = runtime.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn token() -> String {
    let mut bytes = [0_u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .expect("operating-system random source unavailable");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn bundled_executable(resource_dir: &Path) -> PathBuf {
    resource_dir.join("neural-dx").join(if cfg!(windows) {
        "neural-dx-watcher.exe"
    } else {
        "neural-dx-watcher"
    })
}

fn launch(app: &AppHandle) -> Result<(Child, PathBuf, String), String> {
    let secret = token();
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|error| format!("resolve resource directory: {error}"))?;
    let executable = bundled_executable(&resource_dir);
    if !executable.is_file() {
        return Err(
            "Neural DX is not staged; run `node scripts/stage-neural-dx.mjs` before starting Nexus"
                .into(),
        );
    }
    let home = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("resolve app data directory: {error}"))?
        .join("neural-dx");
    std::fs::create_dir_all(home.join("data"))
        .map_err(|error| format!("create Neural DX data directory: {error}"))?;
    std::fs::create_dir_all(home.join("logs"))
        .map_err(|error| format!("create Neural DX log directory: {error}"))?;
    let ready_nonce = token();
    let ready_file = home.join(format!(
        "ready-{}-{}.json",
        std::process::id(),
        &ready_nonce[..12]
    ));
    let _ = std::fs::remove_file(&ready_file);
    let log = std::fs::File::create(home.join("neural-dx.log"))
        .map_err(|error| format!("create Neural DX log: {error}"))?;
    let log_err = log
        .try_clone()
        .map_err(|error| format!("open Neural DX error log: {error}"))?;
    let (callsign, grid) = {
        let state = app.state::<SharedEngine>();
        let engine = engine_lock(&state);
        (
            engine.settings().mycall.clone(),
            engine.settings().mygrid.clone(),
        )
    };
    let mut command = Command::new(executable);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
        .current_dir(&home)
        .env("NEURAL_DX_PORT", "0")
        .env("NEURAL_DX_CALLSIGN", &callsign)
        .env("NEURAL_DX_GRID", &grid)
        .env("NEURAL_DX_NEXUS_EMBED", "1")
        .env("NEURAL_DX_NEXUS_TOKEN", &secret)
        .env("NEURAL_DX_READY_FILE", &ready_file)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    let child = command
        .spawn()
        .map_err(|error| format!("start Neural DX: {error}"))?;
    Ok((child, ready_file, secret))
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(4))
        .build()
        .map_err(|error| format!("create Neural DX HTTP client: {error}"))
}

fn limited_body(mut response: reqwest::blocking::Response) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err("Neural DX response exceeded the safety limit".into());
    }
    let mut body = Vec::new();
    response
        .by_ref()
        .take((MAX_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut body)
        .map_err(|error| format!("read Neural DX response: {error}"))?;
    if body.len() > MAX_RESPONSE_BYTES {
        return Err("Neural DX response exceeded the safety limit".into());
    }
    Ok(body)
}

fn valid_action(action: &NeuralDxAction) -> bool {
    action.id.len() == 32
        && action.id.chars().all(|char| char.is_ascii_hexdigit())
        && action.kind == "postSpot"
        && action.freq_mhz.is_finite()
        && (0.1..=1300.0).contains(&action.freq_mhz)
        && (3..=16).contains(&action.call.len())
        && action
            .call
            .chars()
            .all(|char| char.is_ascii_uppercase() || char.is_ascii_digit() || char == '/')
        && action.comment.len() <= 120
        && !action
            .comment
            .chars()
            .any(|char| char.is_control() && char != '\t')
}

fn journal_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("resolve Neural DX action journal: {error}"))?
        .join("neural-dx")
        .join("action-journal.json"))
}

fn read_journal(path: &Path) -> Result<BTreeMap<String, ActionReceipt>, String> {
    let backup = path.with_extension("json.bak");
    if !path.exists() && backup.exists() {
        std::fs::rename(&backup, path)
            .map_err(|error| format!("recover Neural DX action journal: {error}"))?;
    }
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("read Neural DX action journal: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(format!("read Neural DX action journal: {error}")),
    }
}

fn write_journal(path: &Path, journal: &BTreeMap<String, ActionReceipt>) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Neural DX action journal has no parent directory")?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create Neural DX action journal directory: {error}"))?;
    let pending = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec(journal)
        .map_err(|error| format!("encode Neural DX action journal: {error}"))?;
    let mut file = std::fs::File::create(&pending)
        .map_err(|error| format!("create Neural DX action journal: {error}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("persist Neural DX action journal: {error}"))?;
    // Windows rename does not replace an existing destination. The backup dance keeps every
    // crash point recoverable: read_journal restores it when the installed path is absent.
    let backup = path.with_extension("json.bak");
    let _ = std::fs::remove_file(&backup);
    let had_previous = path.exists();
    if had_previous {
        std::fs::rename(path, &backup)
            .map_err(|error| format!("prepare Neural DX action journal replacement: {error}"))?;
    }
    if let Err(error) = std::fs::rename(&pending, path) {
        if had_previous {
            let _ = std::fs::rename(&backup, path);
        }
        return Err(format!("install Neural DX action journal: {error}"));
    }
    let _ = std::fs::remove_file(backup);
    Ok(())
}

fn execute_with(
    path: &Path,
    action: NeuralDxAction,
    transmit: impl FnOnce(&NeuralDxAction) -> Result<(), String>,
) -> Result<NeuralDxExecution, String> {
    if !valid_action(&action) {
        return Err("Neural DX returned an invalid action".into());
    }
    let mut journal = read_journal(path)?;
    if let Some(receipt) = journal.get_mut(&action.id) {
        if receipt.action.kind != action.kind
            || receipt.action.freq_mhz != action.freq_mhz
            || receipt.action.call != action.call
            || receipt.action.comment != action.comment
        {
            return Err("Neural DX reused an action id with different content".into());
        }
        match receipt.phase.as_str() {
            "success" => {
                return Ok(NeuralDxExecution {
                    id: action.id,
                    state: "delivered".into(),
                    detail: receipt.detail.clone(),
                });
            }
            "transmitting" | "deliveryUnknown" => {
                receipt.phase = "deliveryUnknown".into();
                receipt.detail = Some(
                    "Delivery may have occurred before Nexus restarted; automatic retry is disabled"
                        .into(),
                );
                let detail = receipt.detail.clone();
                write_journal(path, &journal)?;
                return Ok(NeuralDxExecution {
                    id: action.id,
                    state: "deliveryUnknown".into(),
                    detail,
                });
            }
            "prepared" => {}
            _ => return Err("Neural DX action journal contains an invalid phase".into()),
        }
    } else {
        journal.insert(
            action.id.clone(),
            ActionReceipt {
                action: action.clone(),
                phase: "prepared".into(),
                detail: None,
            },
        );
        write_journal(path, &journal)?;
    }
    {
        let receipt = journal
            .get_mut(&action.id)
            .expect("inserted action receipt");
        receipt.phase = "transmitting".into();
        receipt.detail = None;
    }
    write_journal(path, &journal)?;
    if let Err(error) = transmit(&action) {
        let receipt = journal
            .get_mut(&action.id)
            .expect("existing action receipt");
        receipt.phase = "prepared".into();
        receipt.detail = Some(error.clone());
        write_journal(path, &journal)?;
        return Err(error);
    }
    let receipt = journal
        .get_mut(&action.id)
        .expect("existing action receipt");
    receipt.phase = "success".into();
    receipt.detail = None;
    write_journal(path, &journal)?;
    Ok(NeuralDxExecution {
        id: action.id,
        state: "delivered".into(),
        detail: None,
    })
}

fn set_unavailable(state: &NeuralDxState, detail: String) {
    state.0.lock().unwrap_or_else(|e| e.into_inner()).status = NeuralDxStatus {
        phase: "unavailable",
        url: None,
        version: VERSION,
        detail: Some(detail),
    };
}

pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("neural-dx".into())
        .spawn(move || {
            let state = app.state::<NeuralDxState>();
            let (child, ready_file, secret) = match launch(&app) {
                Ok(value) => value,
                Err(detail) => {
                    set_unavailable(&state, detail);
                    return;
                }
            };
            {
                let mut runtime = state.0.lock().unwrap_or_else(|e| e.into_inner());
                runtime.child = Some(child);
                runtime.token = secret.clone();
            }
            let deadline = Instant::now() + Duration::from_secs(60);
            let http = match client() {
                Ok(client) => client,
                Err(detail) => {
                    set_unavailable(&state, detail);
                    return;
                }
            };
            while Instant::now() < deadline {
                let exited = {
                    let mut runtime = state.0.lock().unwrap_or_else(|e| e.into_inner());
                    runtime
                        .child
                        .as_mut()
                        .and_then(|child| child.try_wait().ok().flatten())
                };
                if let Some(exit) = exited {
                    set_unavailable(
                        &state,
                        format!("Neural DX exited before becoming ready: {exit}"),
                    );
                    return;
                }
                if let Ok(raw) = std::fs::read(&ready_file) {
                    if let Ok(ready) = serde_json::from_slice::<ReadyFile>(&raw) {
                        if ready.identity == "neural-dx-watcher" && ready.version == VERSION {
                            let url = format!("http://127.0.0.1:{}", ready.port);
                            let authenticated = http
                                .get(format!("{url}/api/nexus/status"))
                                .header("X-Nexus-Token", &secret)
                                .send()
                                .ok()
                                .filter(|response| response.status().is_success())
                                .and_then(|response| limited_body(response).ok())
                                .and_then(|body| serde_json::from_slice::<Value>(&body).ok())
                                .and_then(|payload| payload.get("ok").and_then(Value::as_bool))
                                == Some(true);
                            if authenticated {
                                state.0.lock().unwrap_or_else(|e| e.into_inner()).status =
                                    NeuralDxStatus {
                                        phase: "ready",
                                        url: Some(url),
                                        version: VERSION,
                                        detail: None,
                                    };
                                let _ = std::fs::remove_file(&ready_file);
                                return;
                            }
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(180));
            }
            set_unavailable(
                &state,
                "Neural DX did not become ready within 60 seconds".into(),
            );
        })
        .expect("spawn Neural DX supervisor");
}

#[tauri::command]
pub fn neural_dx_status(state: State<'_, NeuralDxState>) -> NeuralDxStatus {
    let mut runtime = state.0.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(exit) = runtime
        .child
        .as_mut()
        .and_then(|child| child.try_wait().ok().flatten())
    {
        runtime.child = None;
        runtime.status = NeuralDxStatus {
            phase: "unavailable",
            url: None,
            version: VERSION,
            detail: Some(format!("Neural DX exited: {exit}")),
        };
    }
    runtime.status.clone()
}

#[tauri::command]
pub fn restart_neural_dx(app: AppHandle, state: State<'_, NeuralDxState>) {
    {
        let mut runtime = state.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(child) = runtime.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        runtime.child = None;
        runtime.status = NeuralDxStatus {
            phase: "starting",
            url: None,
            version: VERSION,
            detail: None,
        };
    }
    start(app);
}

fn native_sync_payload(
    payload: Value,
    callsign: String,
    grid: String,
    qso_summary: Value,
) -> Result<Value, String> {
    let mut payload = payload
        .as_object()
        .cloned()
        .ok_or("Neural DX sync payload must be an object")?;
    if payload.keys().any(|key| key != "spots") {
        return Err("Neural DX browser sync may contain spots only".into());
    }
    payload.insert(
        "station".into(),
        serde_json::json!({ "callsign": callsign, "grid": grid }),
    );
    payload.insert("qsoSummary".into(), qso_summary);
    Ok(Value::Object(payload))
}

fn send_sync(url: String, secret: String, payload: Value) -> Result<(), String> {
    let body =
        serde_json::to_vec(&payload).map_err(|error| format!("encode Neural DX sync: {error}"))?;
    if body.len() > MAX_SYNC_BYTES {
        return Err("Neural DX sync payload exceeded the 16 MiB safety limit".into());
    }
    let response = client()?
        .post(format!("{url}/api/nexus/sync"))
        .header("X-Nexus-Token", secret)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .map_err(|error| format!("send Neural DX sync: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Neural DX bridge returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let _ = limited_body(response)?;
    Ok(())
}

#[tauri::command]
pub async fn neural_dx_sync(
    state: State<'_, NeuralDxState>,
    engine: State<'_, SharedEngine>,
    queries: State<'_, LogQueries>,
    payload: Value,
) -> Result<(), String> {
    let (url, secret) = {
        let runtime = state.0.lock().unwrap_or_else(|e| e.into_inner());
        if runtime.status.phase != "ready" {
            return Err("Neural DX is not ready".into());
        }
        (
            runtime
                .status
                .url
                .clone()
                .ok_or("Neural DX URL is unavailable")?,
            runtime.token.clone(),
        )
    };
    let engine = engine.inner().clone();
    let queries = queries.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (callsign, grid) = {
            let engine = engine_lock(&engine);
            (
                engine.settings().mycall.clone(),
                engine.settings().mygrid.clone(),
            )
        };
        let summary = queries.neural_dx_summary(&engine)?;
        let payload = native_sync_payload(payload, callsign, grid, summary)?;
        send_sync(url, secret, payload)
    })
    .await
    .map_err(|error| format!("Neural DX native sync did not finish: {error}"))?
}

#[tauri::command]
pub fn neural_dx_actions(state: State<'_, NeuralDxState>) -> Result<Vec<NeuralDxAction>, String> {
    let (url, secret) = {
        let runtime = state.0.lock().unwrap_or_else(|e| e.into_inner());
        if runtime.status.phase != "ready" {
            return Ok(Vec::new());
        }
        (
            runtime
                .status
                .url
                .clone()
                .ok_or("Neural DX URL is unavailable")?,
            runtime.token.clone(),
        )
    };
    let response = client()?
        .get(format!("{url}/api/nexus/actions"))
        .header("X-Nexus-Token", secret)
        .send()
        .map_err(|error| format!("request Neural DX actions: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Neural DX bridge returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let body = limited_body(response)?;
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|error| format!("decode Neural DX actions: {error}"))?;
    let actions = payload
        .get("actions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if actions.len() > 100 {
        return Err("Neural DX returned too many actions".into());
    }
    actions
        .into_iter()
        .map(|value| {
            let action: NeuralDxAction = serde_json::from_value(value)
                .map_err(|_| "Neural DX returned an invalid action".to_string())?;
            if !valid_action(&action) {
                return Err("Neural DX returned an invalid action".into());
            }
            Ok(action)
        })
        .collect()
}

/// Execute a companion request through Nexus's sole cluster-post authority. The action id and
/// outcome are durable, so an acknowledgement failure cannot retransmit it automatically.
#[tauri::command]
pub fn neural_dx_execute_action(
    app: AppHandle,
    state: State<'_, NeuralDxState>,
    action: NeuralDxAction,
) -> Result<NeuralDxExecution, String> {
    let _serial = state.0.lock().unwrap_or_else(|error| error.into_inner());
    execute_with(&journal_path(&app)?, action, |action| {
        crate::post_spot(action.freq_mhz, action.call.clone(), action.comment.clone())
    })
}

/// Resolve the deliberately fail-closed state left by a restart during transmission. Only an
/// operator can declare the spot delivered or authorize one retry.
#[tauri::command]
pub fn neural_dx_resolve_action(
    app: AppHandle,
    state: State<'_, NeuralDxState>,
    id: String,
    resolution: String,
) -> Result<NeuralDxExecution, String> {
    if id.len() != 32 || !id.chars().all(|char| char.is_ascii_hexdigit()) {
        return Err("invalid Neural DX action id".into());
    }
    let _serial = state.0.lock().unwrap_or_else(|error| error.into_inner());
    let path = journal_path(&app)?;
    let mut journal = read_journal(&path)?;
    let receipt = journal
        .get_mut(&id)
        .ok_or("Neural DX action is not present in the journal")?;
    if receipt.phase != "deliveryUnknown" && receipt.phase != "transmitting" {
        return Err("Neural DX action does not need operator resolution".into());
    }
    match resolution.as_str() {
        "delivered" => {
            receipt.phase = "success".into();
            receipt.detail = Some("Operator marked the delivery as complete".into());
        }
        "retry" => {
            receipt.phase = "prepared".into();
            receipt.detail = Some("Operator authorised one retry".into());
        }
        _ => return Err("invalid Neural DX action resolution".into()),
    }
    let state = if resolution == "delivered" {
        "delivered"
    } else {
        "retry"
    };
    let detail = receipt.detail.clone();
    write_journal(&path, &journal)?;
    Ok(NeuralDxExecution {
        id,
        state: state.into(),
        detail,
    })
}

#[tauri::command]
pub fn neural_dx_ack_actions(
    state: State<'_, NeuralDxState>,
    ids: Vec<String>,
) -> Result<(), String> {
    if ids.len() > 100
        || ids
            .iter()
            .any(|id| id.len() != 32 || !id.chars().all(|char| char.is_ascii_hexdigit()))
    {
        return Err("invalid Neural DX action ids".into());
    }
    let (url, secret) = {
        let runtime = state.0.lock().unwrap_or_else(|e| e.into_inner());
        if runtime.status.phase != "ready" {
            return Err("Neural DX is not ready".into());
        }
        (
            runtime
                .status
                .url
                .clone()
                .ok_or("Neural DX URL is unavailable")?,
            runtime.token.clone(),
        )
    };
    let body = serde_json::to_vec(&serde_json::json!({ "ids": ids }))
        .map_err(|error| format!("encode Neural DX action acknowledgement: {error}"))?;
    let response = client()?
        .post(format!("{url}/api/nexus/actions/ack"))
        .header("X-Nexus-Token", secret)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .map_err(|error| format!("acknowledge Neural DX actions: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Neural DX bridge returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let _ = limited_body(response)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn action() -> NeuralDxAction {
        NeuralDxAction {
            id: "0123456789abcdef0123456789abcdef".into(),
            kind: "postSpot".into(),
            freq_mhz: 14.195,
            call: "VK9DX".into(),
            comment: "UP 5".into(),
        }
    }

    #[test]
    fn native_sync_replaces_browser_station_and_log_truth() {
        let summary = serde_json::json!({
            "workedCalls": ["VK9DX"], "confirmedCalls": [], "totalQso": 67_000
        });
        let payload = native_sync_payload(
            serde_json::json!({"spots": [{"dx_call": "VK9DX"}]}),
            "N0CALL".into(),
            "FN31".into(),
            summary.clone(),
        )
        .unwrap();
        assert_eq!(payload["station"]["callsign"], "N0CALL");
        assert_eq!(payload["qsoSummary"], summary);
        assert_eq!(payload["spots"][0]["dx_call"], "VK9DX");
        assert!(native_sync_payload(
            serde_json::json!({"station": {"callsign": "ATTACK"}, "qsoSummary": {}}),
            "N0CALL".into(),
            "FN31".into(),
            serde_json::json!({}),
        )
        .is_err());
    }

    fn path(tag: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "nexus-neural-dx-{tag}-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn success_is_idempotent_across_ack_failure_and_restart() {
        let path = path("success");
        let sends = Cell::new(0);
        let first = execute_with(&path, action(), |_| {
            sends.set(sends.get() + 1);
            Ok(())
        })
        .unwrap();
        assert_eq!(first.state, "delivered");
        let again = execute_with(&path, action(), |_| {
            sends.set(sends.get() + 1);
            Ok(())
        })
        .unwrap();
        assert_eq!(again.state, "delivered");
        assert_eq!(sends.get(), 1, "a failed child ack cannot retransmit");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn prepared_before_post_is_recoverable_but_transmitting_is_not_retried() {
        let prepared_path = path("prepared");
        let mut journal = BTreeMap::new();
        journal.insert(
            action().id.clone(),
            ActionReceipt {
                action: action(),
                phase: "prepared".into(),
                detail: None,
            },
        );
        write_journal(&prepared_path, &journal).unwrap();
        let sends = Cell::new(0);
        execute_with(&prepared_path, action(), |_| {
            sends.set(sends.get() + 1);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            sends.get(),
            1,
            "crash before post resumes the prepared intent"
        );

        let unknown_path = path("unknown");
        sends.set(sends.get() + 1); // the external transmission happened
        let mut interrupted = BTreeMap::new();
        interrupted.insert(
            action().id.clone(),
            ActionReceipt {
                action: action(),
                phase: "transmitting".into(), // process ended before the success receipt
                detail: None,
            },
        );
        write_journal(&unknown_path, &interrupted).unwrap();
        let after_restart = execute_with(&unknown_path, action(), |_| {
            sends.set(sends.get() + 1);
            Ok(())
        })
        .unwrap();
        assert_eq!(after_restart.state, "deliveryUnknown");
        assert_eq!(sends.get(), 2, "delivery-unknown was not retransmitted");
        let _ = std::fs::remove_file(prepared_path);
        let _ = std::fs::remove_file(unknown_path);
    }

    #[test]
    fn definite_pretransmit_failure_remains_safe_to_retry() {
        let path = path("retry");
        assert!(execute_with(&path, action(), |_| Err("no cluster".into())).is_err());
        let sends = Cell::new(0);
        assert_eq!(
            execute_with(&path, action(), |_| {
                sends.set(sends.get() + 1);
                Ok(())
            })
            .unwrap()
            .state,
            "delivered"
        );
        assert_eq!(sends.get(), 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn interrupted_cross_platform_journal_replacement_recovers_backup() {
        let path = path("backup");
        let mut journal = BTreeMap::new();
        journal.insert(
            action().id.clone(),
            ActionReceipt {
                action: action(),
                phase: "success".into(),
                detail: None,
            },
        );
        write_journal(&path, &journal).unwrap();
        let backup = path.with_extension("json.bak");
        std::fs::rename(&path, &backup).unwrap();
        assert_eq!(read_journal(&path).unwrap().len(), 1);
        assert!(path.exists());
        assert!(!backup.exists());
        let _ = std::fs::remove_file(path);
    }
}
