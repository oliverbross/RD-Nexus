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
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State};
use tempo_app::engine::engine_lock;

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
    #[serde(rename = "type")]
    kind: String,
    freq_mhz: f64,
    call: String,
    comment: String,
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
    if response.content_length().is_some_and(|length| length > MAX_RESPONSE_BYTES as u64) {
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
                                state.0.lock().unwrap_or_else(|e| e.into_inner()).status = NeuralDxStatus {
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

#[tauri::command]
pub fn neural_dx_sync(state: State<'_, NeuralDxState>, payload: Value) -> Result<(), String> {
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
        return Err(format!("Neural DX bridge returned HTTP {}", response.status().as_u16()));
    }
    let _ = limited_body(response)?;
    Ok(())
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
        return Err(format!("Neural DX bridge returned HTTP {}", response.status().as_u16()));
    }
    let body = limited_body(response)?;
    let payload: Value =
        serde_json::from_slice(&body).map_err(|error| format!("decode Neural DX actions: {error}"))?;
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
            if action.kind != "postSpot"
                || !action.freq_mhz.is_finite()
                || !(0.1..=1300.0).contains(&action.freq_mhz)
                || action.call.len() < 3
                || action.call.len() > 16
                || !action.call.chars().all(|char| char.is_ascii_uppercase() || char.is_ascii_digit() || char == '/')
                || action.comment.len() > 120
                || action.comment.chars().any(|char| char.is_control() && char != '\t')
            {
                return Err("Neural DX returned an invalid action".into());
            }
            Ok(action)
        })
        .collect()
}
