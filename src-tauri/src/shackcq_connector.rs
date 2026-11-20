// SPDX-License-Identifier: GPL-3.0-only
//! Narrow ShackCQ website boundary. This module moves bounded station observations and
//! operator-approved completed contacts only; it has no audio, CAT, PTT, scheduler or rotator API.

use futures_util::StreamExt;
use reqwest::{redirect::Policy, Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::sync::Mutex;
use std::time::Duration;
use tauri::State;
use tempo_app::dto::ImportStats;
use tempo_app::engine::engine_lock;

use super::SharedEngine;

const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_LOGBOOK_BYTES: usize = 32 * 1024 * 1024;
const CONNECTOR_CREDENTIAL_SERVICE: &str = "app.shackcq.desktop.nexusreview.connector";
pub const SHACKCQ_DIGI_TX_ENABLED: bool = false;
static LOGBOOK_APPLY_LOCK: Mutex<()> = Mutex::new(());
static CONTACT_FLUSH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static WRITE_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[tauri::command]
pub fn shackcq_connector_capabilities() -> Value {
    serde_json::json!({
        "protocol":{"major":1,"minor":0},
        "scopes":["station.observe","contact.ingress","configuration.read","logbook.read"],
        "pcmOwner":"LOCAL_NEXUS",
        "radioOwner":"LOCAL_NEXUS",
        "rotatorOwner":"LOCAL_NEXUS",
        "productionDigiTxEnabled":SHACKCQ_DIGI_TX_ENABLED,
        "acceptance": super::acceptance::get().map(|i| serde_json::json!({
            "id": i.manifest.id, "root": i.root, "webOrigin": i.manifest.web_origin,
            "radio": i.manifest.radio_address, "rotor": i.manifest.rotor_address,
        })),
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectorAuth {
    pub(super) origin: String,
    pub(super) credential_alias: String,
    pub(super) station_profile_id: String,
    pub(super) device_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairingRequest {
    origin: String,
    code: String,
    name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairingRecord {
    pub(super) auth: ConnectorAuth,
    pub(super) agent_id: String,
    pub(super) account_label: String,
    pub(super) station_label: String,
    pub(super) connect_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PairingResponse {
    agent_id: String,
    credential: String,
    connect_url: String,
    protocol: Value,
    user_id: String,
    station_profile_id: String,
    account_label: String,
    station_label: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StationObservation {
    generation: String,
    observed_utc: String,
    callsign: String,
    grid: Option<String>,
    active_radio_id: String,
    frequency_hz: Option<u64>,
    mode: Option<String>,
    transmitting: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompletedContact {
    operation_identity: String,
    callsign: String,
    mode: String,
    submode: Option<String>,
    frequency_hz: Option<u64>,
    contact_start_utc: String,
    contact_end_utc: Option<String>,
    sent_report: Option<String>,
    received_report: Option<String>,
    grid: Option<String>,
    comment: String,
    user_authorized: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorReceipt {
    status: u16,
    body: Value,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectedLogbook {
    authority: String,
    connection_kind: String,
    integration_id: Option<String>,
    automatic_sync: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopConfiguration {
    protocol: Value,
    revision: u64,
    station_profile_id: String,
    logbook: SelectedLogbook,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DesktopLogbook {
    protocol: Value,
    authority_revision: u64,
    revision: u64,
    station_profile_id: String,
    authority: String,
    integration_id: Option<String>,
    complete: bool,
    records: Vec<DesktopLogbookRecord>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DesktopLogbookRecord {
    id: String,
    revision: u64,
    deleted: bool,
    operation_identity: Option<String>,
    adif: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLogbookSync {
    configuration: DesktopConfiguration,
    imported: ImportStats,
    remote_count: usize,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContactOutboxEvent {
    version: u8,
    #[serde(default = "committed_by_default")]
    committed: bool,
    agent_id: String,
    station_profile_id: String,
    operation_identity: String,
    local_contact_id: String,
    created_utc: String,
    authority: Option<DesktopConfiguration>,
    contact: CompletedContact,
}

fn committed_by_default() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DurableContactReceipt {
    operation_identity: String,
    qso_id: String,
    revision: u64,
    authority: String,
    authority_revision: u64,
    integration_id: Option<String>,
    station_profile_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AcceptedContactReceipt {
    accepted_by_shackcq: bool,
    receipt: DurableContactReceipt,
    provider_state: String,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactOutboxStatus {
    pending: usize,
    awaiting_destination: usize,
    delivered: usize,
    rejected: usize,
    last_error: Option<&'static str>,
}

fn identifier(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
}

fn callsign(value: &str) -> bool {
    (3..=32).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'/')
}

fn endpoint(auth: &ConnectorAuth, path: &str) -> Result<Url, &'static str> {
    if !super::acceptance::origin_allowed(&auth.origin) {
        return Err("fixtureOriginDenied");
    }
    if !identifier(&auth.station_profile_id)
        || !identifier(&auth.device_id)
        || !identifier(&auth.credential_alias)
    {
        return Err("invalidConnectorIdentity");
    }
    let mut origin = Url::parse(&auth.origin).map_err(|_| "originDenied")?;
    let loopback =
        origin.scheme() == "http" && matches!(origin.host_str(), Some("127.0.0.1" | "localhost"));
    if (origin.scheme() != "https" && !loopback)
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.query().is_some()
        || origin.fragment().is_some()
    {
        return Err("originDenied");
    }
    origin.set_path("/");
    origin.join(path).map_err(|_| "originDenied")
}

fn endpoint_for_origin(origin: &str, path: &str) -> Result<Url, &'static str> {
    endpoint(
        &ConnectorAuth {
            origin: origin.to_string(),
            credential_alias: "pairing".into(),
            station_profile_id: "pairing".into(),
            device_id: "pairing".into(),
        },
        path,
    )
}

fn pairing_path() -> std::path::PathBuf {
    super::config_dir().join("shackcq-connector.json")
}

fn selected_configuration_path() -> std::path::PathBuf {
    super::config_dir().join("shackcq-selected-configuration.json")
}

fn contact_outbox_dir() -> std::path::PathBuf {
    super::config_dir().join("shackcq-contact-outbox")
}

fn contact_receipts_dir() -> std::path::PathBuf {
    super::config_dir().join("shackcq-contact-receipts")
}

fn durable_write(path: &std::path::Path, value: &[u8]) -> Result<(), &'static str> {
    let parent = path.parent().ok_or("configurationUnavailable")?;
    fs::create_dir_all(parent).map_err(|_| "configurationUnavailable")?;
    let temporary = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        WRITE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|_| "configurationUnavailable")?;
    file.write_all(value)
        .and_then(|()| file.sync_all())
        .map_err(|_| "configurationUnavailable")?;
    fs::rename(&temporary, path).map_err(|_| "configurationUnavailable")?;
    OpenOptions::new()
        .read(true)
        .open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| "configurationUnavailable")
}

fn save_selected_configuration(configuration: &DesktopConfiguration) -> Result<(), &'static str> {
    let _guard = LOGBOOK_APPLY_LOCK
        .lock()
        .map_err(|_| "configurationUnavailable")?;
    if load_selected_configuration().is_some_and(|current| {
        current.station_profile_id == configuration.station_profile_id
            && current.revision > configuration.revision
    }) {
        return Err("configurationConflict");
    }
    let path = selected_configuration_path();
    fs::create_dir_all(path.parent().ok_or("configurationUnavailable")?)
        .map_err(|_| "configurationUnavailable")?;
    let temporary = path.with_extension("json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(configuration).map_err(|_| "configurationUnavailable")?,
    )
    .map_err(|_| "configurationUnavailable")?;
    fs::rename(temporary, path).map_err(|_| "configurationUnavailable")
}

fn load_selected_configuration() -> Option<DesktopConfiguration> {
    serde_json::from_slice(&fs::read(selected_configuration_path()).ok()?).ok()
}

pub(super) fn contact_outbox_enabled() -> bool {
    matches!(load_pairing(), Ok(Some(_)))
}

pub(super) fn capture_logged_contact(
    record: &tempo_core::logbook::QsoRecord,
) -> Result<bool, &'static str> {
    capture_contact_phase(record, true)
}

fn capture_contact_phase(
    record: &tempo_core::logbook::QsoRecord,
    committed: bool,
) -> Result<bool, &'static str> {
    let pairing = match load_pairing()? {
        Some(pairing) => pairing,
        None => return Ok(false),
    };
    let local_contact_id = record.id.ok_or("contactIdentityUnavailable")?.to_string();
    let operation_identity = format!("nexus-{local_contact_id}");
    if !identifier(&operation_identity) {
        return Err("contactIdentityInvalid");
    }
    let dir = contact_outbox_dir();
    fs::create_dir_all(&dir).map_err(|_| "configurationUnavailable")?;
    let path = dir.join(format!("{operation_identity}.json"));
    if path.exists() {
        if committed {
            let mut event: ContactOutboxEvent =
                serde_json::from_slice(&fs::read(&path).map_err(|_| "configurationUnavailable")?)
                    .map_err(|_| "configurationInvalid")?;
            if !event.committed {
                event.committed = true;
                durable_write(
                    &path,
                    &serde_json::to_vec_pretty(&event).map_err(|_| "configurationUnavailable")?,
                )?;
            }
        }
        return Ok(true);
    }
    if fs::read_dir(&dir)
        .map_err(|_| "configurationUnavailable")?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
        .count()
        >= 1_000
    {
        return Err("contactOutboxFull");
    }
    let started = chrono::DateTime::from_timestamp(record.when_unix as i64, 0)
        .ok_or("contactTimeInvalid")?
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let ended = record.time_off_unix.and_then(|value| {
        chrono::DateTime::from_timestamp(value as i64, 0)
            .map(|instant| instant.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
    });
    let event = ContactOutboxEvent {
        version: 1,
        committed,
        agent_id: pairing.agent_id,
        station_profile_id: pairing.auth.station_profile_id,
        operation_identity: operation_identity.clone(),
        local_contact_id,
        created_utc: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        authority: load_selected_configuration(),
        contact: CompletedContact {
            operation_identity,
            callsign: record.call.clone(),
            mode: record.mode.clone(),
            submode: None,
            frequency_hz: (record.freq_mhz.is_finite() && record.freq_mhz > 0.0)
                .then(|| (record.freq_mhz * 1_000_000.0).round() as u64),
            contact_start_utc: started,
            contact_end_utc: ended,
            sent_report: record.rst_sent.clone(),
            received_report: record.rst_rcvd.clone(),
            grid: record.grid.clone(),
            comment: record.comment.clone().unwrap_or_default(),
            user_authorized: true,
        },
    };
    durable_write(
        &path,
        &serde_json::to_vec_pretty(&event).map_err(|_| "configurationUnavailable")?,
    )?;
    Ok(true)
}

pub(super) fn observe_contact_commit(
    phase: tempo_app::engine::ContactCommitPhase,
    record: &tempo_core::logbook::QsoRecord,
) -> Result<(), &'static str> {
    if phase == tempo_app::engine::ContactCommitPhase::Committed {
        if let Some(isolation) = super::acceptance::get() {
            match isolation.take_contact_commit_interrupt() {
                Ok(true) => {
                    eprintln!("ShackCQ acceptance: interrupting after durable contact commit");
                    std::process::exit(86);
                }
                Ok(false) => {}
                Err(_) => return Err("fixtureInterruptUnavailable"),
            }
        }
    }
    let committed = phase == tempo_app::engine::ContactCommitPhase::Committed;
    capture_contact_phase(record, committed).map(|_| ())
}

fn save_pairing(record: &PairingRecord) -> Result<(), &'static str> {
    let _guard = LOGBOOK_APPLY_LOCK
        .lock()
        .map_err(|_| "configurationUnavailable")?;
    let path = pairing_path();
    fs::create_dir_all(path.parent().ok_or("configurationUnavailable")?)
        .map_err(|_| "configurationUnavailable")?;
    let bytes = serde_json::to_vec_pretty(record).map_err(|_| "configurationUnavailable")?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes).map_err(|_| "configurationUnavailable")?;
    fs::rename(temporary, path).map_err(|_| "configurationUnavailable")
}

pub(super) fn load_pairing() -> Result<Option<PairingRecord>, &'static str> {
    let path = pairing_path();
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("configurationUnavailable"),
    };
    let record: PairingRecord =
        serde_json::from_slice(&bytes).map_err(|_| "configurationInvalid")?;
    if !identifier(&record.agent_id)
        || !identifier(&record.auth.station_profile_id)
        || !identifier(&record.auth.credential_alias)
        || record.auth.device_id != record.agent_id
    {
        return Err("configurationInvalid");
    }
    endpoint(&record.auth, "api/v1/nexus-desktop/configuration")?;
    Ok(Some(record))
}

#[tauri::command]
pub fn shackcq_pairing_status() -> Result<Option<PairingRecord>, &'static str> {
    load_pairing()
}

#[tauri::command]
pub async fn shackcq_pair(request: PairingRequest) -> Result<PairingRecord, &'static str> {
    let code: String = request
        .code
        .chars()
        .filter(|value| value.is_ascii_alphanumeric())
        .map(|value| value.to_ascii_uppercase())
        .collect();
    if code.len() != 12 || request.name.trim().is_empty() || request.name.len() > 80 {
        return Err("pairingCodeInvalid");
    }
    let origin = endpoint_for_origin(&request.origin, "")?;
    let response = client()?
        .post(endpoint_for_origin(&request.origin, "api/v1/agent/pair")?)
        .json(&serde_json::json!({
            "code":code,
            "name":request.name.trim(),
            "platform":std::env::consts::OS,
            "version":env!("CARGO_PKG_VERSION"),
            "build":"Nexus v1.14.0 / ShackCQ review",
        }))
        .send()
        .await
        .map_err(|_| "serviceUnavailable")?;
    let status = response.status().as_u16();
    if status != 201 {
        return Err(match status {
            400 | 401 => "pairingCodeInvalid",
            429 => "tryLater",
            _ => "serviceUnavailable",
        });
    }
    let bytes = response.bytes().await.map_err(|_| "serviceUnavailable")?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err("invalidResponse");
    }
    let paired: PairingResponse = serde_json::from_slice(&bytes).map_err(|_| "invalidResponse")?;
    if paired.protocol != serde_json::json!({"major":1,"minor":3})
        || !identifier(&paired.agent_id)
        || !identifier(&paired.user_id)
        || !identifier(&paired.station_profile_id)
        || !(32..=256).contains(&paired.credential.len())
        || paired
            .credential
            .bytes()
            .any(|byte| byte.is_ascii_whitespace())
    {
        return Err("invalidResponse");
    }
    let connect = Url::parse(&paired.connect_url).map_err(|_| "invalidResponse")?;
    let expected_scheme = if origin.scheme() == "https" {
        "wss"
    } else {
        "ws"
    };
    if connect.scheme() != expected_scheme
        || connect.host_str() != origin.host_str()
        || connect.port_or_known_default() != origin.port_or_known_default()
        || connect.path() != "/api/v1/agent/connect"
        || connect.query().is_some()
        || connect.fragment().is_some()
        || !connect.username().is_empty()
        || connect.password().is_some()
    {
        return Err("invalidResponse");
    }
    let alias = format!("agent-{}", paired.agent_id);
    let entry = keyring::Entry::new(
        &super::acceptance::credential_service(CONNECTOR_CREDENTIAL_SERVICE),
        &alias,
    )
    .map_err(|_| "credentialStoreUnavailable")?;
    entry
        .set_password(&paired.credential)
        .map_err(|_| "credentialStoreUnavailable")?;
    let record = PairingRecord {
        auth: ConnectorAuth {
            origin: request.origin.trim_end_matches('/').to_string(),
            credential_alias: alias,
            station_profile_id: paired.station_profile_id,
            device_id: paired.agent_id.clone(),
        },
        agent_id: paired.agent_id,
        account_label: paired.account_label,
        station_label: paired.station_label,
        connect_url: paired.connect_url,
    };
    if let Err(error) = save_pairing(&record) {
        let _ = entry.delete_credential();
        return Err(error);
    }
    Ok(record)
}

#[tauri::command]
pub fn shackcq_forget_pairing() -> Result<(), &'static str> {
    let _guard = LOGBOOK_APPLY_LOCK
        .lock()
        .map_err(|_| "configurationUnavailable")?;
    if let Some(record) = load_pairing()? {
        keyring::Entry::new(
            &super::acceptance::credential_service(CONNECTOR_CREDENTIAL_SERVICE),
            &record.auth.credential_alias,
        )
        .map_err(|_| "credentialStoreUnavailable")?
        .delete_credential()
        .map_err(|_| "credentialStoreUnavailable")?;
    }
    match fs::remove_file(pairing_path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("configurationUnavailable"),
    }
}

pub(super) fn machine_token(alias: &str) -> Result<String, &'static str> {
    if !identifier(alias) {
        return Err("invalidConnectorIdentity");
    }
    let token = keyring::Entry::new(
        &super::acceptance::credential_service(CONNECTOR_CREDENTIAL_SERVICE),
        alias,
    )
    .map_err(|_| "credentialStoreUnavailable")?
    .get_password()
    .map_err(|_| "connectorNotPaired")?;
    if !(32..=4096).contains(&token.len()) || token.bytes().any(|b| b.is_ascii_whitespace()) {
        return Err("invalidConnectorCredential");
    }
    Ok(token)
}

fn extend_bounded_response(bytes: &mut Vec<u8>, chunk: &[u8]) -> Result<(), &'static str> {
    if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
        return Err("invalidResponse");
    }
    bytes.extend_from_slice(chunk);
    Ok(())
}

fn client() -> Result<Client, &'static str> {
    Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| "serviceUnavailable")
}

fn validate_configuration(
    auth: &ConnectorAuth,
    configuration: &DesktopConfiguration,
) -> Result<(), &'static str> {
    if configuration.protocol != serde_json::json!({"major":1,"minor":0})
        || configuration.station_profile_id != auth.station_profile_id
        || !configuration.logbook.automatic_sync
    {
        return Err("invalidConfiguration");
    }
    match (
        configuration.logbook.authority.as_str(),
        configuration.logbook.connection_kind.as_str(),
        configuration.logbook.integration_id.as_deref(),
    ) {
        ("WEB_LOCAL", "LOCAL", None) => Ok(()),
        ("WAVELOG", "EXTERNAL" | "MANAGED", Some(id)) if identifier(id) => Ok(()),
        _ => Err("invalidConfiguration"),
    }
}

#[tauri::command]
pub async fn shackcq_fetch_configuration(
    auth: ConnectorAuth,
) -> Result<DesktopConfiguration, &'static str> {
    let token = machine_token(&auth.credential_alias)?;
    let response = client()?
        .get(endpoint(&auth, "api/v1/nexus-desktop/configuration")?)
        .bearer_auth(token)
        .header("x-shackcq-desktop-protocol", "1.0")
        .send()
        .await
        .map_err(|_| "serviceUnavailable")?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(match status {
            401 | 403 => "accessDenied",
            409 => "configurationConflict",
            429 => "tryLater",
            _ => "serviceUnavailable",
        });
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err("invalidResponse");
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "serviceUnavailable")?;
        extend_bounded_response(&mut bytes, &chunk)?;
    }
    let configuration: DesktopConfiguration =
        serde_json::from_slice(&bytes).map_err(|_| "invalidConfiguration")?;
    validate_configuration(&auth, &configuration)?;
    save_selected_configuration(&configuration)?;
    Ok(configuration)
}

fn outbox_events(
) -> Result<Vec<(std::path::PathBuf, Result<ContactOutboxEvent, &'static str>)>, &'static str> {
    read_outbox_events(&contact_outbox_dir())
}

fn read_outbox_events(
    dir: &std::path::Path,
) -> Result<Vec<(std::path::PathBuf, Result<ContactOutboxEvent, &'static str>)>, &'static str> {
    let mut paths = match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|part| part.to_str()) == Some("json"))
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("configurationUnavailable"),
    };
    paths.sort();
    Ok(paths
        .into_iter()
        .map(|path| {
            let event = fs::read(&path)
                .map_err(|_| "configurationUnavailable")
                .and_then(|bytes| {
                    serde_json::from_slice(&bytes).map_err(|_| "configurationInvalid")
                });
            (path, event)
        })
        .collect())
}

#[derive(Default, Deserialize, Serialize)]
struct ContactAttempt {
    attempts: u32,
    error: String,
    terminal: bool,
    retry_after: i64,
}

fn retryable_contact_error(code: &str) -> bool {
    matches!(
        code,
        "serviceUnavailable"
            | "tryLater"
            | "authorityChanged"
            | "configurationConflict"
            | "configurationUnavailable"
    )
}

fn record_contact_failure(
    path: &std::path::Path,
    code: &str,
    now: i64,
) -> Result<ContactAttempt, &'static str> {
    let prior: ContactAttempt = fs::read(path.with_extension("attempt"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let attempts = prior.attempts.saturating_add(1);
    let attempt = ContactAttempt {
        attempts,
        error: code.into(),
        terminal: !retryable_contact_error(code),
        retry_after: now.saturating_add(30 * (1i64 << attempts.min(5))),
    };
    durable_write(
        &path.with_extension("attempt"),
        &serde_json::to_vec(&attempt).map_err(|_| "configurationUnavailable")?,
    )?;
    Ok(attempt)
}

fn same_destination(original: &DesktopConfiguration, selected: &DesktopConfiguration) -> bool {
    original.station_profile_id == selected.station_profile_id
        && original.revision <= selected.revision
        && original.logbook.authority == selected.logbook.authority
        && original.logbook.integration_id == selected.logbook.integration_id
}

fn same_pairing(a: &PairingRecord, b: &PairingRecord) -> bool {
    a.agent_id == b.agent_id
        && a.auth.origin == b.auth.origin
        && a.auth.credential_alias == b.auth.credential_alias
        && a.auth.station_profile_id == b.auth.station_profile_id
        && a.auth.device_id == b.auth.device_id
}

/// An operator may retry an access refusal after service access is restored. The
/// immutable event, destination revision and operation identity are never rewritten.
/// Keep the failure history and synchronize the rearm before the ordinary send path.
fn rearm_contact_attempt(
    path: &std::path::Path,
    original: &DesktopConfiguration,
    selected: &DesktopConfiguration,
) -> Result<(), &'static str> {
    if !same_destination(original, selected) || original.revision != selected.revision {
        return Err("outboxDestinationChanged");
    }
    let mut attempt: ContactAttempt = serde_json::from_slice(
        &fs::read(path.with_extension("attempt")).map_err(|_| "configurationUnavailable")?,
    )
    .map_err(|_| "invalidAttemptJournal")?;
    if !attempt.terminal || attempt.error != "accessDenied" {
        return Err("contactRetryNotAllowed");
    }
    attempt.terminal = false;
    attempt.retry_after = 0;
    durable_write(
        &path.with_extension("attempt"),
        &serde_json::to_vec(&attempt).map_err(|_| "configurationUnavailable")?,
    )
}

fn validate_contact_receipt(
    response: &ConnectorReceipt,
    event: &ContactOutboxEvent,
    configuration: &DesktopConfiguration,
) -> Result<AcceptedContactReceipt, &'static str> {
    if response.status != 202 {
        return Err("durableReceiptMissing");
    }
    let accepted: AcceptedContactReceipt =
        serde_json::from_value(response.body.clone()).map_err(|_| "durableReceiptMissing")?;
    let receipt = &accepted.receipt;
    if !accepted.accepted_by_shackcq
        || receipt.operation_identity != event.operation_identity
        || receipt.station_profile_id != event.station_profile_id
        || receipt.authority != configuration.logbook.authority
        || receipt.authority_revision != configuration.revision
        || receipt.integration_id != configuration.logbook.integration_id
        || !identifier(&receipt.qso_id)
        || receipt.revision == 0
        || !matches!(
            accepted.provider_state.as_str(),
            "NOT_APPLICABLE" | "QUEUED"
        )
    {
        return Err("durableReceiptInvalid");
    }
    Ok(accepted)
}

fn stored_contact_receipt(
    event: &ContactOutboxEvent,
    configuration: &DesktopConfiguration,
) -> Result<Option<AcceptedContactReceipt>, &'static str> {
    let path = contact_receipts_dir().join(format!("{}.json", event.operation_identity));
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("configurationUnavailable"),
    };
    let accepted: AcceptedContactReceipt =
        serde_json::from_slice(&bytes).map_err(|_| "durableReceiptInvalid")?;
    let synthetic = ConnectorReceipt {
        status: 202,
        body: serde_json::to_value(&accepted).map_err(|_| "durableReceiptInvalid")?,
    };
    validate_contact_receipt(&synthetic, event, configuration).map(Some)
}

fn save_contact_receipt(
    event: &ContactOutboxEvent,
    accepted: &AcceptedContactReceipt,
) -> Result<(), &'static str> {
    let path = contact_receipts_dir().join(format!("{}.json", event.operation_identity));
    durable_write(
        &path,
        &serde_json::to_vec_pretty(accepted).map_err(|_| "configurationUnavailable")?,
    )
}

#[tauri::command]
pub async fn shackcq_flush_contact_outbox(
    state: State<'_, SharedEngine>,
) -> Result<ContactOutboxStatus, &'static str> {
    flush_contact_outbox(state, false).await
}

#[tauri::command]
pub async fn shackcq_retry_rejected_contacts(
    state: State<'_, SharedEngine>,
) -> Result<ContactOutboxStatus, &'static str> {
    flush_contact_outbox(state, true).await
}

async fn flush_contact_outbox(
    state: State<'_, SharedEngine>,
    retry_rejected: bool,
) -> Result<ContactOutboxStatus, &'static str> {
    let _flush = CONTACT_FLUSH_LOCK.lock().await;
    let pairing = load_pairing()?.ok_or("connectorNotPaired")?;
    let selected = shackcq_fetch_configuration(pairing.auth.clone()).await?;
    let mut status = ContactOutboxStatus::default();
    let mut attempts = 0;
    for (path, event) in outbox_events()? {
        status.pending += 1;
        let now = chrono::Utc::now().timestamp();
        let mut terminal_retry = false;
        match fs::read(path.with_extension("attempt")) {
            Ok(bytes) => match serde_json::from_slice::<ContactAttempt>(&bytes) {
                Ok(attempt)
                    if retry_rejected && attempt.terminal && attempt.error == "accessDenied" =>
                {
                    terminal_retry = true;
                }
                Ok(attempt) if attempt.terminal => {
                    status.rejected += 1;
                    continue;
                }
                Ok(attempt) if attempt.retry_after > now => continue,
                Ok(_) => {}
                Err(_) => {
                    status.rejected += 1;
                    status.last_error = Some("invalidAttemptJournal");
                    continue;
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                status.last_error = Some("configurationUnavailable");
                continue;
            }
        }
        let event = match event {
            Ok(event) => event,
            Err(code) => {
                record_contact_failure(&path, code, now)?;
                status.rejected += 1;
                status.last_error = Some(code);
                continue;
            }
        };
        if event.version != 1
            || !identifier(&event.operation_identity)
            || !identifier(&event.local_contact_id)
            || event.operation_identity != event.contact.operation_identity
        {
            record_contact_failure(&path, "configurationInvalid", now)?;
            status.rejected += 1;
            status.last_error = Some("configurationInvalid");
            continue;
        }
        if event.agent_id != pairing.agent_id
            || event.station_profile_id != pairing.auth.station_profile_id
        {
            status.last_error = Some("outboxScopeMismatch");
            status.awaiting_destination += 1;
            continue;
        }
        let Some(configuration) = event.authority.as_ref() else {
            status.awaiting_destination += 1;
            continue;
        };
        let stored = match stored_contact_receipt(&event, configuration) {
            Ok(stored) => stored,
            Err(code) => {
                record_contact_failure(&path, code, now)?;
                status.rejected += 1;
                status.last_error = Some(code);
                continue;
            }
        };
        if stored.is_some() {
            fs::remove_file(path).map_err(|_| "configurationUnavailable")?;
            status.delivered += 1;
            status.pending -= 1;
            continue;
        }
        if !same_destination(configuration, &selected)
            || (terminal_retry && configuration.revision != selected.revision)
        {
            status.awaiting_destination += 1;
            if terminal_retry {
                status.rejected += 1;
                status.last_error = Some("outboxDestinationChanged");
            }
            continue;
        }
        // Memory is populated before append. Only the actual file, synchronized here
        // outside the Engine lock, can recover the prepare/append crash window.
        {
            let log_path = engine_lock(&state)
                .log_path()
                .map(std::path::Path::to_path_buf);
            let committed = log_path.as_deref().is_some_and(|path| {
                durable_contact_on_disk(path, &event.local_contact_id).unwrap_or(false)
                    || durable_archived_contact(&replica_archive_path(), &event.local_contact_id)
                        .unwrap_or(false)
            });
            if !committed {
                status.last_error = Some("localContactNotDurable");
                continue;
            }
        }
        if attempts >= 32 {
            continue;
        }
        if terminal_retry {
            // Share the pairing/selection lock with configuration transitions. A
            // late configuration read cannot revive a different pairing's work.
            let _guard = LOGBOOK_APPLY_LOCK
                .lock()
                .map_err(|_| "configurationUnavailable")?;
            let current_pairing = load_pairing()?.ok_or("connectorNotPaired")?;
            let current_selection = load_selected_configuration().ok_or("configurationConflict")?;
            if !same_pairing(&pairing, &current_pairing)
                || current_selection.revision != selected.revision
            {
                return Err("configurationConflict");
            }
            rearm_contact_attempt(&path, configuration, &current_selection)?;
        }
        attempts += 1;
        let operation = event.operation_identity.clone();
        let contact = &event.contact;
        let body = serde_json::json!({
            "protocol":{"major":1,"minor":0},
            "stationProfileId":pairing.auth.station_profile_id,
            "radioDeviceId":pairing.auth.device_id,
            "operationIdentity":operation,
            "authorityRevision":configuration.revision,
            "authority":&configuration.logbook.authority,
            "integrationId":&configuration.logbook.integration_id,
            "completed":true,
            "userAuthorized":true,
            "qso":{
                "callsign":contact.callsign,
                "mode":contact.mode,
                "submode":contact.submode,
                "frequencyHz":contact.frequency_hz,
                "contactStartUtc":contact.contact_start_utc,
                "contactEndUtc":contact.contact_end_utc,
                "sentReport":contact.sent_report,
                "receivedReport":contact.received_report,
                "grid":contact.grid,
                "comment":contact.comment,
            },
        });
        match send(
            &pairing.auth,
            "api/v1/nexus-desktop/contact-events",
            &event.operation_identity,
            body,
        )
        .await
        {
            Ok(response) => {
                let accepted = match validate_contact_receipt(&response, &event, configuration) {
                    Ok(accepted) => accepted,
                    Err(code) => {
                        record_contact_failure(&path, code, now)?;
                        status.rejected += 1;
                        status.last_error = Some(code);
                        continue;
                    }
                };
                // A late response must not retire an event after a pairing or source
                // transition. Keep the immutable event; canonical replay is idempotent.
                let latest = shackcq_fetch_configuration(pairing.auth.clone()).await?;
                let _guard = LOGBOOK_APPLY_LOCK
                    .lock()
                    .map_err(|_| "configurationUnavailable")?;
                let current_pairing = load_pairing()?.ok_or("connectorNotPaired")?;
                let current_selection =
                    load_selected_configuration().ok_or("configurationConflict")?;
                if !same_pairing(&pairing, &current_pairing)
                    || latest.revision != selected.revision
                    || current_selection.revision != latest.revision
                    || !same_destination(configuration, &latest)
                    || !same_destination(configuration, &current_selection)
                {
                    status.awaiting_destination += 1;
                    status.last_error = Some("configurationConflict");
                    continue;
                }
                save_contact_receipt(&event, &accepted)?;
                fs::remove_file(path).map_err(|_| "configurationUnavailable")?;
                status.delivered += 1;
                status.pending -= 1;
            }
            Err(code) => {
                status.last_error = Some(code);
                if record_contact_failure(&path, code, now)?.terminal {
                    status.rejected += 1;
                }
            }
        }
    }
    Ok(status)
}

#[tauri::command]
pub fn shackcq_contact_outbox_status() -> Result<ContactOutboxStatus, &'static str> {
    let events = outbox_events()?;
    Ok(ContactOutboxStatus {
        pending: events.len(),
        awaiting_destination: events
            .iter()
            .filter(|(_, event)| event.as_ref().is_ok_and(|event| event.authority.is_none()))
            .count(),
        ..ContactOutboxStatus::default()
    })
}

#[tauri::command]
pub async fn shackcq_sync_selected_logbook(
    auth: ConnectorAuth,
    state: State<'_, SharedEngine>,
) -> Result<DesktopLogbookSync, &'static str> {
    let configuration = shackcq_fetch_configuration(auth.clone()).await?;
    let token = machine_token(&auth.credential_alias)?;
    let response = client()?
        .get(endpoint(&auth, "api/v1/nexus-desktop/logbook")?)
        .bearer_auth(token)
        .header("x-shackcq-desktop-protocol", "1.0")
        .send()
        .await
        .map_err(|_| "serviceUnavailable")?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(match status {
            401 | 403 => "accessDenied",
            409 => "configurationConflict",
            413 => "logbookTooLarge",
            429 => "tryLater",
            _ => "serviceUnavailable",
        });
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_LOGBOOK_BYTES as u64)
    {
        return Err("logbookTooLarge");
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "serviceUnavailable")?;
        if bytes.len().saturating_add(chunk.len()) > MAX_LOGBOOK_BYTES {
            return Err("logbookTooLarge");
        }
        bytes.extend_from_slice(&chunk);
    }
    let logbook: DesktopLogbook = serde_json::from_slice(&bytes).map_err(|_| "invalidLogbook")?;
    if logbook.protocol != serde_json::json!({"major":1,"minor":0})
        || logbook.station_profile_id != auth.station_profile_id
        || logbook.authority_revision != configuration.revision
        || logbook.authority != configuration.logbook.authority
        || logbook.integration_id != configuration.logbook.integration_id
        || !logbook.complete
    {
        return Err("configurationConflict");
    }
    // A source change while the full response was in flight retires that response,
    // even when no other local poll happened to observe the new configuration.
    let latest = shackcq_fetch_configuration(auth.clone()).await?;
    if latest.revision != configuration.revision {
        return Err("configurationConflict");
    }
    let _guard = LOGBOOK_APPLY_LOCK
        .lock()
        .map_err(|_| "configurationUnavailable")?;
    let current = load_selected_configuration().ok_or("configurationConflict")?;
    let pairing = load_pairing()?.ok_or("connectorNotPaired")?;
    if pairing.auth.origin != auth.origin
        || pairing.auth.credential_alias != auth.credential_alias
        || pairing.auth.device_id != auth.device_id
        || current.station_profile_id != configuration.station_profile_id
        || current.revision != configuration.revision
        || current.logbook.authority != configuration.logbook.authority
        || current.logbook.integration_id != configuration.logbook.integration_id
    {
        return Err("configurationConflict");
    }
    let partition = serde_json::to_string(&(
        &auth.origin,
        &auth.device_id,
        &auth.station_profile_id,
        &auth.credential_alias,
        &logbook.authority,
        &logbook.integration_id,
    ))
    .map_err(|_| "invalidLogbook")?;
    let cache_path = super::config_dir().join("shackcq-logbook-partitions.json");
    let mut cache: std::collections::BTreeMap<String, DesktopLogbook> = match fs::read(&cache_path)
    {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "invalidLogbookCache")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Default::default(),
        Err(_) => return Err("configurationUnavailable"),
    };
    validate_snapshot_revision(&logbook, cache.get(&partition))?;
    let mut engine = engine_lock(&state);
    engine.sync_shared_log_if_changed();
    // Write-ahead recovery archive: projection never destroys the only copy of an
    // ambiguous contact or an identified row belonging to an inactive authority.
    archive_replica_records(&replica_archive_path(), engine.log_records())?;
    let (records, added, updated) = project_logbook(engine.log_records(), &logbook, &partition)?;
    let total = records.len();
    let remote_count = logbook.records.iter().filter(|row| !row.deleted).count();
    // Persist the full partition first. If projection persistence fails, the same
    // revision can be safely reapplied; older responses remain fenced after restart.
    cache.insert(partition, logbook);
    durable_write(
        &cache_path,
        &serde_json::to_vec(&cache).map_err(|_| "invalidLogbookCache")?,
    )?;
    if engine.log_records().len() != records.len()
        || engine
            .log_records()
            .iter()
            .zip(&records)
            .any(|(old, new)| **old != *new)
    {
        engine
            .replace_logbook_projection(records)
            .map_err(|_| "logbookSaveFailed")?;
    }
    Ok(DesktopLogbookSync {
        configuration,
        imported: ImportStats {
            added,
            skipped: 0,
            updated,
            total,
        },
        remote_count,
    })
}

fn durable_contact_on_disk(path: &std::path::Path, local_id: &str) -> Result<bool, &'static str> {
    let mut file = fs::File::open(path).map_err(|_| "localContactNotDurable")?;
    file.sync_all().map_err(|_| "localContactNotDurable")?;
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|_| "localContactNotDurable")?;
    let mut log = tempo_core::logbook::Logbook::new();
    log.import_adif(&text);
    Ok(log
        .records()
        .iter()
        .any(|record| record.id.is_some_and(|id| id.to_string() == local_id)))
}

fn validate_snapshot_revision(
    next: &DesktopLogbook,
    previous: Option<&DesktopLogbook>,
) -> Result<(), &'static str> {
    if let Some(previous) = previous {
        if next.authority_revision < previous.authority_revision
            || next.authority_revision == previous.authority_revision
                && next.revision < previous.revision
        {
            return Err("obsoleteLogbookSnapshot");
        }
    }
    Ok(())
}

/// Complete snapshots replace only connector-owned rows. Stable remote identity,
/// never callsign/time heuristics, selects an update. Imports never emit contacts.
fn project_logbook(
    existing: &[std::sync::Arc<tempo_core::logbook::QsoRecord>],
    snapshot: &DesktopLogbook,
    partition: &str,
) -> Result<(Vec<tempo_core::logbook::QsoRecord>, usize, usize), &'static str> {
    use tempo_core::logbook::Logbook;
    const SOURCE: &str = "APP_SHACKCQ_PARTITION";
    const ID: &str = "APP_SHACKCQ_QSO_ID";
    let field = |record: &tempo_core::logbook::QsoRecord, key: &str| {
        record
            .extra
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    };
    if !snapshot.complete
        || snapshot.records.len() > 100_000
        || snapshot
            .records
            .iter()
            .try_fold(0u64, |sum, row| sum.checked_add(row.revision))
            != Some(snapshot.revision)
    {
        return Err("invalidLogbook");
    }
    let mut seen = std::collections::HashSet::new();
    let by_local_id: std::collections::HashMap<_, _> = existing
        .iter()
        .filter_map(|row| row.id.map(|id| (id.to_string(), row)))
        .collect();
    let by_remote_id: std::collections::HashMap<_, _> = existing
        .iter()
        .filter(|row| field(row, SOURCE).as_deref() == Some(partition))
        .filter_map(|row| field(row, ID).map(|id| (id, row)))
        .collect();
    let mut projected = Vec::new();
    let mut added = 0;
    let mut updated = 0;
    for row in &snapshot.records {
        if !identifier(&row.id)
            || row.revision == 0
            || !seen.insert(&row.id)
            || row
                .operation_identity
                .as_deref()
                .is_some_and(|id| !identifier(id))
            || row.deleted != row.adif.is_none()
        {
            return Err("invalidLogbook");
        }
        let echo = row
            .operation_identity
            .as_deref()
            .and_then(|id| id.strip_prefix("nexus-"));
        if row.deleted {
            continue;
        }
        let mut parsed = Logbook::new();
        let (records, skipped, _) =
            parsed.import_adif(row.adif.as_deref().ok_or("invalidLogbook")?);
        if records.len() != 1 || skipped != 0 {
            return Err("invalidLogbook");
        }
        let mut record = records[0].clone();
        let previous = by_remote_id
            .get(&row.id)
            .copied()
            .or_else(|| echo.and_then(|id| by_local_id.get(id).copied()));
        record.id = previous.and_then(|old| old.id);
        record.extra.retain(|(key, _)| key != SOURCE && key != ID);
        record.extra.push((SOURCE.into(), partition.into()));
        record.extra.push((ID.into(), row.id.clone()));
        if let Some(old) = previous {
            if **old != record {
                updated += 1;
            }
        } else {
            added += 1;
        }
        projected.push(record);
    }
    // Only the selected authority contributes to history, worked indices and awards.
    // Unassigned rows are preserved by the write-ahead archive, never guessed into
    // a provider partition. Exact operation echoes above retain their stable ID.
    Ok((projected, added, updated))
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplicaArchive {
    // ADIF records retain original fields and stable IDs. Sets make replay after
    // an archive-write/projection-write crash idempotent without heuristic merging.
    unassigned: std::collections::BTreeSet<String>,
    identified: std::collections::BTreeSet<String>,
}

fn replica_archive_path() -> std::path::PathBuf {
    super::shared_data_dir().join("shackcq-replica-recovery.json")
}

fn read_replica_archive(path: &std::path::Path) -> Result<ReplicaArchive, &'static str> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "invalidReplicaArchive"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(_) => Err("replicaArchiveUnavailable"),
    }
}

fn durable_archived_contact(path: &std::path::Path, id: &str) -> Result<bool, &'static str> {
    fs::File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| "replicaArchiveUnavailable")?;
    let archive = read_replica_archive(path)?;
    let mut log = tempo_core::logbook::Logbook::new();
    for row in archive.unassigned.iter().chain(&archive.identified) {
        log.import_adif(row);
    }
    Ok(log
        .records()
        .iter()
        .any(|r| r.id.is_some_and(|value| value.to_string() == id)))
}

fn archive_replica_records(
    path: &std::path::Path,
    records: &[std::sync::Arc<tempo_core::logbook::QsoRecord>],
) -> Result<(), &'static str> {
    let mut archive = read_replica_archive(path)?;
    for record in records {
        let target = if record
            .extra
            .iter()
            .any(|(k, _)| k == "APP_SHACKCQ_PARTITION")
        {
            &mut archive.identified
        } else {
            &mut archive.unassigned
        };
        target.insert(tempo_core::logbook::adif_record(record));
    }
    durable_write(
        path,
        &serde_json::to_vec(&archive).map_err(|_| "invalidReplicaArchive")?,
    )
}

/// Called before publishing the startup engine. A previously selected authority
/// must never expose legacy records while waiting for its first online refresh.
pub(super) fn recover_legacy_partition(
    engine: &mut tempo_app::engine::Engine,
) -> Result<(), &'static str> {
    if !selected_configuration_path().exists() {
        return Ok(());
    }
    let selected = load_selected_configuration().ok_or("invalidSelectedConfiguration")?;
    // Forgetting a credential must not make offline startup impossible, nor
    // expose the previously selected history to a newly paired account.
    let partition = load_pairing()?
        .map(|pairing| {
            serde_json::to_string(&(
                &pairing.auth.origin,
                &pairing.auth.device_id,
                &pairing.auth.station_profile_id,
                &pairing.auth.credential_alias,
                &selected.logbook.authority,
                &selected.logbook.integration_id,
            ))
        })
        .transpose()
        .map_err(|_| "invalidSelectedConfiguration")?;
    recover_partition_records(engine, &replica_archive_path(), partition.as_deref())
}

fn recover_partition_records(
    engine: &mut tempo_app::engine::Engine,
    archive_path: &std::path::Path,
    partition: Option<&str>,
) -> Result<(), &'static str> {
    archive_replica_records(archive_path, engine.log_records())?;
    let records = engine
        .log_records()
        .iter()
        .filter(|r| {
            partition.is_some_and(|partition| {
                r.extra
                    .iter()
                    .any(|(k, v)| k == "APP_SHACKCQ_PARTITION" && v == partition)
            })
        })
        .map(|r| (**r).clone())
        .collect();
    engine
        .replace_logbook_projection(records)
        .map_err(|_| "logbookSaveFailed")
}

#[tauri::command]
pub fn shackcq_export_unassigned_contacts() -> Result<String, &'static str> {
    let _guard = LOGBOOK_APPLY_LOCK
        .lock()
        .map_err(|_| "configurationUnavailable")?;
    let archive = read_replica_archive(&replica_archive_path())?;
    Ok(tempo_core::logbook::adif_header() + &archive.unassigned.into_iter().collect::<String>())
}

async fn send(
    auth: &ConnectorAuth,
    path: &str,
    operation: &str,
    body: Value,
) -> Result<ConnectorReceipt, &'static str> {
    // The browser-facing command receives only an opaque keychain alias. Raw
    // bearer material never crosses the Tauri IPC boundary.
    let token = machine_token(&auth.credential_alias)?;
    let response = client()?
        .post(endpoint(auth, path)?)
        .bearer_auth(token)
        .header("idempotency-key", operation)
        .header("x-shackcq-desktop-protocol", "1.0")
        .json(&body)
        .send()
        .await
        .map_err(|_| "serviceUnavailable")?;
    let status = response.status().as_u16();
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err("invalidResponse");
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "serviceUnavailable")?;
        extend_bounded_response(&mut bytes, &chunk)?;
    }
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Err(match status {
            401 | 403 => "accessDenied",
            409 if body.get("code").and_then(Value::as_str)
                == Some("DESKTOP_AUTHORITY_OBSOLETE") =>
            {
                "authorityChanged"
            }
            409 => "operationConflict",
            429 => "tryLater",
            400..=499 => "contactRejected",
            _ => "serviceUnavailable",
        });
    }
    Ok(ConnectorReceipt { status, body })
}

#[tauri::command]
pub async fn shackcq_publish_station_state(
    auth: ConnectorAuth,
    observation: StationObservation,
) -> Result<ConnectorReceipt, &'static str> {
    if !identifier(&observation.generation)
        || !identifier(&observation.active_radio_id)
        || !callsign(&observation.callsign)
    {
        return Err("invalidObservation");
    }
    let operation = format!("state:{}:{}", auth.device_id, observation.generation);
    let body = serde_json::json!({
        "protocol":{"major":1,"minor":0},
        "stationProfileId":&auth.station_profile_id,
        "deviceId":&auth.device_id,
        "observation":observation,
    });
    send(
        &auth,
        "api/v1/nexus-desktop/station-state",
        &operation,
        body,
    )
    .await
}

#[tauri::command]
pub async fn shackcq_sync_completed_contact(
    auth: ConnectorAuth,
    contact: CompletedContact,
) -> Result<ConnectorReceipt, &'static str> {
    if !contact.user_authorized
        || !identifier(&contact.operation_identity)
        || !callsign(&contact.callsign)
        || contact.comment.len() > 512
    {
        return Err("invalidContact");
    }
    let operation = contact.operation_identity.clone();
    let configuration = shackcq_fetch_configuration(auth.clone()).await?;
    let body = serde_json::json!({
        "protocol":{"major":1,"minor":0},
        "stationProfileId":&auth.station_profile_id,
        "radioDeviceId":&auth.device_id,
        "operationIdentity":operation.clone(),
        "authorityRevision":configuration.revision,
        "authority":configuration.logbook.authority,
        "integrationId":configuration.logbook.integration_id,
        "completed":true,
        "userAuthorized":true,
        "qso":{
            "callsign":contact.callsign,
            "mode":contact.mode,
            "submode":contact.submode,
            "frequencyHz":contact.frequency_hz,
            "contactStartUtc":contact.contact_start_utc,
            "contactEndUtc":contact.contact_end_utc,
            "sentReport":contact.sent_report,
            "receivedReport":contact.received_report,
            "grid":contact.grid,
            "comment":contact.comment,
        },
    });
    send(
        &auth,
        "api/v1/nexus-desktop/contact-events",
        &operation,
        body,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(revision: u64, adif: Option<&str>) -> DesktopLogbook {
        DesktopLogbook {
            protocol: serde_json::json!({"major":1,"minor":0}),
            authority_revision: 4,
            revision,
            station_profile_id: "station-1".into(),
            authority: "WEB_LOCAL".into(),
            integration_id: None,
            complete: true,
            records: vec![DesktopLogbookRecord {
                id: "remote-1".into(),
                revision,
                deleted: adif.is_none(),
                operation_identity: None,
                adif: adif.map(String::from),
            }],
        }
    }

    const CONTACT_ADIF: &str = "<CALL:5>VK8AA<QSO_DATE:8>20260925<TIME_ON:6>120000<BAND:3>20m<MODE:3>FT8<QSL_RCVD:1>Y<EOR>";

    #[test]
    fn unassigned_legacy_is_not_an_empty_authoritys_active_history() {
        let mut legacy = tempo_core::logbook::Logbook::new();
        legacy.import_adif(CONTACT_ADIF);
        let empty = snapshot(1, None);
        let projected = project_logbook(legacy.records(), &empty, "local")
            .unwrap()
            .0;
        assert!(
            projected.is_empty(),
            "unassigned contacts must not enter selected history"
        );
        assert_eq!(legacy.len(), 1, "the source is never deleted by projection");
    }

    #[test]
    fn forgotten_pairing_recovery_is_empty_but_keeps_identified_and_legacy_records() {
        let directory = std::env::temp_dir().join(format!(
            "nexus-forget-recovery-{}-{}",
            std::process::id(),
            WRITE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        let archive = directory.join("recovery.json");
        let mut engine = tempo_app::engine::Engine::new("N0TEST", "AA00", 0);
        engine.set_log_path(directory.join("log.adi"));
        engine.import_adif(CONTACT_ADIF);
        let mut records: Vec<_> = engine.log_records().iter().map(|r| (**r).clone()).collect();
        let projected = project_logbook(&[], &snapshot(1, Some(CONTACT_ADIF)), "identified-local")
            .unwrap()
            .0;
        records.extend(projected);
        engine.replace_logbook_projection(records).unwrap();
        recover_partition_records(&mut engine, &archive, None).unwrap();
        assert!(engine.log_records().is_empty());
        let held = read_replica_archive(&archive).unwrap();
        assert_eq!(held.unassigned.len(), 1);
        assert_eq!(held.identified.len(), 1);
        let bytes = fs::read(&archive).unwrap();
        recover_partition_records(&mut engine, &archive, None).unwrap();
        assert_eq!(fs::read(&archive).unwrap(), bytes);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn legacy_archive_recovery_and_ui_caches_do_not_merge_authorities() {
        let directory = std::env::temp_dir().join(format!(
            "nexus-legacy-recovery-{}-{}",
            std::process::id(),
            WRITE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        let archive_path = directory.join("recovery.json");
        let engine: SharedEngine = std::sync::Arc::new(Mutex::new(tempo_app::engine::Engine::new(
            "N0TEST", "AA00", 0,
        )));
        engine_lock(&engine).set_log_path(directory.join("log.adi"));
        engine_lock(&engine).import_adif(CONTACT_ADIF);
        let before = super::super::log_delta(&engine, 0, 0);
        let tallies = super::super::LogTallies::default();
        assert_eq!(
            super::super::journey_kept(&engine, &tallies)
                .summary(0)
                .total_qsos,
            1
        );
        let local_id = engine_lock(&engine).log_records()[0]
            .id
            .unwrap()
            .to_string();
        // Crash after archive write but before projection replacement: replay must
        // preserve the same bytes/identity once, then exclude legacy from the view.
        for _ in 0..2 {
            archive_replica_records(&archive_path, engine_lock(&engine).log_records()).unwrap();
        }
        assert_eq!(
            read_replica_archive(&archive_path)
                .unwrap()
                .unassigned
                .len(),
            1
        );
        let empty = snapshot(1, None);
        for authority in ["local", "external", "local"] {
            let mut e = engine_lock(&engine);
            let rows = project_logbook(e.log_records(), &empty, authority)
                .unwrap()
                .0;
            e.replace_logbook_projection(rows).unwrap();
        }
        let after = super::super::log_delta(&engine, before.revision, before.rows.len());
        assert!(
            after.full && after.rows.is_empty(),
            "cached UI must receive an empty full replacement"
        );
        assert_eq!(
            super::super::journey_kept(&engine, &tallies)
                .summary(0)
                .total_qsos,
            0
        );
        assert_eq!(
            *super::super::awards_kept(&engine, &tallies),
            super::super::awards_for_records::<tempo_core::logbook::QsoRecord>(&[], "N0TEST")
        );
        assert!(durable_archived_contact(&archive_path, &local_id).unwrap());
        let bytes = fs::read(&archive_path).unwrap();
        archive_replica_records(&archive_path, engine_lock(&engine).log_records()).unwrap();
        assert_eq!(fs::read(&archive_path).unwrap(), bytes);
        // Interrupted temporary data never overwrites the durable archive.
        fs::write(archive_path.with_extension("tmp"), "incomplete").unwrap();
        assert_eq!(
            read_replica_archive(&archive_path)
                .unwrap()
                .unassigned
                .len(),
            1
        );
        fs::remove_dir_all(directory).unwrap();
    }

    fn held(
        records: Vec<tempo_core::logbook::QsoRecord>,
    ) -> Vec<std::sync::Arc<tempo_core::logbook::QsoRecord>> {
        let mut log = tempo_core::logbook::Logbook::new();
        for record in records {
            log.add(record);
        }
        log.records().to_vec()
    }

    #[test]
    fn authoritative_projection_handles_create_update_qsl_clear_delete_and_switch() {
        let first = snapshot(1, Some(CONTACT_ADIF));
        let (records, added, _) = project_logbook(&[], &first, "local").unwrap();
        assert_eq!(added, 1);
        let existing = held(records);
        assert!(existing[0].qsl_rcvd.card);
        let edited = CONTACT_ADIF
            .replace("VK8AA", "VK8BB")
            .replace("QSL_RCVD:1>Y", "QSL_RCVD:1>N");
        let next = snapshot(2, Some(&edited));
        let (records, added, updated) = project_logbook(&existing, &next, "local").unwrap();
        assert_eq!((records.len(), added, updated), (1, 0, 1));
        assert_eq!(records[0].id, existing[0].id);
        assert_eq!(records[0].call, "VK8BB");
        assert!(!records[0].qsl_rcvd.card);
        let deleted = snapshot(3, None);
        assert!(project_logbook(&held(records), &deleted, "local")
            .unwrap()
            .0
            .is_empty());
        let switched = project_logbook(&existing, &next, "managed-1").unwrap().0;
        assert_eq!(switched.len(), 1);
        assert!(switched[0]
            .extra
            .iter()
            .any(|(key, value)| key == "APP_SHACKCQ_PARTITION" && value == "managed-1"));
        assert_eq!(
            validate_snapshot_revision(&first, Some(&next)),
            Err("obsoleteLogbookSnapshot")
        );
        let mut old_authority = snapshot(100, Some(CONTACT_ADIF));
        old_authority.authority_revision = 3;
        assert_eq!(
            validate_snapshot_revision(&old_authority, Some(&next)),
            Err("obsoleteLogbookSnapshot")
        );
    }

    #[test]
    fn exact_local_echo_is_replaced_once_and_unassigned_contacts_are_excluded() {
        let mut local = tempo_core::logbook::Logbook::new();
        local.import_adif(CONTACT_ADIF);
        let id = local.records()[0].id.unwrap();
        let mut incoming = snapshot(1, Some(CONTACT_ADIF));
        incoming.records[0].operation_identity = Some(format!("nexus-{id}"));
        let projected = project_logbook(local.records(), &incoming, "local")
            .unwrap()
            .0;
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].id, Some(id));
        let replay = project_logbook(&held(projected), &incoming, "local").unwrap();
        assert_eq!((replay.0.len(), replay.1, replay.2), (1, 0, 0));
        incoming.records[0].operation_identity = None;
        assert_eq!(
            project_logbook(local.records(), &incoming, "local")
                .unwrap()
                .0
                .len(),
            1
        );
    }

    #[test]
    fn prepared_contact_requires_persisted_file_not_memory_presence() {
        let mut local = tempo_core::logbook::Logbook::new();
        local.import_adif(CONTACT_ADIF);
        let id = local.records()[0].id.unwrap().to_string();
        let directory = std::env::temp_dir().join(format!(
            "nexus-log-durability-{}-{}",
            std::process::id(),
            WRITE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("log.adi");
        assert!(durable_contact_on_disk(&path, &id).is_err());
        local.save(&path).unwrap();
        assert_eq!(durable_contact_on_disk(&path, &id), Ok(true));
        assert_eq!(durable_contact_on_disk(&path, "missing-record"), Ok(false));
        fs::remove_dir_all(directory).unwrap();
    }

    fn auth(origin: &str) -> ConnectorAuth {
        ConnectorAuth {
            origin: origin.into(),
            credential_alias: "station-1-token".into(),
            station_profile_id: "station-1".into(),
            device_id: "nexus-review-1".into(),
        }
    }

    #[test]
    fn accepts_https_and_loopback_but_rejects_remote_plaintext_and_credentials() {
        assert!(endpoint(&auth("https://shackcq.com"), "api/v1/test").is_ok());
        assert!(endpoint(&auth("http://127.0.0.1:3000"), "api/v1/test").is_ok());
        assert_eq!(
            endpoint(&auth("http://example.com"), "api/v1/test").unwrap_err(),
            "originDenied"
        );
        assert_eq!(
            endpoint(&auth("https://user@example.com"), "api/v1/test").unwrap_err(),
            "originDenied"
        );
    }

    #[test]
    fn credential_alias_and_identity_are_bounded() {
        let mut invalid = auth("https://shackcq.com");
        invalid.credential_alias = "../escape".into();
        assert_eq!(
            endpoint(&invalid, "api/v1/test").unwrap_err(),
            "invalidConnectorIdentity"
        );
        invalid = auth("https://shackcq.com");
        invalid.station_profile_id = "../escape".into();
        assert_eq!(
            endpoint(&invalid, "api/v1/test").unwrap_err(),
            "invalidConnectorIdentity"
        );
    }

    #[test]
    fn review_distribution_cannot_arm_production_transmit() {
        assert!(!SHACKCQ_DIGI_TX_ENABLED);
        assert_eq!(
            shackcq_connector_capabilities()["productionDigiTxEnabled"],
            false
        );
    }

    #[test]
    fn streamed_response_is_rejected_before_it_exceeds_the_bound() {
        let mut response = Vec::new();
        extend_bounded_response(&mut response, &vec![0; MAX_RESPONSE_BYTES]).unwrap();
        assert_eq!(response.len(), MAX_RESPONSE_BYTES);
        assert_eq!(
            extend_bounded_response(&mut response, &[0]).unwrap_err(),
            "invalidResponse"
        );
        assert_eq!(response.len(), MAX_RESPONSE_BYTES);
    }

    #[test]
    fn selected_logbook_must_match_the_paired_station_and_supported_authority() {
        let auth = auth("https://shackcq.com");
        let local = DesktopConfiguration {
            protocol: serde_json::json!({"major":1,"minor":0}),
            revision: 7,
            station_profile_id: "station-1".into(),
            logbook: SelectedLogbook {
                authority: "WEB_LOCAL".into(),
                connection_kind: "LOCAL".into(),
                integration_id: None,
                automatic_sync: true,
            },
        };
        assert!(validate_configuration(&auth, &local).is_ok());
        let mut wrong_station = local.clone();
        wrong_station.station_profile_id = "station-2".into();
        assert_eq!(
            validate_configuration(&auth, &wrong_station),
            Err("invalidConfiguration")
        );
        let mut invalid_wavelog = local;
        invalid_wavelog.logbook.authority = "WAVELOG".into();
        assert_eq!(
            validate_configuration(&auth, &invalid_wavelog),
            Err("invalidConfiguration")
        );
    }

    fn contact_event() -> (ContactOutboxEvent, DesktopConfiguration) {
        let configuration = DesktopConfiguration {
            protocol: serde_json::json!({"major":1,"minor":0}),
            revision: 7,
            station_profile_id: "station-1".into(),
            logbook: SelectedLogbook {
                authority: "WEB_LOCAL".into(),
                connection_kind: "LOCAL".into(),
                integration_id: None,
                automatic_sync: true,
            },
        };
        (
            ContactOutboxEvent {
                version: 1,
                committed: true,
                agent_id: "agent-1".into(),
                station_profile_id: "station-1".into(),
                operation_identity: "nexus-contact-1".into(),
                local_contact_id: "contact-1".into(),
                created_utc: "2026-09-26T00:00:00.000Z".into(),
                authority: Some(configuration.clone()),
                contact: CompletedContact {
                    operation_identity: "nexus-contact-1".into(),
                    callsign: "N0CALL".into(),
                    mode: "FT8".into(),
                    submode: None,
                    frequency_hz: Some(14_074_000),
                    contact_start_utc: "2026-09-26T00:00:00Z".into(),
                    contact_end_utc: Some("2026-09-26T00:01:00Z".into()),
                    sent_report: Some("-10".into()),
                    received_report: Some("-12".into()),
                    grid: Some("AA00AA".into()),
                    comment: "synthetic fixture".into(),
                    user_authorized: true,
                },
            },
            configuration,
        )
    }

    #[test]
    fn explicit_contact_retry_is_durable_and_preserves_the_original_event() {
        let root = std::env::temp_dir().join(format!(
            "shackcq-retry-test-{}-{}",
            std::process::id(),
            WRITE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let path = root.join("contact.json");
        let (event, configuration) = contact_event();
        let bytes = serde_json::to_vec(&event).unwrap();
        durable_write(&path, &bytes).unwrap();
        assert!(
            record_contact_failure(&path, "accessDenied", 100)
                .unwrap()
                .terminal
        );

        rearm_contact_attempt(&path, &configuration, &configuration).unwrap();
        // Reopen the actual journal to simulate restart, not an in-memory copy.
        let attempt: ContactAttempt =
            serde_json::from_slice(&fs::read(path.with_extension("attempt")).unwrap()).unwrap();
        assert!(!attempt.terminal);
        assert_eq!(attempt.retry_after, 0);
        assert_eq!(attempt.attempts, 1);
        assert_eq!(attempt.error, "accessDenied");
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let events = read_outbox_events(&root).unwrap();
        assert_eq!(
            events.len(),
            1,
            "retry must not duplicate the contact event"
        );
        assert_eq!(
            events[0].1.as_ref().unwrap().operation_identity,
            event.operation_identity
        );
        assert_eq!(
            rearm_contact_attempt(&path, &configuration, &configuration),
            Err("contactRetryNotAllowed"),
            "an already rearmed event is not re-created by repeated requests"
        );

        // A still-revoked credential stops again, including after a restart.
        let refused = record_contact_failure(&path, "accessDenied", 200).unwrap();
        assert!(refused.terminal);
        assert_eq!(refused.attempts, 2);
        assert!(!retryable_contact_error("accessDenied"));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_contact_retry_refuses_obsolete_destination_and_other_terminal_failures() {
        let root = std::env::temp_dir().join(format!(
            "shackcq-retry-refusal-test-{}-{}",
            std::process::id(),
            WRITE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let path = root.join("contact.json");
        let (event, original) = contact_event();
        durable_write(&path, &serde_json::to_vec(&event).unwrap()).unwrap();
        record_contact_failure(&path, "accessDenied", 100).unwrap();
        let journal = fs::read(path.with_extension("attempt")).unwrap();
        let mut destinations = vec![original.clone(); 4];
        destinations[0].revision += 1; // Includes switching away and back.
        destinations[1].station_profile_id = "station-2".into();
        destinations[2].logbook.authority = "WAVELOG".into();
        destinations[3].logbook.integration_id = Some("different-integration".into());
        for selected in destinations {
            assert_eq!(
                rearm_contact_attempt(&path, &original, &selected),
                Err("outboxDestinationChanged")
            );
            assert_eq!(fs::read(path.with_extension("attempt")).unwrap(), journal);
        }
        for code in [
            "configurationInvalid",
            "durableReceiptInvalid",
            "invalidContact",
        ] {
            record_contact_failure(&path, code, 200).unwrap();
            let journal = fs::read(path.with_extension("attempt")).unwrap();
            assert_eq!(
                rearm_contact_attempt(&path, &original, &original),
                Err("contactRetryNotAllowed")
            );
            assert_eq!(fs::read(path.with_extension("attempt")).unwrap(), journal);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn outbox_requires_a_bound_durable_receipt_before_cleanup() {
        let (event, configuration) = contact_event();
        let valid = ConnectorReceipt {
            status: 202,
            body: serde_json::json!({
                "acceptedByShackcq":true,
                "receipt":{
                    "operationIdentity":"nexus-contact-1",
                    "qsoId":"qso-1",
                    "revision":1,
                    "authority":"WEB_LOCAL",
                    "authorityRevision":7,
                    "integrationId":null,
                    "stationProfileId":"station-1"
                },
                "providerState":"NOT_APPLICABLE"
            }),
        };
        assert!(validate_contact_receipt(&valid, &event, &configuration).is_ok());

        let missing = ConnectorReceipt {
            status: 202,
            body: serde_json::json!({"acceptedByShackcq":true}),
        };
        assert_eq!(
            validate_contact_receipt(&missing, &event, &configuration).unwrap_err(),
            "durableReceiptMissing"
        );

        let mut wrong_destination = valid;
        wrong_destination.body["receipt"]["authorityRevision"] = serde_json::json!(8);
        assert_eq!(
            validate_contact_receipt(&wrong_destination, &event, &configuration).unwrap_err(),
            "durableReceiptInvalid"
        );
    }

    #[test]
    fn malformed_outbox_and_permanent_failure_leave_later_events_available() {
        let root = std::env::temp_dir().join(format!(
            "shackcq-outbox-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::create_dir(&root).unwrap();
        let bad = root.join("a.json");
        let good = root.join("b.json");
        fs::write(&bad, b"invalid json retained for recovery").unwrap();
        fs::write(&good, serde_json::to_vec(&contact_event().0).unwrap()).unwrap();
        let events = read_outbox_events(&root).unwrap();
        assert_eq!(events.len(), 2);
        assert!(events[0].1.is_err());
        assert!(events[1].1.is_ok());
        let failed = record_contact_failure(&bad, "configurationInvalid", 100).unwrap();
        assert!(failed.terminal);
        assert_eq!(
            fs::read(&bad).unwrap(),
            b"invalid json retained for recovery"
        );
        assert!(read_outbox_events(&root).unwrap()[1].1.is_ok());
        for code in [
            "contactRejected",
            "operationConflict",
            "accessDenied",
            "durableReceiptInvalid",
        ] {
            assert!(!retryable_contact_error(code));
        }
        let retry = record_contact_failure(&good, "serviceUnavailable", 100).unwrap();
        assert!(!retry.terminal && retry.retry_after > 100);
        assert!(
            record_contact_failure(&good, "tryLater", 200)
                .unwrap()
                .retry_after
                > retry.retry_after
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn destination_resume_keeps_original_revision_and_rejects_other_partitions() {
        let (event, original) = contact_event();
        let original_bytes = serde_json::to_vec(&event).unwrap();
        let mut selected = original.clone();
        selected.revision += 2;
        assert!(same_destination(&original, &selected));
        selected.logbook.authority = "WAVELOG".into();
        selected.logbook.integration_id = Some("other-integration".into());
        assert!(!same_destination(&original, &selected));
        selected = original.clone();
        selected.station_profile_id = "other-station".into();
        assert!(!same_destination(&original, &selected));
        selected = original.clone();
        selected.revision -= 1;
        assert!(!same_destination(&original, &selected));
        assert_eq!(serde_json::to_vec(&event).unwrap(), original_bytes);
    }
}
