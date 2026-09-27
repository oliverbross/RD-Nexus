//! OpenHamClock process ownership for the desktop application.
//!
//! OpenHamClock is a complete React + Express application, not a collection of
//! widgets. Nexus therefore runs the pinned upstream build as a localhost-only
//! companion process and embeds its unmodified application surface in the Ham
//! Clock view. The child has its own writable directory and dies with Nexus.

use serde::Serialize;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State};
use tempo_app::engine::engine_lock;

use crate::SharedEngine;

const VERSION: &str = "26.8.0";
const PREFERRED_PORT: u16 = 39_073;

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
        if let Some(child) = runtime.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn set_status(state: &OpenHamClockState, status: OpenHamClockStatus) {
    state.0.lock().unwrap_or_else(|e| e.into_inner()).status = status;
}

fn reserve_port() -> Result<u16, String> {
    // Keep the iframe origin stable so OpenHamClock's layout, preferences and
    // service-worker storage survive a Nexus restart. A second simultaneous
    // Nexus instance falls back to an ephemeral loopback port rather than
    // preventing either app from starting.
    TcpListener::bind(("127.0.0.1", PREFERRED_PORT))
        .or_else(|_| TcpListener::bind(("127.0.0.1", 0)))
        .and_then(|s| s.local_addr())
        .map(|a| a.port())
        .map_err(|e| format!("reserve a localhost port: {e}"))
}

fn bundled_executable(resource_dir: &Path) -> PathBuf {
    resource_dir.join("openhamclock").join(if cfg!(windows) {
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
        return Ok(Command::new(bundled));
    }

    Err("OpenHamClock is not staged; run `node scripts/stage-openhamclock.mjs` before starting Nexus".into())
}

fn launch(app: &AppHandle) -> Result<(Child, String), String> {
    let port = reserve_port()?;
    let home = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("resolve app data directory: {e}"))?
        .join("openhamclock");
    std::fs::create_dir_all(&home)
        .map_err(|e| format!("create OpenHamClock data directory: {e}"))?;
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
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // pkg emits a console-subsystem executable. Keep that implementation
        // detail invisible so Nexus remains the only application window.
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
        .env("HOST", "127.0.0.1")
        .env("PORT", port.to_string())
        .env("OPENHAMCLOCK_HOME", home)
        .env("OHC_NEXUS_EMBED", "1")
        .env("OHC_NEXUS_PARENT_PID", std::process::id().to_string())
        .env("CALLSIGN", &callsign)
        .env("LOCATOR", &grid)
        .env("DX_CLUSTER_CALLSIGN", &callsign)
        .env("AUTO_UPDATE_ENABLED", "false")
        .env("OPEN_BROWSER", "false")
        .env("WSJTX_ENABLED", "false")
        .env("N1MM_UDP_ENABLED", "false")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    let child = command
        .spawn()
        .map_err(|e| format!("start OpenHamClock: {e}"))?;
    Ok((child, format!("http://127.0.0.1:{port}")))
}

pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("openhamclock".into())
        .spawn(move || {
            let state = app.state::<OpenHamClockState>();
            let (child, url) = match launch(&app) {
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
                runtime.status.url = Some(url.clone());
            }

            let deadline = Instant::now() + Duration::from_secs(45);
            let address = url
                .trim_start_matches("http://")
                .parse()
                .map_err(|e| format!("parse OpenHamClock address: {e}"));
            let address = match address {
                Ok(address) => address,
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
                if TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_ok() {
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
                std::thread::sleep(Duration::from_millis(150));
            }
            {
                let mut runtime = state.0.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(child) = runtime.child.as_mut() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                runtime.child = None;
            }
            set_status(
                &state,
                OpenHamClockStatus {
                    phase: "unavailable",
                    url: Some(url),
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
        if let Some(child) = runtime.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        runtime.child = None;
        runtime.status = OpenHamClockStatus {
            phase: "starting",
            url: None,
            version: VERSION,
            detail: None,
        };
    }
    start(app);
}
