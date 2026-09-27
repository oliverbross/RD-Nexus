//! Managed Neural DX Watcher runtime.
//!
//! The pinned upstream Flask application is packaged as a self-contained local
//! executable for each desktop target. It binds only to loopback, uses a private
//! app-data directory, and receives station/logbook/spot snapshots through an
//! authenticated in-process bridge. Nexus remains the sole radio and logbook
//! authority; the companion process is display and analysis only.

use serde::Serialize;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, State};
use tempo_app::engine::engine_lock;

use crate::SharedEngine;

const VERSION: &str = "12.7";
const PREFERRED_PORT: u16 = 39_074;

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
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}-{:x}", std::process::id(), nanos)
}

fn reserve_port() -> Result<u16, String> {
    TcpListener::bind(("127.0.0.1", PREFERRED_PORT))
        .or_else(|_| TcpListener::bind(("127.0.0.1", 0)))
        .and_then(|socket| socket.local_addr())
        .map(|address| address.port())
        .map_err(|error| format!("reserve a localhost port: {error}"))
}

fn bundled_executable(resource_dir: &Path) -> PathBuf {
    resource_dir.join("neural-dx").join(if cfg!(windows) {
        "neural-dx-watcher.exe"
    } else {
        "neural-dx-watcher"
    })
}

fn launch(app: &AppHandle) -> Result<(Child, String, String), String> {
    let port = reserve_port()?;
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
        .env("NEURAL_DX_PORT", port.to_string())
        .env("NEURAL_DX_CALLSIGN", &callsign)
        .env("NEURAL_DX_GRID", &grid)
        .env("NEURAL_DX_NEXUS_EMBED", "1")
        .env("NEURAL_DX_NEXUS_TOKEN", &secret)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    let child = command
        .spawn()
        .map_err(|error| format!("start Neural DX: {error}"))?;
    Ok((child, format!("http://127.0.0.1:{port}"), secret))
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
            let (child, url, secret) = match launch(&app) {
                Ok(value) => value,
                Err(detail) => {
                    set_unavailable(&state, detail);
                    return;
                }
            };
            {
                let mut runtime = state.0.lock().unwrap_or_else(|e| e.into_inner());
                runtime.child = Some(child);
                runtime.token = secret;
                runtime.status.url = Some(url.clone());
            }
            let deadline = Instant::now() + Duration::from_secs(60);
            let address = match url.trim_start_matches("http://").parse() {
                Ok(address) => address,
                Err(error) => {
                    set_unavailable(&state, format!("parse Neural DX address: {error}"));
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
                if TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_ok() {
                    state.0.lock().unwrap_or_else(|e| e.into_inner()).status = NeuralDxStatus {
                        phase: "ready",
                        url: Some(url),
                        version: VERSION,
                        detail: None,
                    };
                    return;
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
    let address = url.trim_start_matches("http://");
    let body =
        serde_json::to_vec(&payload).map_err(|error| format!("encode Neural DX sync: {error}"))?;
    let mut stream = TcpStream::connect_timeout(
        &address
            .parse()
            .map_err(|error| format!("parse Neural DX address: {error}"))?,
        Duration::from_secs(2),
    )
    .map_err(|error| format!("connect Neural DX bridge: {error}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
    write!(
        stream,
        "POST /api/nexus/sync HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nX-Nexus-Token: {secret}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .and_then(|_| stream.write_all(&body))
    .map_err(|error| format!("write Neural DX bridge: {error}"))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| format!("read Neural DX bridge: {error}"))?;
    let status = response.lines().next().unwrap_or_default();
    if !status.contains(" 200 ") {
        return Err(format!("Neural DX bridge returned {status}"));
    }
    Ok(())
}

#[tauri::command]
pub fn neural_dx_actions(state: State<'_, NeuralDxState>) -> Result<Vec<Value>, String> {
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
    let address = url.trim_start_matches("http://");
    let mut stream = TcpStream::connect_timeout(
        &address
            .parse()
            .map_err(|error| format!("parse Neural DX address: {error}"))?,
        Duration::from_secs(2),
    )
    .map_err(|error| format!("connect Neural DX bridge: {error}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
    write!(
        stream,
        "GET /api/nexus/actions HTTP/1.1\r\nHost: {address}\r\nX-Nexus-Token: {secret}\r\nConnection: close\r\n\r\n"
    )
    .map_err(|error| format!("write Neural DX action request: {error}"))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| format!("read Neural DX action response: {error}"))?;
    let status = response.lines().next().unwrap_or_default();
    if !status.contains(" 200 ") {
        return Err(format!("Neural DX bridge returned {status}"));
    }
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .ok_or("Neural DX bridge returned an invalid HTTP response")?;
    let payload: Value =
        serde_json::from_str(body).map_err(|error| format!("decode Neural DX actions: {error}"))?;
    Ok(payload
        .get("actions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}
