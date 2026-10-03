//! OpenHamClock process ownership for the desktop application.
//!
//! OpenHamClock is a complete React + Express application, not a collection of
//! widgets. Nexus therefore runs the pinned upstream build as a localhost-only
//! companion process and embeds its unmodified application surface in the Ham
//! Clock view. The child has its own writable directory and dies with Nexus.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State};
use tempo_app::dto::{AppSnapshot, LoggedQso};
use tempo_app::engine::engine_lock;
use tempo_core::logbook::QsoEdit;
use tempo_core::process::command;

use crate::SharedEngine;

const VERSION: &str = "26.8.0";
const PREFERRED_PORT: u16 = 39_073;
const MAX_HEALTH_RESPONSE_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenHamClockStatus {
    phase: &'static str,
    url: Option<String>,
    version: &'static str,
    detail: Option<String>,
}

struct Runtime {
    child: Option<Child>,
    status: OpenHamClockStatus,
}

pub struct OpenHamClockState(Mutex<Runtime>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenHamClockLogDelta {
    revision: u64,
    reset: bool,
    upserts: Vec<OpenHamClockLogRow>,
    deleted: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenHamClockLogRow {
    row: LoggedQso,
    edit_key: String,
}

impl Default for OpenHamClockState {
    fn default() -> Self {
        Self(Mutex::new(Runtime {
            child: None,
            status: OpenHamClockStatus {
                phase: "idle",
                url: None,
                version: VERSION,
                detail: None,
            },
        }))
    }
}

impl Drop for OpenHamClockState {
    fn drop(&mut self) {
        let runtime = self.0.get_mut().unwrap_or_else(|e| e.into_inner());
        stop_runtime(runtime);
    }
}

fn stop_runtime(runtime: &mut Runtime) {
    if let Some(child) = runtime.child.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
    runtime.child = None;
}

/// Stop the owned launcher during Tauri's synchronous quit cleanup. OpenHamClock's packaged
/// worker also watches Nexus's PID so forced termination cannot strand it.
pub fn stop(state: &OpenHamClockState) {
    let mut runtime = state.0.lock().unwrap_or_else(|e| e.into_inner());
    stop_runtime(&mut runtime);
}

fn set_status(state: &OpenHamClockState, status: OpenHamClockStatus) {
    state.0.lock().unwrap_or_else(|e| e.into_inner()).status = status;
}

#[derive(Deserialize)]
struct PortHandshake {
    port: u16,
}

#[derive(Deserialize)]
struct HealthResponse {
    status: String,
    version: String,
}

fn health_ready(address: std::net::SocketAddr) -> bool {
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(300)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    if stream
        .write_all(b"GET /api/health?format=json HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nAccept: application/json\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut response = Vec::new();
    if stream
        .take(MAX_HEALTH_RESPONSE_BYTES + 1)
        .read_to_end(&mut response)
        .is_err()
        || response.len() as u64 > MAX_HEALTH_RESPONSE_BYTES
    {
        return false;
    }
    let Some(header_end) = response.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
        return false;
    };
    let Ok(headers) = std::str::from_utf8(&response[..header_end]) else {
        return false;
    };
    let Some(status_line) = headers.lines().next() else {
        return false;
    };
    if status_line != "HTTP/1.1 200 OK" && status_line != "HTTP/1.0 200 OK" {
        return false;
    }
    let Ok(health) = serde_json::from_slice::<HealthResponse>(&response[header_end + 4..]) else {
        return false;
    };
    health.status == "ok" && health.version == VERSION
}

fn bundled_executable(resource_dir: &Path) -> PathBuf {
    resource_dir
        .join("resources")
        .join("openhamclock")
        .join(if cfg!(windows) {
            "openhamclock.exe"
        } else {
            "openhamclock"
        })
}

fn child_command(app: &AppHandle) -> Result<Command, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("resolve resource directory: {e}"))?;
    let bundled = bundled_executable(&resource_dir);
    if bundled.is_file() {
        return Ok(command(bundled));
    }

    Err("OpenHamClock is not staged; run `node scripts/stage-openhamclock.mjs` before starting Nexus".into())
}

fn launch(app: &AppHandle) -> Result<(Child, PathBuf), String> {
    let home = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("resolve app data directory: {e}"))?
        .join("openhamclock");
    std::fs::create_dir_all(&home)
        .map_err(|e| format!("create OpenHamClock data directory: {e}"))?;
    let port_file = home.join(format!("nexus-{}.port.json", std::process::id()));
    let _ = std::fs::remove_file(&port_file);
    let log = std::fs::File::create(home.join("openhamclock.log"))
        .map_err(|e| format!("create OpenHamClock log: {e}"))?;
    let log_err = log
        .try_clone()
        .map_err(|e| format!("open OpenHamClock error log: {e}"))?;
    let (callsign, grid) = {
        let state = app.state::<SharedEngine>();
        let engine = engine_lock(&state);
        (
            engine.settings().mycall.clone(),
            engine.settings().mygrid.clone(),
        )
    };

    let mut command = child_command(app)?;
    command
        .env("HOST", "127.0.0.1")
        .env("PORT", PREFERRED_PORT.to_string())
        .env("OPENHAMCLOCK_HOME", &home)
        .env("OHC_NEXUS_PORT_FILE", &port_file)
        .env("OHC_NEXUS_EMBED", "1")
        .env("OHC_NEXUS_PARENT_PID", std::process::id().to_string())
        .env("CALLSIGN", &callsign)
        .env("LOCATOR", &grid)
        .env("DX_CLUSTER_CALLSIGN", &callsign)
        .env("OPEN_BROWSER", "false")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    let child = command
        .spawn()
        .map_err(|e| format!("start OpenHamClock: {e}"))?;
    Ok((child, port_file))
}

pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("openhamclock".into())
        .spawn(move || {
            let state = app.state::<OpenHamClockState>();
            let (child, port_file) = match launch(&app) {
                Ok(v) => v,
                Err(detail) => {
                    set_status(
                        &state,
                        OpenHamClockStatus {
                            phase: "unavailable",
                            url: None,
                            version: VERSION,
                            detail: Some(detail),
                        },
                    );
                    return;
                }
            };
            {
                let mut runtime = state.0.lock().unwrap_or_else(|e| e.into_inner());
                runtime.child = Some(child);
            }

            let deadline = Instant::now() + Duration::from_secs(45);
            let mut last_url = None;
            while Instant::now() < deadline {
                let exited = {
                    let mut runtime = state.0.lock().unwrap_or_else(|e| e.into_inner());
                    runtime
                        .child
                        .as_mut()
                        .and_then(|child| child.try_wait().ok().flatten())
                };
                if let Some(exit) = exited {
                    set_status(
                        &state,
                        OpenHamClockStatus {
                            phase: "unavailable",
                            url: None,
                            version: VERSION,
                            detail: Some(format!(
                                "OpenHamClock exited before becoming ready: {exit}"
                            )),
                        },
                    );
                    return;
                }
                if let Ok(bytes) = std::fs::read(&port_file) {
                    if let Ok(handshake) = serde_json::from_slice::<PortHandshake>(&bytes) {
                        let url = format!("http://127.0.0.1:{}", handshake.port);
                        last_url = Some(url.clone());
                        if health_ready(([127, 0, 0, 1], handshake.port).into()) {
                            set_status(
                                &state,
                                OpenHamClockStatus {
                                    phase: "ready",
                                    url: Some(url),
                                    version: VERSION,
                                    detail: None,
                                },
                            );
                            return;
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            {
                let mut runtime = state.0.lock().unwrap_or_else(|e| e.into_inner());
                stop_runtime(&mut runtime);
            }
            set_status(
                &state,
                OpenHamClockStatus {
                    phase: "unavailable",
                    url: last_url,
                    version: VERSION,
                    detail: Some("OpenHamClock did not become ready within 45 seconds".into()),
                },
            );
        })
        .expect("spawn OpenHamClock supervisor");
}

#[tauri::command]
pub fn openhamclock_status(
    app: AppHandle,
    state: State<'_, OpenHamClockState>,
) -> OpenHamClockStatus {
    let mut runtime = state.0.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(exit) = runtime
        .child
        .as_mut()
        .and_then(|child| child.try_wait().ok().flatten())
    {
        runtime.child = None;
        runtime.status = OpenHamClockStatus {
            phase: "unavailable",
            url: None,
            version: VERSION,
            detail: Some(format!("OpenHamClock exited: {exit}")),
        };
    }
    let should_start = runtime.status.phase == "idle";
    if should_start {
        runtime.status.phase = "starting";
    }
    let status = runtime.status.clone();
    drop(runtime);
    if should_start {
        start(app);
    }
    status
}

#[tauri::command]
pub fn restart_openhamclock(app: AppHandle, state: State<'_, OpenHamClockState>) {
    {
        let mut runtime = state.0.lock().unwrap_or_else(|e| e.into_inner());
        stop_runtime(&mut runtime);
        runtime.status = OpenHamClockStatus {
            phase: "starting",
            url: None,
            version: VERSION,
            detail: None,
        };
    }
    start(app);
}

/// Treat PTT from the embedded upstream application as an external-program
/// request. The engine's broker arbiter is the single authority for operator
/// opt-in, TX arm, licence/band legality, ownership and watchdog state. An
/// un-key request is always accepted by that arbiter.
#[tauri::command]
pub fn openhamclock_set_ptt(
    state: State<'_, SharedEngine>,
    on: bool,
) -> Result<AppSnapshot, String> {
    let mut engine = engine_lock(&state);
    let granted = engine.openhamclock_ptt(on);
    let snapshot = engine.snapshot();
    if on && !granted {
        return Err("Nexus transmit safety gate refused the OpenHamClock PTT request".into());
    }
    Ok(snapshot)
}

/// A compact, bounded log mutation feed for an already initialized Ham Clock.
/// A revision outside the core log's recent-change window returns `reset=true`;
/// the UI then performs the same capped/paged cold load used at startup.
#[tauri::command]
pub async fn openhamclock_log_delta(
    state: State<'_, SharedEngine>,
    since_revision: u64,
) -> Result<OpenHamClockLogDelta, String> {
    let (revision, ids, plan) = {
        let engine = engine_lock(&state);
        (
            engine.log_revision(),
            engine.log_changed_ids_since(since_revision),
            engine.log_view(),
        )
    };
    let Some(ids) = ids else {
        return Ok(OpenHamClockLogDelta {
            revision,
            reset: true,
            upserts: Vec::new(),
            deleted: Vec::new(),
        });
    };
    let read_ids = ids.clone();
    let rows = tauri::async_runtime::spawn_blocking(move || plan.rows(&read_ids))
        .await
        .map_err(|e| format!("join OpenHamClock log delta read: {e}"))??;
    let mut upserts = Vec::with_capacity(rows.len());
    let mut deleted = Vec::new();
    for id in ids {
        match rows.get(&id) {
            Some(record) => upserts.push(OpenHamClockLogRow {
                row: LoggedQso::from((**record).clone()),
                edit_key: QsoEdit::project(record).key(),
            }),
            None => deleted.push(id.to_string()),
        }
    }
    Ok(OpenHamClockLogDelta {
        revision,
        reset: false,
        upserts,
        deleted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn bundled_executable_matches_the_tauri_resource_layout() {
        let expected = Path::new("/bundle")
            .join("resources")
            .join("openhamclock")
            .join(if cfg!(windows) {
                "openhamclock.exe"
            } else {
                "openhamclock"
            });
        assert_eq!(bundled_executable(Path::new("/bundle")), expected);
    }

    fn health_response(status: &'static str, body: Vec<u8>) -> std::net::SocketAddr {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind health fixture");
        let address = listener.local_addr().expect("fixture address");
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept health probe");
            let mut request = [0u8; 512];
            let _ = stream.read(&mut request);
            let response = format!(
                "{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                status,
                body.len(),
                String::from_utf8_lossy(&body)
            );
            stream
                .write_all(response.as_bytes())
                .expect("write health fixture");
        });
        address
    }

    #[test]
    fn readiness_requires_the_expected_application_health_and_version() {
        assert!(health_ready(health_response(
            "HTTP/1.1 200 OK",
            br#"{"status":"ok","version":"26.8.0"}"#.to_vec(),
        )));
        assert!(!health_ready(health_response(
            "HTTP/1.1 200 OK",
            br#"{"status":"ok","version":"0.0.0"}"#.to_vec(),
        )));
        assert!(!health_ready(health_response(
            "HTTP/1.1 200 OK",
            br#"not-json-but-contains-\"status\":\"ok\"-and-\"version\":\"26.8.0\""#.to_vec(),
        )));
        assert!(!health_ready(health_response(
            "HTTP/1.1 503 Service Unavailable",
            br#"{"status":"ok","version":"26.8.0"}"#.to_vec(),
        )));
        assert!(!health_ready(health_response(
            "HTTP/1.1 200 OK",
            vec![b' '; MAX_HEALTH_RESPONSE_BYTES as usize + 1],
        )));
    }
}
