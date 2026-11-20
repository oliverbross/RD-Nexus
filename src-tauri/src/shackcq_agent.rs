// SPDX-License-Identifier: GPL-3.0-only
//! ShackCQ Web's outbound, receive-safe station agent. Nexus remains the only CAT owner: this
//! task reads Nexus snapshots and submits bounded receive controls back through the Engine.

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::State;
use tempo_app::engine::engine_lock;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

use super::shackcq_connector::{load_pairing, machine_token};
use super::{settings_path, SharedEngine};

const PROTOCOL_MINOR: u8 = 3;
const ROTATOR_PROTOCOL_MINOR: u8 = 1;
const DEFAULT_ROTCTLD_PORT: u16 = 4533;
static RUNNING: AtomicBool = AtomicBool::new(false);
static STATUS: OnceLock<Mutex<AgentStatus>> = OnceLock::new();

#[derive(Clone, Debug)]
struct RotatorEndpoint {
    device_id: String,
    address: String,
    model: String,
}

#[derive(Clone, Debug)]
struct PreparedRotatorTarget {
    id: String,
    azimuth: f64,
    elevation: f64,
    valid_until: chrono::DateTime<chrono::Utc>,
}

#[derive(Default)]
struct RotatorRuntime {
    prepared: Option<PreparedRotatorTarget>,
    target: Option<(f64, f64)>,
    state: &'static str,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    running: bool,
    connected: bool,
    generation: Option<u64>,
    last_error: Option<&'static str>,
}

fn status_cell() -> &'static Mutex<AgentStatus> {
    STATUS.get_or_init(|| Mutex::new(AgentStatus::default()))
}

fn set_status(update: impl FnOnce(&mut AgentStatus)) {
    if let Ok(mut status) = status_cell().lock() {
        update(&mut status);
    }
}

#[tauri::command]
pub fn shackcq_agent_status() -> AgentStatus {
    status_cell()
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default()
}

#[tauri::command]
pub fn shackcq_start_agent(state: State<'_, SharedEngine>) -> Result<AgentStatus, &'static str> {
    let pairing = load_pairing()?.ok_or("connectorNotPaired")?;
    let token = machine_token(&pairing.auth.credential_alias)?;
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Ok(shackcq_agent_status());
    }
    set_status(|status| {
        status.running = true;
        status.last_error = None;
    });
    let engine = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let mut backoff = 1u64;
        while RUNNING.load(Ordering::SeqCst) {
            match run_once(&pairing, &token, engine.clone()).await {
                Ok(()) => backoff = 1,
                Err("accessDenied") => {
                    set_status(|status| status.last_error = Some("accessDenied"));
                    break;
                }
                Err(code) => {
                    set_status(|status| status.last_error = Some(code));
                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(30);
                }
            }
        }
        RUNNING.store(false, Ordering::SeqCst);
        set_status(|status| {
            status.running = false;
            status.connected = false;
            status.generation = None;
        });
    });
    Ok(shackcq_agent_status())
}

#[tauri::command]
pub fn shackcq_stop_agent() -> AgentStatus {
    RUNNING.store(false, Ordering::SeqCst);
    set_status(|status| status.running = false);
    shackcq_agent_status()
}

async fn run_once(
    pairing: &super::shackcq_connector::PairingRecord,
    token: &str,
    engine: SharedEngine,
) -> Result<(), &'static str> {
    if let Some(i) = super::acceptance::get() {
        let expected =
            i.manifest.web_origin.replacen("http://", "ws://", 1) + "/api/v1/agent/connect";
        if pairing.connect_url != expected {
            return Err("fixtureOriginDenied");
        }
    }
    let mut request = pairing
        .connect_url
        .as_str()
        .into_client_request()
        .map_err(|_| "configurationInvalid")?;
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {token}")
            .parse()
            .map_err(|_| "invalidConnectorCredential")?,
    );
    let connect = tokio_tungstenite::connect_async_with_config(request, None, false);
    let (socket, _) = match tokio::time::timeout(Duration::from_secs(10), connect).await {
        Ok(Ok(value)) => value,
        Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response)))
            if [401, 403].contains(&response.status().as_u16()) =>
        {
            return Err("accessDenied")
        }
        _ => return Err("serviceUnavailable"),
    };
    let (mut sink, mut source) = socket.split();
    sink.send(Message::Text(
        agent_hello(pairing, &engine).to_string().into(),
    ))
    .await
    .map_err(|_| "serviceUnavailable")?;
    let accepted = tokio::time::timeout(Duration::from_secs(5), source.next())
        .await
        .map_err(|_| "serviceUnavailable")?
        .ok_or("serviceUnavailable")?
        .map_err(|_| "serviceUnavailable")?;
    let accepted = message_json(accepted)?;
    if accepted.get("type").and_then(Value::as_str) != Some("agent.accepted")
        || accepted.get("agentId").and_then(Value::as_str) != Some(pairing.agent_id.as_str())
    {
        return Err("invalidResponse");
    }
    let generation = accepted
        .get("generation")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or("invalidResponse")?;
    set_status(|status| {
        status.connected = true;
        status.generation = Some(generation);
        status.last_error = None;
    });
    let mut sequence = 0u64;
    let mut rotator_sequence = 0u64;
    let mut rotator = RotatorRuntime {
        state: "IDLE",
        ..RotatorRuntime::default()
    };
    let mut telemetry = tokio::time::interval(Duration::from_secs(2));
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    loop {
        tokio::select! {
            _ = telemetry.tick() => {
                if !RUNNING.load(Ordering::SeqCst) { break; }
                sequence += 1;
                let snapshot = radio_snapshot(pairing, &engine, generation, sequence);
                sink.send(Message::Text(snapshot.to_string().into())).await.map_err(|_| "serviceUnavailable")?;
                if let Some(endpoint) = rotator_endpoint(pairing, &engine) {
                    rotator_sequence += 1;
                    let snapshot = rotator_snapshot(pairing, &endpoint, generation, rotator_sequence, &mut rotator).await;
                    sink.send(Message::Text(snapshot.to_string().into())).await.map_err(|_| "serviceUnavailable")?;
                }
            }
            _ = heartbeat.tick() => {
                sink.send(Message::Text(json!({
                    "type":"agent.heartbeat",
                    "protocol":{"major":1,"minor":PROTOCOL_MINOR},
                    "generation":generation,
                }).to_string().into())).await.map_err(|_| "serviceUnavailable")?;
            }
            next = source.next() => {
                let message = next.ok_or("serviceUnavailable")?.map_err(|_| "serviceUnavailable")?;
                if message.is_close() { break; }
                let value = message_json(message)?;
                if value.get("type").and_then(Value::as_str) == Some("radio.command") {
                    let result = apply_radio_command(pairing, &engine, generation, &value).await;
                    // Publish the controller-observed state before resolving the Web request.
                    // The hosted endpoint includes this presence in its response, so the browser
                    // never labels the Engine's optimistic request as radio readback.
                    sequence += 1;
                    let snapshot = radio_snapshot(pairing, &engine, generation, sequence);
                    sink.send(Message::Text(snapshot.to_string().into())).await.map_err(|_| "serviceUnavailable")?;
                    sink.send(Message::Text(result.to_string().into())).await.map_err(|_| "serviceUnavailable")?;
                } else if value.get("type").and_then(Value::as_str) == Some("rotator.command") {
                    let endpoint = rotator_endpoint(pairing, &engine);
                    let result = apply_rotator_command(pairing, endpoint, generation, &mut rotator, &value).await;
                    sink.send(Message::Text(result.to_string().into())).await.map_err(|_| "serviceUnavailable")?;
                }
            }
        }
    }
    set_status(|status| {
        status.connected = false;
        status.generation = None;
    });
    Ok(())
}

fn message_json(message: Message) -> Result<Value, &'static str> {
    match message {
        Message::Text(text) if text.len() <= 64 * 1024 => {
            serde_json::from_str(&text).map_err(|_| "invalidResponse")
        }
        _ => Err("invalidResponse"),
    }
}

fn agent_hello(pairing: &super::shackcq_connector::PairingRecord, engine: &SharedEngine) -> Value {
    let current = engine_lock(engine);
    let snapshot = current.snapshot();
    let settings = current.settings();
    let configured = settings
        .radios
        .iter()
        .find(|radio| radio.id == settings.active_radio);
    let name = snapshot
        .radios
        .iter()
        .find(|radio| radio.is_active)
        .map(|radio| radio.name.as_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Nexus radio");
    let model_id = configured.map_or(settings.rig_model, |radio| radio.rig_model);
    let model = configured
        .map(|radio| radio.rig_model_name.as_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(settings.rig_model_name.as_str());
    let model = if model.trim().is_empty() {
        format!("Hamlib model {model_id}")
    } else {
        model.trim().to_string()
    };
    let manufacturer = model.split_whitespace().next().unwrap_or("Unknown");
    let capabilities = radio_capabilities(model_id, manufacturer, &model);
    let rotators = rotator_endpoint_from_settings(pairing, settings)
        .map(|endpoint| vec![rotator_hello(&endpoint)])
        .unwrap_or_default();
    json!({
        "type":"agent.hello",
        "protocol":{"major":1,"minor":PROTOCOL_MINOR},
        "agentId":pairing.agent_id,
        "platform":std::env::consts::OS,
        "version":env!("CARGO_PKG_VERSION"),
        "build":"Nexus v1.14.0 / ShackCQ review",
        "devices":[{
            "id":pairing.agent_id,
            "name":name,
            "hamlibModelId":model_id,
            "manufacturer":manufacturer,
            "model":model,
            "capabilities":capabilities,
        }],
        "rotators":rotators,
    })
}

fn rotator_endpoint(
    pairing: &super::shackcq_connector::PairingRecord,
    engine: &SharedEngine,
) -> Option<RotatorEndpoint> {
    let current = engine_lock(engine);
    rotator_endpoint_from_settings(pairing, current.settings())
}

#[cfg(feature = "radio")]
fn rotator_endpoint_from_settings(
    pairing: &super::shackcq_connector::PairingRecord,
    settings: &tempo_app::settings::Settings,
) -> Option<RotatorEndpoint> {
    let external = settings.rotator_host.trim();
    if super::acceptance::get().is_some_and(|i| external != i.manifest.rotor_address) {
        return None;
    }
    let (address, model) = if !external.is_empty() {
        (external.to_string(), "External rotctld".to_string())
    } else if settings.rotator_model > 0 {
        let port = settings
            .radios
            .iter()
            .find(|profile| profile.id == settings.active_radio)
            .map(|profile| profile.rotctld_port)
            .filter(|port| *port != 0)
            .unwrap_or(DEFAULT_ROTCTLD_PORT);
        (
            format!("127.0.0.1:{port}"),
            format!("Hamlib model {}", settings.rotator_model),
        )
    } else {
        return None;
    };
    Some(RotatorEndpoint {
        device_id: format!("{}:rotator", pairing.agent_id),
        address,
        model,
    })
}

#[cfg(not(feature = "radio"))]
fn rotator_endpoint_from_settings(
    _pairing: &super::shackcq_connector::PairingRecord,
    _settings: &tempo_app::settings::Settings,
) -> Option<RotatorEndpoint> {
    None
}

fn rotator_capabilities(endpoint: &RotatorEndpoint) -> Value {
    json!({
        "backend":"Nexus rotctld",
        "model":endpoint.model,
        "azimuth":true,
        // The retained Nexus controller accepts az/el, but the current Settings projection does
        // not prove that the selected physical mount has an elevation axis. Do not advertise one.
        "elevation":false,
        "manualJog":false,
        "park":false,
        "stop":true,
        "minimumAzimuth":0.0,
        "maximumAzimuth":360.0,
        "minimumElevation":0.0,
        "maximumElevation":0.0,
    })
}

fn rotator_hello(endpoint: &RotatorEndpoint) -> Value {
    json!({
        "id":endpoint.device_id,
        "name":"Nexus rotator",
        "backend":"Nexus rotctld",
        "model":endpoint.model,
        "capabilities":rotator_capabilities(endpoint),
    })
}

async fn read_rotator_position(endpoint: &RotatorEndpoint) -> Option<(f64, Option<f64>)> {
    #[cfg(feature = "radio")]
    {
        let address = endpoint.address.clone();
        tauri::async_runtime::spawn_blocking(move || tempo_audio::rotator::read_position(&address))
            .await
            .ok()
            .and_then(Result::ok)
    }
    #[cfg(not(feature = "radio"))]
    {
        let _ = endpoint;
        None
    }
}

async fn rotator_snapshot(
    pairing: &super::shackcq_connector::PairingRecord,
    endpoint: &RotatorEndpoint,
    generation: u64,
    sequence: u64,
    runtime: &mut RotatorRuntime,
) -> Value {
    if runtime
        .prepared
        .as_ref()
        .is_some_and(|prepared| prepared.valid_until < chrono::Utc::now())
    {
        runtime.prepared = None;
        runtime.state = "IDLE";
    }
    let position = read_rotator_position(endpoint).await;
    if let (Some((azimuth, elevation)), Some((target_azimuth, target_elevation))) =
        (position, runtime.target)
    {
        let az_error = (azimuth - target_azimuth)
            .abs()
            .min(360.0 - (azimuth - target_azimuth).abs().min(360.0));
        let el_matches = elevation.is_none_or(|value| (value - target_elevation).abs() <= 2.0);
        if az_error <= 2.0 && el_matches {
            runtime.target = None;
            runtime.state = "IDLE";
        }
    }
    let (azimuth, elevation) = position
        .map(|(azimuth, elevation)| (json!(azimuth), elevation.map_or(Value::Null, |v| json!(v))))
        .unwrap_or((Value::Null, Value::Null));
    let (target_azimuth, target_elevation) = runtime
        .target
        .or_else(|| runtime.prepared.as_ref().map(|p| (p.azimuth, p.elevation)))
        .map(|(azimuth, elevation)| (json!(azimuth), json!(elevation)))
        .unwrap_or((Value::Null, Value::Null));
    json!({
        "type":"rotator.snapshot",
        "protocol":{"major":1,"minor":ROTATOR_PROTOCOL_MINOR},
        "agentId":pairing.agent_id,
        "deviceId":endpoint.device_id,
        "generation":generation,
        "sequence":sequence,
        "observedUtc":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "connection":if position.is_some() { "live" } else { "stale" },
        "state":if position.is_some() { runtime.state } else { "OUTCOME_UNKNOWN" },
        "azimuth":azimuth,
        "elevation":elevation,
        "targetAzimuth":target_azimuth,
        "targetElevation":target_elevation,
        "positionFresh":position.is_some(),
        "capabilities":rotator_capabilities(endpoint),
    })
}

fn radio_capabilities(model_id: u32, manufacturer: &str, model: &str) -> Value {
    json!({
        "modelId":model_id,
        "manufacturer":manufacturer,
        "model":model,
        "backend":"Nexus CAT",
        "frequencyRangesHz":[],
        "modes":[],
        "filtersHz":[],
        "setters":["radio.set.frequency","radio.set.mode","preset.recall"],
        "meters":["strengthDb"],
        "readOnlyTxState":true,
        "rotatorEnvelope":"DECLARED_NO_MOVEMENT",
    })
}

fn radio_snapshot(
    pairing: &super::shackcq_connector::PairingRecord,
    engine: &SharedEngine,
    generation: u64,
    sequence: u64,
) -> Value {
    let current = engine_lock(engine);
    let snapshot = current.snapshot();
    let settings = current.settings();
    let configured = settings
        .radios
        .iter()
        .find(|radio| radio.id == settings.active_radio);
    let model_id = configured.map_or(settings.rig_model, |radio| radio.rig_model);
    let model = configured
        .map(|radio| radio.rig_model_name.as_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(settings.rig_model_name.as_str());
    let model = if model.trim().is_empty() {
        format!("Hamlib model {model_id}")
    } else {
        model.trim().to_string()
    };
    let manufacturer = model.split_whitespace().next().unwrap_or("Unknown");
    // The active Nexus owner retains the last successful CAT observation separately from its
    // requested dial. A refused or queued set_frequency must never look like rig readback.
    let observed_frequency = current.rig_observed_frequency_hz();
    let connection = if snapshot.radio.cat_ok == Some(true) && observed_frequency.is_some() {
        "live"
    } else {
        "stale"
    };
    let mut meters = serde_json::Map::new();
    if let Some(value) = snapshot.radio.smeter_db {
        meters.insert("strengthDb".into(), json!(value));
    }
    json!({
        "type":"radio.snapshot",
        "protocol":{"major":1,"minor":PROTOCOL_MINOR},
        "agentId":pairing.agent_id,
        "deviceId":pairing.agent_id,
        "generation":generation,
        "sequence":sequence,
        "observedUtc":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "connection":connection,
        "frequencyHz":observed_frequency,
        "mode":snapshot.radio.rig_mode,
        "filterHz":Value::Null,
        "meters":meters,
        "transmitting":snapshot.radio.transmitting || snapshot.radio.rig_keyed,
        "capabilities":radio_capabilities(model_id, manufacturer, &model),
    })
}

async fn apply_radio_command(
    pairing: &super::shackcq_connector::PairingRecord,
    engine: &SharedEngine,
    generation: u64,
    command: &Value,
) -> Value {
    let command_id = command
        .get("commandId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let device_id = command
        .get("deviceId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let expected = command.get("expectedGeneration").and_then(Value::as_u64);
    let command_type = command.get("type").and_then(Value::as_str);
    let agent_id = command.get("agentId").and_then(Value::as_str);
    let protocol = command.get("protocol").and_then(Value::as_object);
    let protocol_valid = protocol.is_some_and(|value| {
        value.get("major").and_then(Value::as_u64) == Some(1)
            && value
                .get("minor")
                .and_then(Value::as_u64)
                .is_some_and(|minor| minor <= u64::from(PROTOCOL_MINOR))
    });
    let valid_until = command
        .get("validUntilUtc")
        .and_then(Value::as_str)
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&chrono::Utc));
    let now = chrono::Utc::now();
    let command_fresh = valid_until
        .is_some_and(|until| until > now && until <= now + chrono::Duration::seconds(30));
    let action = command.get("action").and_then(Value::as_str).unwrap_or("");
    let parameters = command.get("parameters").and_then(Value::as_object);
    let valid_id = |value: &str| {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
    };
    let mut ok = false;
    let mut code = "COMMAND_INVALID";
    let mut expected_frequency = None;
    let mut expected_mode: Option<String> = None;
    let mut observation_before = (0, 0);
    if command.get("validUntilUtc").is_some() && !command_fresh {
        code = "STALE_COMMAND_REJECTED";
    } else if expected.is_some() && expected != Some(generation) {
        code = "STALE_GENERATION_REJECTED";
    }
    if valid_id(command_id)
        && command_type == Some("radio.command")
        && agent_id == Some(pairing.agent_id.as_str())
        && device_id == pairing.agent_id
        && expected == Some(generation)
        && protocol_valid
        && command_fresh
        && parameters.is_some()
        && serde_json::to_vec(parameters.expect("checked parameters"))
            .is_ok_and(|encoded| encoded.len() <= 4_096)
    {
        let parameters = parameters.expect("checked");
        let mut current = engine_lock(engine);
        let snapshot = current.snapshot();
        let apply = match action {
            "radio.set.frequency" => parameters
                .get("frequencyHz")
                .and_then(Value::as_u64)
                .and_then(|hz| {
                    let mhz = hz as f64 / 1_000_000.0;
                    tempo_app::bandplan::band_for_dial(mhz)
                        .map(|band| (mhz, band.to_string(), snapshot.radio.sideband.clone()))
                }),
            "radio.set.mode" => parameters
                .get("mode")
                .and_then(Value::as_str)
                .filter(|mode| !mode.is_empty() && mode.len() <= 20)
                .map(|mode| {
                    (
                        snapshot.radio.dial_mhz,
                        snapshot.radio.band.clone(),
                        mode.to_string(),
                    )
                }),
            "preset.recall" => parameters
                .get("frequencyHz")
                .and_then(Value::as_u64)
                .zip(parameters.get("mode").and_then(Value::as_str))
                .and_then(|(hz, mode)| {
                    let mhz = hz as f64 / 1_000_000.0;
                    tempo_app::bandplan::band_for_dial(mhz)
                        .map(|band| (mhz, band.to_string(), mode.to_string()))
                }),
            _ => None,
        };
        if let Some((mhz, band, mode)) = apply {
            observation_before = current.rig_observation_revisions();
            if matches!(action, "radio.set.frequency" | "preset.recall") {
                expected_frequency = Some((mhz * 1_000_000.0).round() as u64);
            }
            if matches!(action, "radio.set.mode" | "preset.recall") {
                expected_mode = Some(mode.clone());
            }
            current.set_frequency(mhz, &band, &mode);
            let _ = current.settings().save(&settings_path());
            code = "READBACK_TIMEOUT";
        } else if ["radio.set.frequency", "radio.set.mode", "preset.recall"].contains(&action) {
            code = "PARAMETERS_INVALID";
        } else {
            code = "ACTION_NOT_ADVERTISED";
        }
    }
    if expected_frequency.is_some() || expected_mode.is_some() {
        for _ in 0..30 {
            let confirmed = {
                let current = engine_lock(engine);
                let snapshot = current.snapshot();
                let cat_live = snapshot.radio.cat_ok == Some(true);
                let observed = current.rig_observation_revisions();
                let fresh_frequency =
                    expected_frequency.is_none() || observed.0 != observation_before.0;
                let fresh_mode = expected_mode.is_none() || observed.1 != observation_before.1;
                let frequency_matches = expected_frequency.is_none_or(|expected| {
                    current
                        .rig_observed_frequency_hz()
                        .is_some_and(|observed| observed.abs_diff(expected) <= 10)
                });
                let mode_matches = expected_mode.as_ref().is_none_or(|expected| {
                    snapshot
                        .radio
                        .rig_mode
                        .as_deref()
                        .is_some_and(|observed| observed.eq_ignore_ascii_case(expected))
                });
                cat_live && fresh_frequency && fresh_mode && frequency_matches && mode_matches
            };
            if confirmed {
                ok = true;
                code = "READBACK_CONFIRMED";
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    let current = engine_lock(engine);
    let snapshot = current.snapshot();
    let observed_frequency = current.rig_observed_frequency_hz();
    json!({
        "type":"radio.command.result",
        "protocol":{"major":1,"minor":PROTOCOL_MINOR},
        "commandId":command_id,
        "agentId":pairing.agent_id,
        "deviceId":pairing.agent_id,
        "generation":generation,
        "ok":ok,
        "code":code,
        "readback":{
            "frequencyHz":observed_frequency,
            "mode":snapshot.radio.rig_mode,
        },
    })
}

fn protocol_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}

fn rotator_command_result(
    pairing: &super::shackcq_connector::PairingRecord,
    endpoint: Option<&RotatorEndpoint>,
    generation: u64,
    command_id: &str,
    ok: bool,
    code: &'static str,
    preparation_id: Option<&str>,
    position: Option<(f64, Option<f64>)>,
) -> Value {
    let device_id = endpoint
        .map(|value| value.device_id.clone())
        .unwrap_or_else(|| format!("{}:rotator", pairing.agent_id));
    let (azimuth, elevation) = position
        .map(|(azimuth, elevation)| (json!(azimuth), elevation.map_or(Value::Null, |v| json!(v))))
        .unwrap_or((Value::Null, Value::Null));
    json!({
        "type":"rotator.command.result",
        "protocol":{"major":1,"minor":ROTATOR_PROTOCOL_MINOR},
        "commandId":command_id,
        "agentId":pairing.agent_id,
        "deviceId":device_id,
        "generation":generation,
        "ok":ok,
        "code":code,
        "preparationId":preparation_id,
        "readback":{"azimuth":azimuth,"elevation":elevation},
    })
}

async fn apply_rotator_command(
    pairing: &super::shackcq_connector::PairingRecord,
    endpoint: Option<RotatorEndpoint>,
    generation: u64,
    runtime: &mut RotatorRuntime,
    command: &Value,
) -> Value {
    let command_id = command
        .get("commandId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let agent_id = command.get("agentId").and_then(Value::as_str).unwrap_or("");
    let control_instance_id = command
        .get("controlInstanceId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let protocol = command.get("protocol").and_then(Value::as_object);
    let expected_generation = command.get("expectedGeneration").and_then(Value::as_u64);
    let device_id = command
        .get("deviceId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let action = command.get("action").and_then(Value::as_str).unwrap_or("");
    let parameters = command.get("parameters").and_then(Value::as_object);
    let valid_until = command
        .get("validUntilUtc")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<chrono::DateTime<chrono::Utc>>().ok());
    let now = chrono::Utc::now();
    let fresh = valid_until
        .is_some_and(|until| until > now && until <= now + chrono::Duration::seconds(30));
    let scoped = endpoint
        .as_ref()
        .is_some_and(|endpoint| endpoint.device_id == device_id);
    let protocol_matches = protocol.is_some_and(|protocol| {
        protocol.get("major").and_then(Value::as_u64) == Some(1)
            && protocol
                .get("minor")
                .and_then(Value::as_u64)
                .is_some_and(|minor| minor <= u64::from(ROTATOR_PROTOCOL_MINOR))
    });
    let bounded_parameters = parameters
        .and_then(|parameters| serde_json::to_vec(parameters).ok())
        .is_some_and(|bytes| bytes.len() <= 2_048);
    if !protocol_id(command_id)
        || agent_id != pairing.agent_id
        || !protocol_id(control_instance_id)
        || !protocol_matches
        || expected_generation != Some(generation)
        || !scoped
        || !fresh
        || !bounded_parameters
    {
        let code = if valid_until.is_some() && !fresh {
            "STALE_COMMAND_REJECTED"
        } else if expected_generation != Some(generation) {
            "STALE_GENERATION_REJECTED"
        } else {
            "COMMAND_INVALID"
        };
        return rotator_command_result(
            pairing,
            endpoint.as_ref(),
            generation,
            command_id,
            false,
            code,
            None,
            None,
        );
    }
    let endpoint = endpoint.expect("scope checked");
    let parameters = parameters.expect("checked");
    match action {
        "rotator.target.prepare" => {
            let azimuth = parameters.get("azimuth").and_then(Value::as_f64);
            let elevation = parameters.get("elevation").and_then(Value::as_f64);
            let exact = parameters.len() == 2
                && parameters.contains_key("azimuth")
                && parameters.contains_key("elevation");
            if !exact
                || azimuth.is_none_or(|value| !(-180.0..=720.0).contains(&value))
                || elevation != Some(0.0)
            {
                return rotator_command_result(
                    pairing,
                    Some(&endpoint),
                    generation,
                    command_id,
                    false,
                    "PARAMETERS_UNSUPPORTED",
                    None,
                    None,
                );
            }
            let prepared = PreparedRotatorTarget {
                id: command_id.to_string(),
                azimuth: azimuth.expect("checked").rem_euclid(360.0),
                elevation: 0.0,
                valid_until: valid_until.expect("fresh checked"),
            };
            runtime.prepared = Some(prepared.clone());
            runtime.state = "PREPARED";
            rotator_command_result(
                pairing,
                Some(&endpoint),
                generation,
                command_id,
                true,
                "TARGET_PREPARED",
                Some(&prepared.id),
                read_rotator_position(&endpoint).await,
            )
        }
        "rotator.move.confirm" => {
            let preparation_id = parameters
                .get("preparationId")
                .and_then(Value::as_str)
                .unwrap_or("");
            let exact = parameters.len() == 1 && parameters.contains_key("preparationId");
            let prepared = runtime.prepared.as_ref().filter(|prepared| {
                exact
                    && protocol_id(preparation_id)
                    && prepared.id == preparation_id
                    && prepared.valid_until > now
            });
            let Some(prepared) = prepared.cloned() else {
                return rotator_command_result(
                    pairing,
                    Some(&endpoint),
                    generation,
                    command_id,
                    false,
                    "PREPARATION_INVALID_OR_EXPIRED",
                    None,
                    None,
                );
            };
            #[cfg(feature = "radio")]
            let applied = {
                let address = endpoint.address.clone();
                let azimuth = prepared.azimuth;
                let elevation = prepared.elevation;
                tauri::async_runtime::spawn_blocking(move || {
                    tempo_audio::rotator::point_azel(&address, azimuth, elevation)
                })
                .await
                .ok()
                .and_then(Result::ok)
                .is_some()
            };
            #[cfg(not(feature = "radio"))]
            let applied = false;
            if !applied {
                runtime.state = "OUTCOME_UNKNOWN";
                return rotator_command_result(
                    pairing,
                    Some(&endpoint),
                    generation,
                    command_id,
                    false,
                    "NATIVE_CONTROLLER_REJECTED",
                    Some(&prepared.id),
                    read_rotator_position(&endpoint).await,
                );
            }
            runtime.prepared = None;
            runtime.target = Some((prepared.azimuth, prepared.elevation));
            runtime.state = "MOVING";
            let position = read_rotator_position(&endpoint).await;
            let confirmed = position.is_some_and(|(azimuth, _)| {
                let delta = (azimuth - prepared.azimuth).abs().rem_euclid(360.0);
                delta.min(360.0 - delta) <= 2.0
            });
            if confirmed {
                runtime.target = None;
                runtime.state = "IDLE";
            }
            rotator_command_result(
                pairing,
                Some(&endpoint),
                generation,
                command_id,
                true,
                if confirmed {
                    "READBACK_CONFIRMED"
                } else {
                    "MOVE_ACCEPTED_POSITION_PENDING"
                },
                Some(&prepared.id),
                position,
            )
        }
        "rotator.stop" => {
            if !parameters.is_empty() {
                return rotator_command_result(
                    pairing,
                    Some(&endpoint),
                    generation,
                    command_id,
                    false,
                    "PARAMETERS_INVALID",
                    None,
                    None,
                );
            }
            #[cfg(feature = "radio")]
            let stopped = {
                let address = endpoint.address.clone();
                tauri::async_runtime::spawn_blocking(move || tempo_audio::rotator::stop(&address))
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .is_some()
            };
            #[cfg(not(feature = "radio"))]
            let stopped = false;
            runtime.prepared = None;
            runtime.target = None;
            runtime.state = if stopped { "IDLE" } else { "OUTCOME_UNKNOWN" };
            rotator_command_result(
                pairing,
                Some(&endpoint),
                generation,
                command_id,
                stopped,
                if stopped {
                    "STOP_CONFIRMED"
                } else {
                    "NATIVE_CONTROLLER_REJECTED"
                },
                None,
                read_rotator_position(&endpoint).await,
            )
        }
        _ => rotator_command_result(
            pairing,
            Some(&endpoint),
            generation,
            command_id,
            false,
            "ACTION_NOT_ADVERTISED",
            None,
            None,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    fn pairing() -> super::super::shackcq_connector::PairingRecord {
        super::super::shackcq_connector::PairingRecord {
            auth: super::super::shackcq_connector::ConnectorAuth {
                origin: "http://127.0.0.1:1".into(),
                credential_alias: "fixture".into(),
                station_profile_id: "station-1".into(),
                device_id: "device-1".into(),
            },
            agent_id: "agent-1".into(),
            account_label: "Fixture".into(),
            station_label: "Fixture station".into(),
            connect_url: "ws://127.0.0.1:1".into(),
        }
    }

    #[cfg(feature = "radio")]
    fn fake_rotctld(connections: usize) -> (RotatorEndpoint, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback rotctld");
        let address = listener.local_addr().expect("rotctld address").to_string();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut azimuth = 17.0;
            for _ in 0..connections {
                let (mut stream, _) = listener.accept().expect("fixture connection");
                let mut line = String::new();
                std::io::BufReader::new(stream.try_clone().expect("clone"))
                    .read_line(&mut line)
                    .expect("fixture command");
                tx.send(line.clone()).expect("capture command");
                if let Some(value) = line
                    .strip_prefix("P ")
                    .and_then(|row| row.split_whitespace().next())
                    .and_then(|value| value.parse::<f64>().ok())
                {
                    azimuth = value;
                    stream.write_all(b"RPRT 0\n").expect("point reply");
                } else if line == "p\n" {
                    stream
                        .write_all(format!("{azimuth:.1}\n0.0\n").as_bytes())
                        .expect("position reply");
                } else if line == "S\n" {
                    stream.write_all(b"RPRT 0\n").expect("stop reply");
                }
            }
        });
        (
            RotatorEndpoint {
                device_id: "agent-1:rotator".into(),
                address,
                model: "fixture".into(),
            },
            rx,
        )
    }

    fn rotator_command(
        endpoint: &RotatorEndpoint,
        command_id: &str,
        action: &str,
        parameters: Value,
    ) -> Value {
        json!({
            "type":"rotator.command",
            "protocol":{"major":1,"minor":ROTATOR_PROTOCOL_MINOR},
            "commandId":command_id,
            "agentId":"agent-1",
            "deviceId":endpoint.device_id,
            "expectedGeneration":7,
            "controlInstanceId":"fixture-browser",
            "validUntilUtc":(chrono::Utc::now() + chrono::Duration::seconds(10))
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "action":action,
            "parameters":parameters,
        })
    }

    fn radio_command(command_id: &str, valid_until: chrono::DateTime<chrono::Utc>) -> Value {
        json!({
            "type":"radio.command",
            "protocol":{"major":1,"minor":PROTOCOL_MINOR},
            "commandId":command_id,
            "agentId":"agent-1",
            "deviceId":"agent-1",
            "expectedGeneration":7,
            "validUntilUtc":valid_until.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "action":"radio.set.frequency",
            "parameters":{"frequencyHz":14_074_000_u64},
        })
    }

    #[test]
    fn advertised_controls_are_receive_safe() {
        let capabilities = radio_capabilities(2028, "Kenwood", "Kenwood TS-480");
        let setters = capabilities["setters"].as_array().unwrap();
        assert_eq!(setters.len(), 3);
        let text = serde_json::to_string(setters).unwrap();
        for prohibited in ["ptt", "tune", "power", "audio", "transmit"] {
            assert!(!text.to_ascii_lowercase().contains(prohibited));
        }
    }

    #[test]
    fn radio_snapshot_keeps_cat_readback_when_a_frequency_request_is_refused() {
        let pairing = pairing();
        let mut engine = tempo_app::engine::Engine::new("N0CALL", "AA00", 0);
        engine.seed_rig_dial(145_004_100);
        engine.observe_rig_mode("USB".into());
        engine.set_frequency(145.004_2, "2m", "USB");
        // A successful poll still reads the old dial: the requested tune did not take.
        engine.note_rig_freq_observation(145_004_100);
        let shared = std::sync::Arc::new(std::sync::Mutex::new(engine));
        let refused = radio_snapshot(&pairing, &shared, 7, 1);
        assert_eq!(refused["frequencyHz"], 145_004_100);
        assert_eq!(refused["mode"], "USB");

        engine_lock(&shared).note_rig_freq_observation(145_004_200);
        let accepted = radio_snapshot(&pairing, &shared, 7, 2);
        assert_eq!(accepted["frequencyHz"], 145_004_200);
    }

    #[tokio::test]
    async fn stale_radio_command_is_rejected_before_engine_mutation() {
        let pairing = pairing();
        let engine: SharedEngine = std::sync::Arc::new(std::sync::Mutex::new(
            tempo_app::engine::Engine::new("N0CALL", "AA00", 0),
        ));
        let before = engine_lock(&engine).snapshot().radio.dial_mhz;
        let command = radio_command(
            "stale-radio",
            chrono::Utc::now() - chrono::Duration::seconds(5),
        );

        let result = apply_radio_command(&pairing, &engine, 7, &command).await;

        assert_eq!(result["ok"], false);
        assert_eq!(result["code"], "STALE_COMMAND_REJECTED");
        assert_eq!(engine_lock(&engine).snapshot().radio.dial_mhz, before);
    }

    #[tokio::test]
    async fn stale_radio_generation_is_rejected_before_engine_mutation() {
        let pairing = pairing();
        let engine: SharedEngine = std::sync::Arc::new(std::sync::Mutex::new(
            tempo_app::engine::Engine::new("N0CALL", "AA00", 0),
        ));
        let before = engine_lock(&engine).snapshot().radio.dial_mhz;
        let command = radio_command(
            "stale-generation",
            chrono::Utc::now() + chrono::Duration::seconds(10),
        );

        let result = apply_radio_command(&pairing, &engine, 8, &command).await;

        assert_eq!(result["ok"], false);
        assert_eq!(result["code"], "STALE_GENERATION_REJECTED");
        assert_eq!(engine_lock(&engine).snapshot().radio.dial_mhz, before);
    }

    #[test]
    fn rotator_advertises_only_implemented_controller_capabilities() {
        let endpoint = RotatorEndpoint {
            device_id: "agent-1:rotator".into(),
            address: "127.0.0.1:4533".into(),
            model: "fixture".into(),
        };
        let capabilities = rotator_capabilities(&endpoint);
        assert_eq!(capabilities["azimuth"], true);
        assert_eq!(capabilities["stop"], true);
        assert_eq!(capabilities["elevation"], false);
        assert_eq!(capabilities["manualJog"], false);
        assert_eq!(capabilities["park"], false);

        let hello = rotator_hello(&endpoint);
        assert_eq!(hello["backend"], "Nexus rotctld");
        assert_eq!(hello["model"], "fixture");
    }

    #[cfg(feature = "radio")]
    #[tokio::test]
    async fn fake_rotator_requires_prepare_then_confirm_and_reads_back_independently() {
        let pairing = pairing();
        let (endpoint, received) = fake_rotctld(3);
        let mut runtime = RotatorRuntime {
            state: "IDLE",
            ..RotatorRuntime::default()
        };
        let prepare = rotator_command(
            &endpoint,
            "prep-1",
            "rotator.target.prepare",
            json!({"azimuth":123.4,"elevation":0.0}),
        );
        let prepared =
            apply_rotator_command(&pairing, Some(endpoint.clone()), 7, &mut runtime, &prepare)
                .await;
        assert_eq!(prepared["code"], "TARGET_PREPARED");
        assert_eq!(prepared["readback"]["azimuth"], 17.0);

        let confirm = rotator_command(
            &endpoint,
            "confirm-1",
            "rotator.move.confirm",
            json!({"preparationId":"prep-1"}),
        );
        let confirmed =
            apply_rotator_command(&pairing, Some(endpoint.clone()), 7, &mut runtime, &confirm)
                .await;
        assert_eq!(confirmed["ok"], true);
        assert_eq!(confirmed["code"], "READBACK_CONFIRMED");
        assert_eq!(confirmed["readback"]["azimuth"], 123.4);
        assert_eq!(received.recv().unwrap(), "p\n");
        assert_eq!(received.recv().unwrap(), "P 123.4 0.0\n");
        assert_eq!(received.recv().unwrap(), "p\n");
    }

    #[cfg(feature = "radio")]
    #[tokio::test]
    async fn fake_rotator_stop_uses_nexus_controller_and_reports_readback() {
        let pairing = pairing();
        let (endpoint, received) = fake_rotctld(2);
        let mut runtime = RotatorRuntime {
            state: "MOVING",
            target: Some((180.0, 0.0)),
            ..RotatorRuntime::default()
        };
        let stop = rotator_command(&endpoint, "stop-1", "rotator.stop", json!({}));
        let result = apply_rotator_command(&pairing, Some(endpoint), 7, &mut runtime, &stop).await;
        assert_eq!(result["ok"], true);
        assert_eq!(result["code"], "STOP_CONFIRMED");
        assert_eq!(received.recv().unwrap(), "S\n");
        assert_eq!(received.recv().unwrap(), "p\n");
    }

    #[tokio::test]
    async fn stale_rotator_command_is_rejected_before_controller_access() {
        let pairing = pairing();
        let endpoint = RotatorEndpoint {
            device_id: "agent-1:rotator".into(),
            address: "127.0.0.1:1".into(),
            model: "fixture".into(),
        };
        let mut command = rotator_command(&endpoint, "stale-1", "rotator.stop", json!({}));
        command["validUntilUtc"] = json!((chrono::Utc::now() - chrono::Duration::seconds(5))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        let mut runtime = RotatorRuntime::default();
        let result =
            apply_rotator_command(&pairing, Some(endpoint), 7, &mut runtime, &command).await;
        assert_eq!(result["ok"], false);
        assert_eq!(result["code"], "STALE_COMMAND_REJECTED");
    }
}
