//! Wavelog API v2 QSO synchronisation.
//!
//! Deletion is propagated only through a persisted one-to-one binding. Absence is never matched
//! by callsign/date heuristics, and an implausible bulk disappearance aborts before either log is
//! changed. This makes a deliberate delete two-way without turning a filter/configuration error
//! into a logbook purge.

use reqwest::blocking::{Client, Response};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;
use tempo_app::engine::{engine_lock, Engine};
use tempo_core::logbook::{LogOp, QsoEdit, QsoRecord, RecordId};

const PAGE_SIZE: usize = 5_000;
const MAX_PAGE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_JSON_BYTES: u64 = 4 * 1024 * 1024;
const MAX_REMOTE_QSOS: usize = 2_000_000;
const MAX_REMOTE_PAGES: usize = 10_000;
const MAX_DELETIONS_PER_SYNC: usize = 25;
const STATE_VERSION: u32 = 2;
const CREATE_RECEIPT_FIELD: &str = "APP_RDNEXUS_LOCALID";
static SYNC_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub downloaded: usize,
    pub uploaded: usize,
    pub local_updates: usize,
    pub remote_updates: usize,
    pub local_deletes: usize,
    pub remote_deletes: usize,
    pub matched: usize,
    pub conflicts_resolved_from_wavelog: usize,
    pub synced_local: usize,
    pub synced_remote: usize,
    pub local_only: usize,
    pub remote_only: usize,
    pub remote_total: usize,
    pub local_total: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Station {
    pub station_id: String,
    pub profile_name: String,
    pub callsign: String,
    pub gridsquare: String,
    pub active: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct CommonQso {
    call: String,
    band: String,
    mode: String,
    qso_date: String,
    time_on: String,
    time_off: Option<String>,
    freq_hz: Option<u64>,
    freq_rx_hz: Option<u64>,
    rst_sent: Option<String>,
    rst_rcvd: Option<String>,
    gridsquare: Option<String>,
    name: Option<String>,
    qth: Option<String>,
    comment: Option<String>,
    notes: Option<String>,
    tx_pwr: Option<String>,
    prop_mode: Option<String>,
    sat_name: Option<String>,
    state: Option<String>,
    iota: Option<String>,
    sota_ref: Option<String>,
    pota_ref: Option<String>,
    sig: Option<String>,
    sig_info: Option<String>,
    srx: Option<String>,
    stx: Option<String>,
    srx_string: Option<String>,
    stx_string: Option<String>,
    #[serde(default)]
    extra: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct RemoteQso {
    id: String,
    qso: CommonQso,
    create_receipt: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct PaginationMeta {
    page: usize,
    per_page: usize,
    count: usize,
    total: usize,
    total_pages: usize,
    has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Binding {
    local_id: String,
    last_local: CommonQso,
    last_remote: CommonQso,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PendingOperation {
    Create { local_id: String, qso: CommonQso },
    DeleteRemote { remote_id: String, local_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SyncState {
    version: u32,
    endpoint: String,
    station_id: String,
    #[serde(default)]
    bindings: BTreeMap<String, Binding>,
    #[serde(default)]
    pending: BTreeMap<String, PendingOperation>,
    #[serde(default)]
    last_sync_unix: u64,
    #[serde(default)]
    last_remote_total: usize,
    #[serde(default)]
    last_local_total: usize,
}

impl SyncState {
    fn fresh(endpoint: &str, station_id: &str) -> Self {
        Self {
            version: STATE_VERSION,
            endpoint: endpoint.to_string(),
            station_id: station_id.to_string(),
            bindings: BTreeMap::new(),
            pending: BTreeMap::new(),
            last_sync_unix: 0,
            last_remote_total: 0,
            last_local_total: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub local_total: usize,
    pub remote_total: usize,
    pub synced_local: usize,
    pub synced_remote: usize,
    pub local_only: usize,
    pub remote_only: usize,
    pub last_sync_unix: u64,
    pub interval_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StatusState {
    version: u32,
    endpoint: String,
    station_id: String,
    status: SyncStatus,
}

fn api_base(input: &str) -> Result<String, String> {
    let mut base = input.trim().trim_end_matches('/').to_string();
    if !base.starts_with("https://") && !cfg!(test) {
        return Err("Wavelog API v2 requires an HTTPS URL".into());
    }
    for suffix in ["/index.php/api/v2", "/api/v2", "/index.php"] {
        if base.ends_with(suffix) {
            base.truncate(base.len() - suffix.len());
            break;
        }
    }
    if base.is_empty() {
        return Err("Wavelog URL is empty".into());
    }
    Ok(format!("{base}/index.php/api/v2"))
}

fn client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("RD-Nexus/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "could not create the Wavelog HTTP client".into())
}

fn bearer(token: &str) -> String {
    format!("Bearer {}", token.trim())
}

fn checked(response: Response) -> Result<Response, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let label = match status.as_u16() {
        401 | 403 => "Wavelog refused the API v2 token or station access",
        404 => "the configured URL does not expose Wavelog API v2",
        409 | 422 => "Wavelog refused the QSO data",
        429 => "Wavelog rate-limited the sync",
        500..=599 => "Wavelog reported a server error",
        _ => "Wavelog refused the request",
    };
    Err(format!("{label} (HTTP {})", status.as_u16()))
}

fn limited_json(response: Response, limit: u64, label: &str) -> Result<Value, String> {
    if response.content_length().is_some_and(|n| n > limit) {
        return Err(format!("Wavelog returned an oversized {label} response"));
    }
    let mut bytes = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| format!("could not read the Wavelog {label} response"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("Wavelog returned an oversized {label} response"));
    }
    serde_json::from_slice(&bytes).map_err(|_| format!("Wavelog returned invalid {label} JSON"))
}

fn object_data(value: Value) -> Value {
    match value {
        Value::Object(mut o) => o.remove("data").unwrap_or(Value::Object(o)),
        other => other,
    }
}

fn array_data(value: Value) -> Vec<Value> {
    match object_data(value) {
        Value::Array(a) => a,
        Value::Object(mut o) => o
            .remove("qsos")
            .or_else(|| o.remove("stations"))
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn value<'a>(obj: &'a Map<String, Value>, names: &[&str]) -> Option<&'a Value> {
    names.iter().find_map(|name| {
        obj.get(*name).or_else(|| {
            obj.iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value)
        })
    })
}

fn text_field(obj: &Map<String, Value>, names: &[&str]) -> Option<String> {
    value(obj, names).and_then(|v| match v {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(if *b { "1" } else { "0" }.into()),
        _ => None,
    })
}

fn bool_field(obj: &Map<String, Value>, names: &[&str]) -> bool {
    text_field(obj, names)
        .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "y"))
}

fn adif_extras(obj: &Map<String, Value>) -> BTreeMap<String, String> {
    const SKIP: &[&str] = &[
        "ID",
        "QSO_ID",
        "PRIMARY_KEY",
        "STATION_ID",
        "STATION_PROFILE_ID",
        "USER_ID",
        "CREATED_AT",
        "UPDATED_AT",
        "TIME_ON",
        "TIME_OFF",
        "QSO_DATE",
        "CALL",
        "BAND",
        "MODE",
        "SUBMODE",
        "FREQ",
        "FREQ_RX",
        "RST_SENT",
        "RST_RCVD",
        "GRIDSQUARE",
        "NAME",
        "QTH",
        "COMMENT",
        "NOTES",
        "TX_PWR",
        "PROP_MODE",
        "SAT_NAME",
        "STATE",
        "IOTA",
        "SOTA_REF",
        "POTA_REF",
        "SIG",
        "SIG_INFO",
        "SRX",
        "STX",
        "SRX_STRING",
        "STX_STRING",
    ];
    obj.iter()
        .filter_map(|(raw, value)| {
            let name = raw
                .strip_prefix("COL_")
                .or_else(|| raw.strip_prefix("col_"))
                .unwrap_or(raw)
                .to_ascii_uppercase();
            if SKIP.contains(&name.as_str())
                || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return None;
            }
            let text = match value {
                Value::String(s) if !s.is_empty() => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(v) => {
                    if *v {
                        "Y".into()
                    } else {
                        "N".into()
                    }
                }
                _ => return None,
            };
            Some((name, text))
        })
        .collect()
}

fn remote_from_value(value: Value) -> Option<RemoteQso> {
    let Value::Object(obj) = value else {
        return None;
    };
    let id = text_field(&obj, &["id", "qso_id", "col_primary_key"])?;
    let call = text_field(&obj, &["call", "col_call"])?;
    let band = text_field(&obj, &["band", "col_band"])?;
    let mode = text_field(&obj, &["submode", "col_submode"])
        .or_else(|| text_field(&obj, &["mode", "col_mode"]))?;
    let timestamp = text_field(&obj, &["qso_date", "col_time_on"])?;
    let qso_date = {
        let s = timestamp.clone();
        if s.len() >= 10 {
            s[..10].replace('-', "")
        } else {
            s
        }
    };
    // API v2 returns a combined `qso_date` timestamp (for example
    // `2026-06-16 17:06:00`) and does not include a separate `time_on` field.
    // Database-shaped responses use COL_TIME_ON, while accepting `time_on`
    // keeps compatibility with older development builds of the endpoint.
    let time_on = text_field(&obj, &["time_on", "col_time_on"]).unwrap_or(timestamp);
    let time_on = time_part(&time_on);
    let freq_hz = text_field(&obj, &["freq", "col_freq"]).and_then(|s| parse_freq_hz(&s));
    let mut extra = adif_extras(&obj);
    let create_receipt = extra.remove(CREATE_RECEIPT_FIELD);
    Some(RemoteQso {
        id,
        create_receipt,
        qso: CommonQso {
            call: call.to_ascii_uppercase(),
            band: band.to_ascii_uppercase(),
            mode: mode.to_ascii_uppercase(),
            qso_date,
            time_on,
            time_off: text_field(&obj, &["time_off", "col_time_off"]).map(|s| time_part(&s)),
            freq_hz,
            freq_rx_hz: text_field(&obj, &["freq_rx", "col_freq_rx"])
                .and_then(|s| parse_freq_hz(&s)),
            rst_sent: text_field(&obj, &["rst_sent", "col_rst_sent"]),
            rst_rcvd: text_field(&obj, &["rst_rcvd", "col_rst_rcvd"]),
            gridsquare: text_field(&obj, &["gridsquare", "col_gridsquare"]),
            name: text_field(&obj, &["name", "col_name"]),
            qth: text_field(&obj, &["qth", "col_qth"]),
            comment: text_field(&obj, &["comment", "col_comment"]),
            notes: text_field(&obj, &["notes", "col_notes"]),
            tx_pwr: text_field(&obj, &["tx_pwr", "col_tx_pwr"])
                .map(|value| canonical_number(&value)),
            prop_mode: text_field(&obj, &["prop_mode", "col_prop_mode"]),
            sat_name: text_field(&obj, &["sat_name", "col_sat_name"]),
            state: text_field(&obj, &["state", "col_state"]),
            iota: text_field(&obj, &["iota", "col_iota"]),
            sota_ref: text_field(&obj, &["sota_ref", "col_sota_ref"]),
            pota_ref: text_field(&obj, &["pota_ref", "col_pota_ref"]),
            sig: text_field(&obj, &["sig", "col_sig"]),
            sig_info: text_field(&obj, &["sig_info", "col_sig_info"]),
            srx: text_field(&obj, &["srx", "col_srx"]),
            stx: text_field(&obj, &["stx", "col_stx"]),
            srx_string: text_field(&obj, &["srx_string", "col_srx_string"]),
            stx_string: text_field(&obj, &["stx_string", "col_stx_string"]),
            extra,
        },
    })
}

fn time_part(value: &str) -> String {
    let tail = value.rsplit_once(' ').map_or(value, |(_, t)| t);
    tail.trim_end_matches('Z')
        .replace(':', "")
        .chars()
        .take(6)
        .collect()
}

fn parse_freq_hz(value: &str) -> Option<u64> {
    let n = value.parse::<f64>().ok()?;
    Some(if n < 1_000_000.0 {
        (n * 1_000_000.0).round() as u64
    } else {
        n.round() as u64
    })
}

fn canonical_number(value: &str) -> String {
    value
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .map(|v| {
            let mut text = v.to_string();
            if text == "-0" {
                text = "0".into();
            }
            text
        })
        .unwrap_or_else(|| value.trim().to_string())
}

fn optional_text(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn fetch_qsos(base: &str, token: &str, station_id: &str) -> Result<Vec<RemoteQso>, String> {
    let client = client()?;
    let mut out = Vec::new();
    let mut remote_ids = HashSet::new();
    let mut expected_total = None;
    let mut expected_pages = None;
    let mut page = 1usize;
    loop {
        if page > MAX_REMOTE_PAGES {
            return Err("Wavelog pagination exceeded the safety page limit".into());
        }
        let response = checked(
            client
                .get(format!("{base}/qso"))
                .header(AUTHORIZATION, bearer(token))
                .header(ACCEPT, "application/json")
                .query(&[
                    ("station_id", station_id),
                    ("page", &page.to_string()),
                    ("per_page", &PAGE_SIZE.to_string()),
                ])
                .send()
                .map_err(|_| "could not reach Wavelog API v2".to_string())?,
        )?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_PAGE_BYTES)
        {
            return Err("Wavelog returned an implausibly large QSO page".into());
        }
        let body = limited_json(response, MAX_PAGE_BYTES, "QSO page")?;
        let meta: PaginationMeta = serde_json::from_value(
            body.get("meta")
                .cloned()
                .ok_or_else(|| "Wavelog omitted the QSO pagination proof".to_string())?,
        )
        .map_err(|_| "Wavelog returned incomplete QSO pagination metadata".to_string())?;
        let rows = array_data(body);
        let count = rows.len();
        if meta.page != page || meta.count != count || meta.per_page == 0 {
            return Err("Wavelog returned inconsistent QSO pagination metadata".into());
        }
        if expected_total.is_some_and(|total| total != meta.total)
            || expected_pages.is_some_and(|pages| pages != meta.total_pages)
        {
            return Err(
                "Wavelog QSO totals changed during pagination; no changes were applied".into(),
            );
        }
        expected_total = Some(meta.total);
        expected_pages = Some(meta.total_pages);
        let calculated_pages = meta.total.div_ceil(meta.per_page);
        if (meta.total == 0 && !matches!(meta.total_pages, 0 | 1))
            || (meta.total > 0 && meta.total_pages != calculated_pages)
            || meta.has_more != (page < meta.total_pages)
        {
            return Err("Wavelog returned contradictory QSO pagination metadata".into());
        }
        for row in &rows {
            let Value::Object(obj) = row else {
                return Err("Wavelog returned a non-object QSO row".into());
            };
            let row_station =
                text_field(obj, &["station_id", "station_profile_id", "COL_STATION_ID"])
                    .ok_or_else(|| "Wavelog omitted a QSO station id".to_string())?;
            if row_station != station_id {
                return Err("Wavelog returned a QSO from a different station profile".into());
            }
        }
        let parsed: Vec<_> = rows.into_iter().filter_map(remote_from_value).collect();
        if parsed.len() != count {
            return Err("Wavelog returned a QSO shape RD-Nexus could not safely interpret".into());
        }
        for row in &parsed {
            if !remote_ids.insert(row.id.clone()) {
                return Err(
                    "Wavelog repeated a QSO id during pagination; no changes were applied".into(),
                );
            }
        }
        out.extend(parsed);
        if out.len() > MAX_REMOTE_QSOS || out.len() > meta.total {
            return Err("Wavelog returned more QSOs than its pagination proof allows".into());
        }
        if meta.has_more {
            page += 1;
        } else {
            break;
        }
    }
    if out.len() != expected_total.unwrap_or_default() {
        return Err("Wavelog returned a truncated QSO list; no changes were applied".into());
    }
    Ok(out)
}

pub fn stations(url: &str, token: &str) -> Result<Vec<Station>, String> {
    let base = api_base(url)?;
    let response = checked(
        client()?
            .get(format!("{base}/station"))
            .header(AUTHORIZATION, bearer(token))
            .header(ACCEPT, "application/json")
            .send()
            .map_err(|_| "could not reach Wavelog API v2".to_string())?,
    )?;
    let value = limited_json(response, MAX_JSON_BYTES, "station")?;
    Ok(array_data(value)
        .into_iter()
        .filter_map(|v| {
            let Value::Object(obj) = v else { return None };
            Some(Station {
                station_id: text_field(
                    &obj,
                    &["id", "station_id", "station_profile_id", "station_id"],
                )?,
                profile_name: text_field(&obj, &["station_profile_name", "profile_name", "name"])
                    .unwrap_or_default(),
                callsign: text_field(&obj, &["station_callsign", "callsign", "call"])
                    .unwrap_or_default(),
                gridsquare: text_field(&obj, &["station_gridsquare", "gridsquare", "grid"])
                    .unwrap_or_default(),
                active: bool_field(&obj, &["active", "station_active"]),
            })
        })
        .collect())
}

fn qso_json(qso: &CommonQso, station_id: &str, include_all_adif: bool) -> Value {
    let mut o = Map::new();
    let date = if qso.qso_date.len() == 8 {
        format!(
            "{}-{}-{}",
            &qso.qso_date[0..4],
            &qso.qso_date[4..6],
            &qso.qso_date[6..8]
        )
    } else {
        qso.qso_date.clone()
    };
    o.insert(
        "station_profile_id".into(),
        json!(station_id.parse::<u64>().unwrap_or_default()),
    );
    o.insert("call".into(), json!(qso.call));
    o.insert("band".into(), json!(qso.band));
    o.insert("mode".into(), json!(qso.mode));
    o.insert("qso_date".into(), json!(date));
    o.insert("time_on".into(), json!(format_time(&qso.time_on)));
    optional(&mut o, "time_off", qso.time_off.as_deref().map(format_time));
    optional(&mut o, "freq", qso.freq_hz);
    optional(&mut o, "freq_rx", qso.freq_rx_hz);
    optional(&mut o, "rst_sent", qso.rst_sent.clone());
    optional(&mut o, "rst_rcvd", qso.rst_rcvd.clone());
    optional(&mut o, "gridsquare", qso.gridsquare.clone());
    optional(&mut o, "name", qso.name.clone());
    optional(&mut o, "qth", qso.qth.clone());
    optional(&mut o, "comment", qso.comment.clone());
    optional(&mut o, "notes", qso.notes.clone());
    optional(&mut o, "tx_pwr", qso.tx_pwr.clone());
    optional(&mut o, "prop_mode", qso.prop_mode.clone());
    optional(&mut o, "sat_name", qso.sat_name.clone());
    optional(&mut o, "state", qso.state.clone());
    optional(&mut o, "iota", qso.iota.clone());
    optional(&mut o, "sota_ref", qso.sota_ref.clone());
    optional(&mut o, "pota_ref", qso.pota_ref.clone());
    optional(&mut o, "sig", qso.sig.clone());
    optional(&mut o, "sig_info", qso.sig_info.clone());
    optional(&mut o, "srx", qso.srx.clone());
    optional(&mut o, "stx", qso.stx.clone());
    optional(&mut o, "srx_string", qso.srx_string.clone());
    optional(&mut o, "stx_string", qso.stx_string.clone());
    if include_all_adif {
        for (name, value) in &qso.extra {
            o.entry(name.to_ascii_lowercase())
                .or_insert_with(|| json!(value));
        }
    }
    Value::Object(o)
}

fn optional<T: Serialize>(o: &mut Map<String, Value>, key: &str, value: Option<T>) {
    if let Some(value) = value {
        o.insert(key.into(), json!(value));
    }
}

fn format_time(value: &str) -> String {
    match value.len() {
        4 => format!("{}:{}:00", &value[0..2], &value[2..4]),
        6.. => format!("{}:{}:{}", &value[0..2], &value[2..4], &value[4..6]),
        _ => value.to_string(),
    }
}

fn create_qso(
    base: &str,
    token: &str,
    station_id: &str,
    qso: &CommonQso,
) -> Result<RemoteQso, String> {
    let response = checked(
        client()?
            .post(format!("{base}/qso"))
            .header(AUTHORIZATION, bearer(token))
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .json(&qso_json(qso, station_id, true))
            .send()
            .map_err(|_| "could not reach Wavelog API v2".to_string())?,
    )?;
    let value = limited_json(response, MAX_JSON_BYTES, "created-QSO")?;
    remote_from_value(object_data(value))
        .ok_or_else(|| "Wavelog created the QSO but returned no usable QSO id".into())
}

fn patch_qso(
    base: &str,
    token: &str,
    id: &str,
    station_id: &str,
    qso: &CommonQso,
) -> Result<(), String> {
    checked(
        client()?
            .patch(format!("{base}/qso/{id}"))
            .header(AUTHORIZATION, bearer(token))
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .json(&qso_json(qso, station_id, false))
            .send()
            .map_err(|_| "could not reach Wavelog API v2".to_string())?,
    )?;
    Ok(())
}

fn delete_qso(base: &str, token: &str, id: &str) -> Result<(), String> {
    let response = client()?
        .delete(format!("{base}/qso/{id}"))
        .header(AUTHORIZATION, bearer(token))
        .header(ACCEPT, "application/json")
        .send()
        .map_err(|_| "could not reach Wavelog API v2".to_string())?;
    if response.status().is_success() || response.status().as_u16() == 404 {
        Ok(())
    } else {
        checked(response).map(|_| ())
    }
}

fn create_op_key(local_id: &str) -> String {
    format!("create:{local_id}")
}

fn delete_op_key(remote_id: &str) -> String {
    format!("delete:{remote_id}")
}

fn qso_with_receipt(qso: &CommonQso, local_id: &str) -> CommonQso {
    let mut tagged = qso.clone();
    tagged
        .extra
        .insert(CREATE_RECEIPT_FIELD.into(), local_id.into());
    tagged
}

fn finalize_create(
    state_path: &Path,
    state: &mut SyncState,
    remote_id: String,
    local_id: String,
    qso: CommonQso,
) -> Result<String, String> {
    state.bindings.insert(
        remote_id.clone(),
        Binding {
            local_id: local_id.clone(),
            last_local: qso.clone(),
            last_remote: qso,
        },
    );
    state.pending.remove(&create_op_key(&local_id));
    save_state(state_path, state)?;
    Ok(remote_id)
}

fn create_with_journal(
    base: &str,
    token: &str,
    station_id: &str,
    state_path: &Path,
    state: &mut SyncState,
    local_id: &str,
    qso: &CommonQso,
    remote: &[RemoteQso],
) -> Result<(String, bool), String> {
    if let Some((remote_id, _)) = state
        .bindings
        .iter()
        .find(|(_, binding)| binding.local_id == local_id)
    {
        return Ok((remote_id.clone(), false));
    }
    if let Some(found) = remote
        .iter()
        .find(|row| row.create_receipt.as_deref() == Some(local_id))
    {
        let id = finalize_create(
            state_path,
            state,
            found.id.clone(),
            local_id.to_string(),
            qso.clone(),
        )?;
        return Ok((id, false));
    }
    let key = create_op_key(local_id);
    if !state.pending.contains_key(&key) {
        state.pending.insert(
            key,
            PendingOperation::Create {
                local_id: local_id.to_string(),
                qso: qso.clone(),
            },
        );
        save_state(state_path, state)?;
    }
    let made = create_qso(base, token, station_id, &qso_with_receipt(qso, local_id))?;
    let id = finalize_create(
        state_path,
        state,
        made.id,
        local_id.to_string(),
        qso.clone(),
    )?;
    Ok((id, true))
}

fn delete_remote_with_journal(
    base: &str,
    token: &str,
    state_path: &Path,
    state: &mut SyncState,
    remote_id: &str,
    local_id: &str,
) -> Result<(), String> {
    let key = delete_op_key(remote_id);
    if !state.pending.contains_key(&key) {
        state.pending.insert(
            key.clone(),
            PendingOperation::DeleteRemote {
                remote_id: remote_id.to_string(),
                local_id: local_id.to_string(),
            },
        );
        save_state(state_path, state)?;
    }
    delete_qso(base, token, remote_id)?;
    state.bindings.remove(remote_id);
    state.pending.remove(&key);
    save_state(state_path, state)
}

fn finalize_absent_delete(
    state_path: &Path,
    state: &mut SyncState,
    remote_id: &str,
) -> Result<(), String> {
    state.bindings.remove(remote_id);
    state.pending.remove(&delete_op_key(remote_id));
    save_state(state_path, state)
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    y += if m <= 2 { 1 } else { 0 };
    (y, m as u32, d as u32)
}

fn date_time(unix: u64) -> (String, String) {
    let days = (unix / 86_400) as i64;
    let seconds = unix % 86_400;
    let (y, m, d) = civil_from_days(days);
    (
        format!("{y:04}{m:02}{d:02}"),
        format!(
            "{:02}{:02}{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        ),
    )
}

fn unix_from_date_time(date: &str, time: &str) -> Option<u64> {
    if date.len() != 8 || time.len() < 4 {
        return None;
    }
    let year = date[0..4].parse::<i64>().ok()?;
    let month = date[4..6].parse::<i64>().ok()?;
    let day = date[6..8].parse::<i64>().ok()?;
    let hour = time[0..2].parse::<u64>().ok()?;
    let minute = time[2..4].parse::<u64>().ok()?;
    let second = if time.len() >= 6 {
        time[4..6].parse::<u64>().ok()?
    } else {
        0
    };
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let y = year - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days)
        .ok()
        .map(|days| days * 86_400 + hour * 3_600 + minute * 60 + second)
}

fn local_common(r: &QsoRecord) -> CommonQso {
    let (qso_date, time_on) = date_time(r.when_unix);
    let time_off = r.time_off_unix.map(|v| date_time(v).1);
    let mut extra: BTreeMap<String, String> = r.extra.iter().cloned().collect();
    for (name, value) in [
        ("COUNTRY", r.country.clone()),
        ("DXCC", r.dxcc.map(|v| v.to_string())),
        ("OPERATOR", r.operator.clone()),
        ("STATION_CALLSIGN", r.station_callsign.clone()),
        ("MY_GRIDSQUARE", r.my_grid.clone()),
        ("MY_RIG", r.my_rig.clone()),
    ] {
        if let Some(value) = optional_text(&value) {
            extra.insert(name.into(), value);
        }
    }
    if r.qsl_rcvd.card {
        extra.insert("QSL_RCVD".into(), "Y".into());
    }
    if r.qsl_rcvd.lotw {
        extra.insert("LOTW_QSL_RCVD".into(), "Y".into());
    }
    if r.qsl_rcvd.eqsl {
        extra.insert("EQSL_QSL_RCVD".into(), "Y".into());
    }
    if r.qsl_sent.sent {
        extra.insert("QSL_SENT".into(), "Y".into());
    }
    if let Some(contest) = &r.contest {
        extra.insert("CONTEST_ID".into(), contest.contest_id.clone());
    }
    let their_program = r
        .ota
        .their_program
        .as_deref()
        .unwrap_or("")
        .to_ascii_uppercase();
    CommonQso {
        call: r.call.to_ascii_uppercase(),
        band: r.band.to_ascii_uppercase(),
        mode: r.mode.to_ascii_uppercase(),
        qso_date,
        time_on,
        time_off,
        freq_hz: (r.freq_mhz > 0.0).then(|| (r.freq_mhz * 1_000_000.0).round() as u64),
        freq_rx_hz: r
            .freq_rx_mhz
            .filter(|f| *f > 0.0)
            .map(|f| (f * 1_000_000.0).round() as u64),
        rst_sent: optional_text(&r.rst_sent),
        rst_rcvd: optional_text(&r.rst_rcvd),
        gridsquare: optional_text(&r.grid),
        name: optional_text(&r.name),
        qth: optional_text(&r.qth),
        comment: optional_text(&r.comment),
        notes: optional_text(&r.notes),
        tx_pwr: r.tx_power.map(|v| canonical_number(&v.to_string())),
        prop_mode: optional_text(&r.prop_mode),
        sat_name: optional_text(&r.sat_name),
        state: optional_text(&r.state),
        iota: optional_text(&r.ota.iota),
        sota_ref: (their_program == "SOTA")
            .then(|| r.ota.their_ref.clone())
            .flatten(),
        pota_ref: (their_program == "POTA")
            .then(|| r.ota.their_ref.clone())
            .flatten(),
        sig: optional_text(&r.ota.their_program),
        sig_info: optional_text(&r.ota.their_ref),
        srx: r
            .contest
            .as_ref()
            .and_then(|c| c.srx.map(|v| v.to_string())),
        stx: r
            .contest
            .as_ref()
            .and_then(|c| c.stx.map(|v| v.to_string())),
        srx_string: r
            .contest
            .as_ref()
            .and_then(|c| optional_text(&c.srx_string)),
        stx_string: r
            .contest
            .as_ref()
            .and_then(|c| optional_text(&c.stx_string)),
        extra,
    }
}

fn identity(q: &CommonQso) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        q.call, q.band, q.mode, q.qso_date, q.time_on
    )
}

fn load_state(path: &Path, endpoint: &str, station_id: &str) -> SyncState {
    let loaded = fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<SyncState>(&b).ok());
    loaded
        .filter(|s| {
            s.version == STATE_VERSION && s.endpoint == endpoint && s.station_id == station_id
        })
        .unwrap_or_else(|| SyncState::fresh(endpoint, station_id))
}

fn save_state(path: &Path, state: &SyncState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| "could not create the sync-state folder".to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes =
        serde_json::to_vec(state).map_err(|_| "could not encode Wavelog sync state".to_string())?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&tmp)
        .map_err(|_| "could not write Wavelog sync state".to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "could not save Wavelog sync state".to_string())?;
    fs::rename(tmp, path).map_err(|_| "could not install Wavelog sync state".to_string())?;
    save_status_state(path, state)
}

fn status_path(state_path: &Path) -> std::path::PathBuf {
    state_path.with_extension("status.json")
}

fn status_from_state(state: &SyncState) -> SyncStatus {
    let bound_local: HashSet<_> = state
        .bindings
        .values()
        .map(|binding| binding.local_id.as_str())
        .collect();
    let synced_local = bound_local.len();
    let synced_remote = state.bindings.len().min(state.last_remote_total);
    let local_total = state.last_local_total.max(synced_local);
    SyncStatus {
        local_total,
        remote_total: state.last_remote_total,
        synced_local,
        synced_remote,
        local_only: local_total.saturating_sub(synced_local),
        remote_only: state.last_remote_total.saturating_sub(synced_remote),
        last_sync_unix: state.last_sync_unix,
        interval_seconds: 300,
    }
}

fn save_status_state(state_path: &Path, state: &SyncState) -> Result<(), String> {
    let path = status_path(state_path);
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(&StatusState {
        version: STATE_VERSION,
        endpoint: state.endpoint.clone(),
        station_id: state.station_id.clone(),
        status: status_from_state(state),
    })
    .map_err(|_| "could not encode Wavelog sync status".to_string())?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&tmp)
        .map_err(|_| "could not write Wavelog sync status".to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "could not save Wavelog sync status".to_string())?;
    fs::rename(tmp, path).map_err(|_| "could not install Wavelog sync status".to_string())
}

fn collect_local(engine: &Mutex<Engine>) -> Result<HashMap<String, QsoRecord>, String> {
    let rows = engine_lock(engine).log_rows();
    let mut found = HashMap::new();
    rows.each_record(Duration::from_secs(30), &mut |r| {
        if let Some(id) = r.id {
            found.insert(id.to_string(), r.clone());
        }
        std::ops::ControlFlow::Continue(())
    })
    .map_err(|e| e.to_string())?;
    Ok(found)
}

fn delete_local(engine: &Mutex<Engine>, id: &str) -> Result<(), String> {
    let id = id
        .parse::<RecordId>()
        .map_err(|_| "the Wavelog binding contains an invalid local QSO id".to_string())?;
    let (result, durability) =
        tempo_app::logwrite::change_ops(engine, id, None, &[LogOp::Delete(id)], "wavelog_delete");
    result
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Wavelog deletion was refused: {e:?}"))?;
    durability
        .wait(tempo_app::logstore::DURABLE_WAIT)
        .map_err(|e| e.to_string())
}

pub fn status(
    _engine: &Mutex<Engine>,
    url: &str,
    station_id: &str,
    state_path: &Path,
) -> Result<SyncStatus, String> {
    let base = api_base(url)?;
    if let Some(saved) = fs::read(status_path(state_path))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<StatusState>(&bytes).ok())
        .filter(|saved| {
            saved.version == STATE_VERSION
                && saved.endpoint == base
                && saved.station_id == station_id
        })
    {
        return Ok(saved.status);
    }
    let state = load_state(state_path, &base, station_id);
    // One-time migration for an existing complete ledger. Every later render reads the compact
    // sidecar above instead of reparsing tens of megabytes of delete-safety bindings.
    let status = status_from_state(&state);
    save_status_state(state_path, &state)?;
    Ok(status)
}

fn apply_remote(
    engine: &Mutex<Engine>,
    local: &QsoRecord,
    remote: &CommonQso,
) -> Result<bool, String> {
    let Some(id) = local.id else { return Ok(false) };
    let mut current = local.clone();
    for _ in 0..4 {
        let key = QsoEdit::project(&current).key();
        let remote = remote.clone();
        let (result, durability) = tempo_app::logwrite::update_row(
            engine,
            id,
            &key,
            move |stored| {
                let mut row = stored.clone();
                row.call = remote.call.clone();
                row.band = remote.band.clone();
                row.mode = remote.mode.clone();
                row.freq_mhz = remote.freq_hz.unwrap_or_default() as f64 / 1_000_000.0;
                row.freq_rx_mhz = remote.freq_rx_hz.map(|v| v as f64 / 1_000_000.0);
                row.rst_sent = remote.rst_sent.clone();
                row.rst_rcvd = remote.rst_rcvd.clone();
                row.grid = remote.gridsquare.clone();
                row.name = remote.name.clone();
                row.qth = remote.qth.clone();
                row.comment = remote.comment.clone();
                row.notes = remote.notes.clone();
                row.tx_power = remote.tx_pwr.as_deref().and_then(|v| v.parse().ok());
                row.prop_mode = remote.prop_mode.clone();
                row.sat_name = remote.sat_name.clone();
                row.state = remote.state.clone();
                row.ota.iota = remote.iota.clone();
                if remote.sota_ref.is_some() {
                    row.ota.their_program = Some("SOTA".into());
                    row.ota.their_ref = remote.sota_ref.clone();
                } else if remote.pota_ref.is_some() {
                    row.ota.their_program = Some("POTA".into());
                    row.ota.their_ref = remote.pota_ref.clone();
                } else if remote.sig.is_some() || remote.sig_info.is_some() {
                    row.ota.their_program = remote.sig.clone();
                    row.ota.their_ref = remote.sig_info.clone();
                }
                let mut extra: BTreeMap<String, String> = row.extra.iter().cloned().collect();
                extra.extend(remote.extra.clone());
                row.extra = extra.into_iter().collect();
                if let Some(when) = unix_from_date_time(&remote.qso_date, &remote.time_on) {
                    row.when_unix = when;
                    row.time_known = true;
                }
                row.time_off_unix = remote
                    .time_off
                    .as_deref()
                    .and_then(|time| unix_from_date_time(&remote.qso_date, time));
                row
            },
            |_, _| (),
        );
        match result.map_err(|e| e.to_string())? {
            Ok(Some(())) => {
                durability
                    .wait(tempo_app::logstore::DURABLE_WAIT)
                    .map_err(|e| e.to_string())?;
                return Ok(true);
            }
            Ok(None) => return Ok(false),
            Err(tempo_app::station::RowRefusal::Changed(now)) => {
                // Connector stamps and confirmation workers can legitimately change a row
                // between the whole-log snapshot and this edit. Retry against that exact newer
                // version instead of failing the entire 60k-contact reconciliation.
                current = (*now).clone();
            }
            Err(tempo_app::station::RowRefusal::Gone) => return Ok(false),
            Err(tempo_app::station::RowRefusal::Busy) => {
                return Err("the local QSO kept changing while Wavelog was updating it".into())
            }
        }
    }
    Err("the local QSO changed too many times while Wavelog was updating it".into())
}

fn append_remote_adif(remote: &CommonQso, adif: &mut String) {
    fn tag(name: &str, value: &str, out: &mut String) {
        out.push_str(&format!("<{name}:{}>{value}", value.len()));
    }
    for (name, value) in [
        ("CALL", Some(remote.call.as_str())),
        ("BAND", Some(remote.band.as_str())),
        ("MODE", Some(remote.mode.as_str())),
        ("QSO_DATE", Some(remote.qso_date.as_str())),
        ("TIME_ON", Some(remote.time_on.as_str())),
        ("TIME_OFF", remote.time_off.as_deref()),
        ("GRIDSQUARE", remote.gridsquare.as_deref()),
        ("RST_SENT", remote.rst_sent.as_deref()),
        ("RST_RCVD", remote.rst_rcvd.as_deref()),
        ("NAME", remote.name.as_deref()),
        ("QTH", remote.qth.as_deref()),
        ("COMMENT", remote.comment.as_deref()),
        ("NOTES", remote.notes.as_deref()),
        ("TX_PWR", remote.tx_pwr.as_deref()),
        ("PROP_MODE", remote.prop_mode.as_deref()),
        ("SAT_NAME", remote.sat_name.as_deref()),
        ("STATE", remote.state.as_deref()),
        ("IOTA", remote.iota.as_deref()),
        ("SOTA_REF", remote.sota_ref.as_deref()),
        ("POTA_REF", remote.pota_ref.as_deref()),
        ("SIG", remote.sig.as_deref()),
        ("SIG_INFO", remote.sig_info.as_deref()),
        ("SRX", remote.srx.as_deref()),
        ("STX", remote.stx.as_deref()),
        ("SRX_STRING", remote.srx_string.as_deref()),
        ("STX_STRING", remote.stx_string.as_deref()),
    ] {
        if let Some(value) = value {
            tag(name, value, adif);
        }
    }
    if let Some(hz) = remote.freq_hz {
        tag("FREQ", &format!("{:.6}", hz as f64 / 1_000_000.0), adif);
    }
    if let Some(hz) = remote.freq_rx_hz {
        tag("FREQ_RX", &format!("{:.6}", hz as f64 / 1_000_000.0), adif);
    }
    for (name, value) in &remote.extra {
        tag(name, value, adif);
    }
    adif.push_str("<EOR>");
}

fn import_remote_batch(engine: &Mutex<Engine>, remotes: &[RemoteQso]) -> Result<(), String> {
    let mut adif = String::with_capacity(remotes.len().saturating_mul(256));
    for remote in remotes {
        append_remote_adif(&remote.qso, &mut adif);
    }
    let (result, durability) = tempo_app::logwrite::import_adif(engine, &adif);
    result?;
    durability
        .wait(tempo_app::logstore::DURABLE_WAIT)
        .map_err(|e| e.to_string())
}

pub fn upload_one(
    url: &str,
    token: &str,
    station_id: &str,
    record: &QsoRecord,
    state_path: &Path,
) -> Result<String, String> {
    let _only_one_sync = SYNC_LOCK
        .lock()
        .map_err(|_| "the Wavelog sync lock is unavailable".to_string())?;
    if !token.trim().starts_with("wl2_") {
        return Err("the stored key is not a Wavelog API v2 token".into());
    }
    if station_id.trim().parse::<u64>().is_err() {
        return Err("the Wavelog station profile id must be a number".into());
    }
    let base = api_base(url)?;
    let local_id = record
        .id
        .ok_or_else(|| "the new local QSO has no durable id yet".to_string())?
        .to_string();
    let mut state = load_state(state_path, &base, station_id);
    if let Some((remote_id, _)) = state
        .bindings
        .iter()
        .find(|(_, binding)| binding.local_id == local_id)
    {
        return Ok(remote_id.clone());
    }
    let qso = local_common(record);
    let pending_exists = state.pending.contains_key(&create_op_key(&local_id));
    let remote = if pending_exists {
        fetch_qsos(&base, token, station_id)?
    } else {
        Vec::new()
    };
    let (remote_id, created) = create_with_journal(
        &base, token, station_id, state_path, &mut state, &local_id, &qso, &remote,
    )?;
    if created {
        state.last_local_total = state.last_local_total.saturating_add(1);
        state.last_remote_total = state.last_remote_total.saturating_add(1);
        save_state(state_path, &state)?;
    }
    Ok(remote_id)
}

pub fn sync(
    engine: &Mutex<Engine>,
    url: &str,
    token: &str,
    station_id: &str,
    state_path: &Path,
) -> Result<SyncReport, String> {
    let _only_one_sync = SYNC_LOCK
        .lock()
        .map_err(|_| "the Wavelog sync lock is unavailable".to_string())?;
    if !token.trim().starts_with("wl2_") {
        return Err("the stored key is not a Wavelog API v2 token".into());
    }
    if station_id.trim().parse::<u64>().is_err() {
        return Err("the Wavelog station profile id must be a number".into());
    }
    let base = api_base(url)?;
    let remote = fetch_qsos(&base, token, station_id)?;
    let mut state = load_state(state_path, &base, station_id);
    let mut local = collect_local(engine)?;
    let mut report = SyncReport {
        downloaded: 0,
        uploaded: 0,
        local_updates: 0,
        remote_updates: 0,
        local_deletes: 0,
        remote_deletes: 0,
        matched: 0,
        conflicts_resolved_from_wavelog: 0,
        synced_local: 0,
        synced_remote: 0,
        local_only: 0,
        remote_only: 0,
        remote_total: remote.len(),
        local_total: local.len(),
    };
    // Resolve durable create receipts before identity matching. A prior POST may have succeeded
    // while the process died before its binding save; the private ADIF marker makes that remote
    // row unambiguous and prevents a second POST after restart.
    let pending_creates: Vec<_> = state
        .pending
        .values()
        .filter_map(|operation| match operation {
            PendingOperation::Create { local_id, qso } => Some((local_id.clone(), qso.clone())),
            PendingOperation::DeleteRemote { .. } => None,
        })
        .collect();
    for (local_id, qso) in pending_creates {
        if !local.contains_key(&local_id) {
            state.pending.remove(&create_op_key(&local_id));
            save_state(state_path, &state)?;
        } else if let Some(found) = remote
            .iter()
            .find(|row| row.create_receipt.as_deref() == Some(local_id.as_str()))
        {
            finalize_create(state_path, &mut state, found.id.clone(), local_id, qso)?;
        }
    }

    let remote_ids: HashSet<_> = remote.iter().map(|r| r.id.clone()).collect();
    let local_ids: HashSet<_> = local.keys().cloned().collect();
    let mut deleted_remote_ids = HashSet::new();
    let pending_deletes: Vec<_> = state
        .pending
        .values()
        .filter_map(|operation| match operation {
            PendingOperation::DeleteRemote {
                remote_id,
                local_id,
            } => Some((remote_id.clone(), local_id.clone())),
            PendingOperation::Create { .. } => None,
        })
        .collect();
    for (remote_id, local_id) in pending_deletes {
        if !remote_ids.contains(&remote_id) {
            finalize_absent_delete(state_path, &mut state, &remote_id)?;
        } else if !local_ids.contains(&local_id) {
            delete_remote_with_journal(
                &base, token, state_path, &mut state, &remote_id, &local_id,
            )?;
            deleted_remote_ids.insert(remote_id);
            report.remote_deletes += 1;
        } else {
            // The local row exists again, so the original deletion intent is no longer true.
            state.pending.remove(&delete_op_key(&remote_id));
            save_state(state_path, &state)?;
        }
    }

    // Only a persisted binding can turn absence into deletion. Check the entire plan before
    // mutating either side: a wrong station filter or a truncated response must fail closed.
    let bindings_before_deletion_reconcile = state.bindings.len();
    let locally_deleted: Vec<_> = state
        .bindings
        .iter()
        .filter(|(remote_id, binding)| {
            remote_ids.contains(*remote_id) && !local_ids.contains(&binding.local_id)
        })
        .map(|(remote_id, binding)| (remote_id.clone(), binding.local_id.clone()))
        .collect();
    let remotely_deleted: HashSet<_> = state
        .bindings
        .iter()
        .filter(|(remote_id, binding)| {
            !remote_ids.contains(*remote_id)
                && local_ids.contains(&binding.local_id)
                // Several Wavelog duplicates can legitimately bind to one local ADIF contact.
                // Removing one duplicate removes only that binding; the local contact is deleted
                // only when its final remotely-present binding disappeared.
                && !state.bindings.iter().any(|(other_id, other)| {
                    other.local_id == binding.local_id && remote_ids.contains(other_id)
                })
        })
        .map(|(_, binding)| binding.local_id.clone())
        .collect();
    let deletion_count = locally_deleted.len() + remotely_deleted.len();
    if deletion_count > MAX_DELETIONS_PER_SYNC {
        return Err(format!(
            "Wavelog sync stopped before deletion: {deletion_count} bound contacts disappeared (safety limit {MAX_DELETIONS_PER_SYNC})"
        ));
    }

    for (remote_id, local_id) in locally_deleted {
        if deleted_remote_ids.contains(&remote_id) {
            continue;
        }
        delete_remote_with_journal(&base, token, state_path, &mut state, &remote_id, &local_id)?;
        deleted_remote_ids.insert(remote_id);
        report.remote_deletes += 1;
    }
    for local_id in remotely_deleted {
        delete_local(engine, &local_id)?;
        local.remove(&local_id);
        report.local_deletes += 1;
    }
    // A remote id absent from the fetched complete station log is stale bookkeeping now. Its
    // final binding propagated the local deletion above; a duplicate binding was simply removed.
    state
        .bindings
        .retain(|remote_id, _| remote_ids.contains(remote_id));
    // A successful DELETE, or cleanup after both copies were already removed, must not be
    // forgotten because an unrelated edit later in this large reconciliation fails. Persist the
    // reduced binding set before moving on to additions/edits.
    if state.bindings.len() != bindings_before_deletion_reconcile {
        save_state(state_path, &state)?;
    }

    let mut bound_local: HashSet<_> = state
        .bindings
        .values()
        .map(|b| b.local_id.clone())
        .collect();
    let mut by_identity: HashMap<String, Vec<String>> = HashMap::new();
    for (id, row) in &local {
        by_identity
            .entry(identity(&local_common(row)))
            .or_default()
            .push(id.clone());
    }

    let mut pending_imports = Vec::new();
    for rq in &remote {
        if deleted_remote_ids.contains(&rq.id) {
            continue;
        }
        if let Some(binding) = state.bindings.get(&rq.id).cloned() {
            if let Some(row) = local.get(&binding.local_id).cloned() {
                let lq = local_common(&row);
                let local_changed = lq != binding.last_local;
                let remote_changed = rq.qso != binding.last_remote;
                let (last_local, last_remote) = if remote_changed {
                    if local_changed && lq != rq.qso {
                        report.conflicts_resolved_from_wavelog += 1;
                    }
                    if lq != rq.qso && apply_remote(engine, &row, &rq.qso)? {
                        report.local_updates += 1;
                    }
                    (rq.qso.clone(), rq.qso.clone())
                } else if local_changed {
                    patch_qso(&base, token, &rq.id, station_id, &lq)?;
                    report.remote_updates += 1;
                    (lq.clone(), lq)
                } else {
                    (binding.last_local, binding.last_remote)
                };
                state.bindings.insert(
                    rq.id.clone(),
                    Binding {
                        local_id: binding.local_id,
                        last_local,
                        last_remote,
                    },
                );
                continue;
            }
        }
        let candidate = by_identity.get(&identity(&rq.qso)).and_then(|ids| {
            ids.iter()
                .find(|id| !bound_local.contains(*id))
                .or_else(|| ids.first())
                .cloned()
        });
        let local_id = if let Some(id) = candidate {
            report.matched += 1;
            bound_local.insert(id.clone());
            id
        } else {
            pending_imports.push(rq.clone());
            continue;
        };
        state.bindings.insert(
            rq.id.clone(),
            Binding {
                last_local: local
                    .get(&local_id)
                    .map(local_common)
                    .unwrap_or_else(|| rq.qso.clone()),
                local_id,
                last_remote: rq.qso.clone(),
            },
        );
    }

    // A first sync can contain tens of thousands of QSOs. Import them as one ADIF transaction,
    // then scan the local log once; importing and rescanning once per QSO is quadratic and makes
    // a real-world Wavelog history effectively impossible to initialise.
    if !pending_imports.is_empty() {
        import_remote_batch(engine, &pending_imports)?;
        report.downloaded += pending_imports.len();
        local = collect_local(engine)?;
        let mut imported_by_identity: HashMap<String, (String, CommonQso)> = HashMap::new();
        for (id, row) in &local {
            imported_by_identity
                .entry(identity(&local_common(row)))
                .or_insert_with(|| (id.clone(), local_common(row)));
        }
        for rq in pending_imports {
            let (local_id, last_local) = imported_by_identity
                .get(&identity(&rq.qso))
                .cloned()
                .ok_or_else(|| {
                    "Wavelog QSOs were imported but one could not be bound to its local row"
                        .to_string()
                })?;
            state.bindings.insert(
                rq.id,
                Binding {
                    local_id,
                    last_local,
                    last_remote: rq.qso,
                },
            );
        }
    }

    local = collect_local(engine)?;
    let now_bound: HashSet<_> = state
        .bindings
        .values()
        .map(|b| b.local_id.clone())
        .collect();
    for (id, row) in &local {
        if now_bound.contains(id) {
            continue;
        }
        let qso = local_common(row);
        let (_, created) = create_with_journal(
            &base, token, station_id, state_path, &mut state, id, &qso, &remote,
        )?;
        report.uploaded += usize::from(created);
    }
    report.local_total = local.len();
    report.remote_total = report
        .remote_total
        .saturating_sub(report.remote_deletes)
        .saturating_add(report.uploaded);
    let synced_local: HashSet<_> = state
        .bindings
        .values()
        .filter(|binding| local.contains_key(&binding.local_id))
        .map(|binding| binding.local_id.as_str())
        .collect();
    report.synced_local = synced_local.len();
    report.synced_remote = state.bindings.len().min(report.remote_total);
    report.local_only = report.local_total.saturating_sub(report.synced_local);
    report.remote_only = report.remote_total.saturating_sub(report.synced_remote);
    state.last_sync_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    state.last_remote_total = report.remote_total;
    state.last_local_total = report.local_total;
    save_state(state_path, &state)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state_path(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "rd-nexus-wavelog-{label}-{}-{nonce}.json",
            std::process::id()
        ))
    }

    fn remove_test_state(path: &Path) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(status_path(path));
    }

    #[test]
    fn normalizes_instance_and_v2_urls() {
        assert_eq!(
            api_base("https://log.example").unwrap(),
            "https://log.example/index.php/api/v2"
        );
        assert_eq!(
            api_base("https://log.example/index.php/api/v2/").unwrap(),
            "https://log.example/index.php/api/v2"
        );
    }

    #[test]
    fn parses_database_style_qso_fields() {
        let q = remote_from_value(json!({"COL_PRIMARY_KEY": 42, "COL_CALL":"vk9xx", "COL_BAND":"20m", "COL_MODE":"FT8", "COL_TIME_ON":"2026-09-27 01:02:03", "COL_FREQ":"14.074"})).unwrap();
        assert_eq!(q.id, "42");
        assert_eq!(q.qso.call, "VK9XX");
        assert_eq!(q.qso.qso_date, "20260927");
        assert_eq!(q.qso.time_on, "010203");
        assert_eq!(q.qso.freq_hz, Some(14_074_000));
    }

    #[test]
    fn parses_api_v2_combined_qso_timestamp() {
        let q = remote_from_value(json!({
            "id": 4886,
            "station_id": 1,
            "call": "N9EAT",
            "band": "20m",
            "mode": "SSB",
            "freq": 14075000,
            "qso_date": "2026-06-16 17:06:00"
        }))
        .unwrap();
        assert_eq!(q.qso.qso_date, "20260616");
        assert_eq!(q.qso.time_on, "170600");
    }

    #[test]
    fn older_binding_rows_gain_new_adif_fields_without_losing_the_ledger() {
        let q: CommonQso = serde_json::from_value(json!({
            "call": "K1ABC",
            "band": "20M",
            "mode": "FT4",
            "qso_date": "20260924",
            "time_on": "231137"
        }))
        .unwrap();
        assert_eq!(q.call, "K1ABC");
        assert_eq!(q.state, None);
        assert!(q.extra.is_empty());
    }

    #[test]
    fn status_reads_the_compact_sidecar_without_reopening_the_binding_ledger() {
        let path = std::env::temp_dir().join(format!(
            "rd-nexus-wavelog-status-{}-{}.json",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let endpoint = "https://log.example/index.php/api/v2";
        let mut state = SyncState::fresh(endpoint, "1");
        state.last_sync_unix = 42;
        state.last_local_total = 1;
        state.last_remote_total = 1;
        state.bindings.insert(
            "99".into(),
            Binding {
                local_id: "local-1".into(),
                last_local: CommonQso::default(),
                last_remote: CommonQso::default(),
            },
        );
        save_state(&path, &state).unwrap();
        fs::write(&path, b"not the full ledger anymore").unwrap();

        let summary = status(
            &Mutex::new(Engine::new("N0CALL", "AA00", 0)),
            endpoint,
            "1",
            &path,
        )
        .unwrap();
        assert_eq!(summary.local_total, 1);
        assert_eq!(summary.remote_total, 1);
        assert_eq!(summary.synced_local, 1);
        assert_eq!(summary.last_sync_unix, 42);

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(status_path(&path));
    }

    #[test]
    fn restart_recovers_successful_create_without_posting_twice() {
        let path = test_state_path("create-recovery");
        let endpoint = "https://log.example/index.php/api/v2";
        let local_id = "local-create-1";
        let qso = CommonQso {
            call: "K1ABC".into(),
            band: "20M".into(),
            mode: "FT4".into(),
            qso_date: "20260924".into(),
            time_on: "231137".into(),
            ..CommonQso::default()
        };
        let mut before_crash = SyncState::fresh(endpoint, "1");
        before_crash.pending.insert(
            create_op_key(local_id),
            PendingOperation::Create {
                local_id: local_id.into(),
                qso: qso.clone(),
            },
        );
        save_state(&path, &before_crash).unwrap();

        // Model POST success followed by a crash before the final local save: Wavelog has the
        // receipt-tagged row, while durable local state still contains only the pending intent.
        let remote = vec![RemoteQso {
            id: "remote-99".into(),
            qso: qso.clone(),
            create_receipt: Some(local_id.into()),
        }];
        let mut after_restart = load_state(&path, endpoint, "1");
        let (remote_id, created) = create_with_journal(
            endpoint,
            "unused-because-receipt-is-found",
            "1",
            &path,
            &mut after_restart,
            local_id,
            &qso,
            &remote,
        )
        .unwrap();

        assert_eq!(remote_id, "remote-99");
        assert!(!created, "receipt recovery must not issue a second POST");
        assert!(after_restart.pending.is_empty());
        assert_eq!(after_restart.bindings["remote-99"].local_id, local_id);
        let durable = load_state(&path, endpoint, "1");
        assert!(durable.pending.is_empty());
        assert_eq!(durable.bindings["remote-99"].local_id, local_id);
        remove_test_state(&path);
    }

    #[test]
    fn restart_finalizes_successful_delete_when_remote_is_already_absent() {
        let path = test_state_path("delete-recovery");
        let endpoint = "https://log.example/index.php/api/v2";
        let remote_id = "remote-100";
        let local_id = "local-delete-1";
        let qso = CommonQso::default();
        let mut before_crash = SyncState::fresh(endpoint, "1");
        before_crash.bindings.insert(
            remote_id.into(),
            Binding {
                local_id: local_id.into(),
                last_local: qso.clone(),
                last_remote: qso,
            },
        );
        before_crash.pending.insert(
            delete_op_key(remote_id),
            PendingOperation::DeleteRemote {
                remote_id: remote_id.into(),
                local_id: local_id.into(),
            },
        );
        save_state(&path, &before_crash).unwrap();

        // Model DELETE success followed by a crash before the final local save. A complete
        // remote snapshot no longer contains the id, so restart can finalize locally only.
        let mut after_restart = load_state(&path, endpoint, "1");
        finalize_absent_delete(&path, &mut after_restart, remote_id).unwrap();

        assert!(after_restart.pending.is_empty());
        assert!(!after_restart.bindings.contains_key(remote_id));
        let durable = load_state(&path, endpoint, "1");
        assert!(durable.pending.is_empty());
        assert!(!durable.bindings.contains_key(remote_id));
        remove_test_state(&path);
    }

    #[test]
    fn unix_conversion_is_utc() {
        assert_eq!(date_time(0), ("19700101".into(), "000000".into()));
        assert_eq!(unix_from_date_time("19700101", "000000"), Some(0));
        assert_eq!(
            unix_from_date_time("20260927", "010203").map(date_time),
            Some(("20260927".into(), "010203".into()))
        );
        assert_eq!(date_time(1_757_000_000).0.len(), 8);
    }
}
