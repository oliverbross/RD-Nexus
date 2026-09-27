//! Tauri adapter for the portable DX Chaser decision engine.
//!
//! The adapter is intentionally narrow: Nexus remains authoritative for decoded messages,
//! cluster/RBN spots, worked-state, radio control, identity and the TX latch. This module
//! translates those facts and executes decisions only through Engine verbs.

use super::{
    engine_lock, now_unix, settings_path, SharedDxChaser, SharedEngine, SharedLivePaths,
    SharedRegionPaths, SharedSpots,
};
use dx_chaser::{Action, Chaser, Decode, Settings, Snapshot, Spot};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::Write;
use std::ops::ControlFlow;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::State;
use tempo_core::logbook::sqlite::{Narrow, Order, Scope};

const KEYRING_SERVICE: &str = "Nexus DX Chaser Twilio";
const KEYRING_BUNDLE: &str = "credentials-v1";
const DXCC_MATRIX_BANDS: [&str; 9] = [
    "160m", "80m", "40m", "30m", "20m", "17m", "15m", "12m", "10m",
];
const DXCC_MATRIX_SPOT_AGE: Duration = Duration::from_secs(5 * 60);
/// One owner from normalization through durable install and runtime application.
///
/// Atomic file replacement prevents torn JSON, but it does not impose an order between two
/// callers: without this transaction A can save, B can save and apply, then A can apply, leaving
/// disk B/runtime A. This lock is deliberately process-wide because the settings file is
/// process-wide even if a test or a future window constructs another managed state handle.
static CONFIGURE_TRANSACTION: Mutex<()> = Mutex::new(());
const TWILIO_CLIENT_FAILURE: &str = "Twilio client could not be initialized";
const TWILIO_AMBIGUOUS_FAILURE: &str =
    "Twilio delivery outcome is unknown; the message was not retried to prevent a duplicate";
const DXCC_MATRIX_LOG_COLUMNS: Narrow = Narrow {
    columns: &["call", "country", "band"],
    uploads: false,
};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DxccMatrixSpot {
    call: String,
    band: String,
    freq_mhz: f64,
    source: String,
    spotter: String,
    mode: Option<String>,
    received_unix: u64,
    age_seconds: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DxccMatrixEntity {
    name: String,
    prefix: String,
    rank: Option<u16>,
    worked: Vec<bool>,
    active_spots: Vec<DxccMatrixSpot>,
    worked_any: bool,
    worked_all: bool,
    has_active_spot: bool,
    search_text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DxccMatrixFilters {
    total_entities: usize,
    worked_entities: usize,
    unworked_entities: usize,
    spotted_unworked_entities: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DxccMatrix {
    bands: Vec<String>,
    entities: Vec<DxccMatrixEntity>,
    filters: DxccMatrixFilters,
    generated_at: u64,
}

#[derive(Debug, Clone)]
struct MatrixSpotFact {
    entity: String,
    spot: DxccMatrixSpot,
}

#[derive(Debug, Clone)]
struct ActiveAttempt {
    call: String,
    band: String,
    started_at: u64,
    baseline_log_tick: u32,
}

#[derive(Debug, Clone)]
struct LoggedQsoFact {
    signature: String,
    call: String,
    entity: String,
    band: String,
    mode: String,
    grid: Option<String>,
    when_unix: u64,
}

#[derive(Default)]
pub(crate) struct ChaserState {
    engine: Chaser,
    loaded: bool,
    last_decode_fingerprint: Vec<String>,
    last_slot: Option<u64>,
    last_qso_active: bool,
    last_log_tick: u32,
    log_baselined: bool,
    log_high_water: u64,
    active_attempt: Option<ActiveAttempt>,
    observed_qsos: VecDeque<String>,
    arm_halt_generation: Option<u64>,
    sms_outbox: dx_chaser::SmsOutbox,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TwilioCredentials {
    account_sid: String,
    auth_token: String,
    from_number: String,
    to_number: String,
}

impl TwilioCredentials {
    fn complete(&self) -> bool {
        [
            &self.account_sid,
            &self.auth_token,
            &self.from_number,
            &self.to_number,
        ]
        .iter()
        .all(|value| !value.trim().is_empty())
    }

    fn normalized(mut self) -> Self {
        self.account_sid = self.account_sid.trim().to_string();
        self.auth_token = self.auth_token.trim().to_string();
        self.from_number = normalize_e164(&self.from_number);
        self.to_number = normalize_e164(&self.to_number);
        self
    }
}

fn normalize_e164(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() || value.starts_with('+') {
        value.to_string()
    } else {
        format!("+{value}")
    }
}

fn config_path() -> std::path::PathBuf {
    settings_path().with_file_name("dx-chaser.json")
}

fn history_path() -> std::path::PathBuf {
    settings_path().with_file_name("dx-chaser-history.json")
}

fn load_json<T: DeserializeOwned>(path: &Path, label: &str) -> Option<T> {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(value) => Some(value),
            Err(error) => {
                tempo_core::applog::warn(
                    "dx-chaser",
                    &format!("ignored corrupt or truncated {label}: {error}"),
                );
                None
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            tempo_core::applog::warn("dx-chaser", &format!("could not read {label}: {error}"));
            None
        }
    }
}

#[cfg(not(windows))]
fn install_atomic(tmp: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(tmp, destination)
}

#[cfg(windows)]
fn install_atomic(tmp: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVE_FILE_REPLACE_EXISTING, MOVE_FILE_WRITE_THROUGH,
    };

    let source: Vec<u16> = tmp.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both buffers are owned, NUL-terminated UTF-16 paths and remain alive for the call.
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVE_FILE_REPLACE_EXISTING | MOVE_FILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

static SAVE_NONCE: AtomicU64 = AtomicU64::new(0);

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    write_json_atomic_with(path, value, install_atomic)
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn write_json_atomic_with<T, F>(path: &Path, value: &T, install: F) -> Result<(), String>
where
    T: Serialize,
    F: FnOnce(&Path, &Path) -> std::io::Result<()>,
{
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let mut scratch = None;
    for _ in 0..100 {
        let nonce = SAVE_NONCE.fetch_add(1, Ordering::Relaxed);
        let candidate = path.with_extension(format!("{}.{}.tmp", std::process::id(), nonce));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                scratch = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    let (tmp, mut file) =
        scratch.ok_or_else(|| "could not allocate atomic save file".to_string())?;
    let result = (|| -> Result<(), String> {
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        install(&tmp, path).map_err(|e| e.to_string())?;
        sync_parent_directory(path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn load_settings() -> Settings {
    load_json::<Settings>(&config_path(), "DX Chaser settings")
        .unwrap_or_default()
        .normalized()
}

fn save_settings(settings: &Settings) -> Result<(), String> {
    write_json_atomic(&config_path(), settings)
}

fn load_history() -> Option<dx_chaser::PersistentState> {
    load_json(&history_path(), "DX Chaser history")
}

fn save_history(engine: &Chaser) -> Result<(), String> {
    let path = history_path();
    write_json_atomic(&path, &engine.persistent_state())
}

fn chaser_lock(state: &SharedDxChaser) -> MutexGuard<'_, ChaserState> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn band_for_mhz(mhz: f64) -> String {
    propagation::Band::from_mhz(mhz)
        .map(|band| band.label().to_string())
        .unwrap_or_default()
}

fn is_chaser_digital_mode(mode: Option<&str>) -> bool {
    matches!(mode, Some(mode) if mode.eq_ignore_ascii_case("FT8") || mode.eq_ignore_ascii_case("FT4"))
}

fn is_chaser_cluster_spot(mode: Option<&str>, freq_mhz: f64, rbn: bool) -> bool {
    if mode.is_some() {
        return is_chaser_digital_mode(mode);
    }
    !rbn && dx_chaser::STANDARD_FT8_MHZ
        .iter()
        .any(|(_, dial)| (freq_mhz - dial).abs() <= 0.025)
}

fn attempt_matches_qso(attempt: &ActiveAttempt, qso_call: Option<&str>) -> bool {
    qso_call.is_some_and(|call| tempo_core::message::same_call(call, &attempt.call))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NexusQueuePresence {
    Present,
    BlockedByUnrelatedQso,
    Missing,
}

fn nexus_queue_presence(
    attempt: &ActiveAttempt,
    qso_call: Option<&str>,
    qso_active: bool,
) -> NexusQueuePresence {
    if attempt_matches_qso(attempt, qso_call) {
        NexusQueuePresence::Present
    } else if qso_active {
        NexusQueuePresence::BlockedByUnrelatedQso
    } else {
        NexusQueuePresence::Missing
    }
}

fn dxcc_primary_prefixes() -> HashMap<String, String> {
    include_str!("../../crates/propagation/data/cty.dat")
        .lines()
        .filter(|line| {
            line.as_bytes()
                .first()
                .is_some_and(|byte| !matches!(byte, b' ' | b'\t'))
        })
        .filter_map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?.trim();
            let prefix = fields.nth(6)?.trim().trim_start_matches('*');
            (!name.is_empty() && !prefix.is_empty())
                .then(|| (name.to_string(), prefix.to_ascii_uppercase()))
        })
        .collect()
}

fn matrix_entity_for_call(call: &str, names: &HashSet<&str>) -> Option<String> {
    propagation::dxcc::resolve(call)
        .filter(|info| info.is_dxcc && names.contains(info.entity))
        .map(|info| info.entity.to_string())
}

fn read_matrix_worked(engine: &SharedEngine) -> Result<HashSet<(String, String)>, String> {
    // Capture the canonical store handle while the Engine is held, then perform the potentially
    // blocking whole-log pass after releasing it. This is the same C14 boundary as LogQueries.
    let rows = { engine_lock(engine).log_rows() };
    let names: HashSet<&str> = propagation::dxcc::dxcc_entity_names().into_iter().collect();
    let mut worked = HashSet::new();
    rows.each(
        DXCC_MATRIX_LOG_COLUMNS,
        Scope::All,
        Order::Log,
        &mut |record| {
            if DXCC_MATRIX_BANDS.contains(&record.band.as_str()) {
                let entity = matrix_entity_for_call(&record.call, &names).or_else(|| {
                    record
                        .country
                        .as_deref()
                        .filter(|country| names.contains(*country))
                        .map(str::to_string)
                });
                if let Some(entity) = entity {
                    worked.insert((entity, record.band.clone()));
                }
            }
            ControlFlow::Continue(())
        },
    )
    .map_err(|error| format!("the DXCC matrix could not read the logbook: {error}"))?;
    Ok(worked)
}

fn read_matrix_spots(spots: &SharedSpots, now: u64) -> Vec<MatrixSpotFact> {
    let recent = spots
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .recent_within(Instant::now(), DXCC_MATRIX_SPOT_AGE);
    let names: HashSet<&str> = propagation::dxcc::dxcc_entity_names().into_iter().collect();
    recent
        .into_iter()
        .filter_map(|cluster| {
            let mode = cluster.skimmer_mode().map(str::to_string);
            if !is_chaser_cluster_spot(mode.as_deref(), cluster.freq_mhz(), cluster.rbn) {
                return None;
            }
            let freq_mhz = cluster.freq_mhz();
            let band = band_for_mhz(freq_mhz);
            if !DXCC_MATRIX_BANDS.contains(&band.as_str()) {
                return None;
            }
            let entity = matrix_entity_for_call(&cluster.dx_call, &names)?;
            Some(MatrixSpotFact {
                entity,
                spot: DxccMatrixSpot {
                    call: cluster.dx_call,
                    band,
                    freq_mhz,
                    source: if cluster.rbn { "RBN" } else { "DX cluster" }.into(),
                    spotter: cluster.spotter,
                    mode,
                    received_unix: cluster.received_unix,
                    age_seconds: now.saturating_sub(cluster.received_unix),
                },
            })
        })
        .collect()
}

fn read_matrix_pskr_spots(
    live_paths: &SharedLivePaths,
    region_paths: &SharedRegionPaths,
    mycall: &str,
    now: u64,
) -> Vec<MatrixSpotFact> {
    let own = live_paths
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .recent(now as i64, DXCC_MATRIX_SPOT_AGE.as_secs() as i64);
    let regional = region_paths
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .recent(now as i64, DXCC_MATRIX_SPOT_AGE.as_secs() as i64);
    let names: HashSet<&str> = propagation::dxcc::dxcc_entity_names().into_iter().collect();
    own.into_iter()
        .filter(|path| path.side(mycall) == propagation::Side::IHeard)
        .chain(regional)
        .filter(|path| {
            matches!(path.mode.as_deref(), Some(mode) if mode.eq_ignore_ascii_case("FT8") || mode.eq_ignore_ascii_case("FT4"))
        })
        .filter_map(|path| {
            let band = path.band.label().to_string();
            if !DXCC_MATRIX_BANDS.contains(&band.as_str()) {
                return None;
            }
            let entity = matrix_entity_for_call(&path.tx_call, &names)?;
            Some(MatrixSpotFact {
                entity,
                spot: DxccMatrixSpot {
                    call: path.tx_call,
                    band,
                    freq_mhz: path
                        .freq_mhz
                        .unwrap_or_else(|| propagation::band_digital_mhz(path.band)),
                    source: "PSK Reporter".into(),
                    spotter: path.rx_call,
                    mode: path.mode,
                    received_unix: path.time.max(0) as u64,
                    age_seconds: now.saturating_sub(path.time.max(0) as u64),
                },
            })
        })
        .collect()
}

fn build_dxcc_matrix(
    worked: &HashSet<(String, String)>,
    spot_facts: Vec<MatrixSpotFact>,
    now: u64,
) -> DxccMatrix {
    let prefixes = dxcc_primary_prefixes();
    let mut spots_by_entity: BTreeMap<String, Vec<DxccMatrixSpot>> = BTreeMap::new();
    for fact in spot_facts {
        if !worked.contains(&(fact.entity.clone(), fact.spot.band.clone())) {
            spots_by_entity
                .entry(fact.entity)
                .or_default()
                .push(fact.spot);
        }
    }
    for spots in spots_by_entity.values_mut() {
        spots.sort_by(|a, b| {
            let band_a = DXCC_MATRIX_BANDS
                .iter()
                .position(|band| *band == a.band)
                .unwrap_or(usize::MAX);
            let band_b = DXCC_MATRIX_BANDS
                .iter()
                .position(|band| *band == b.band)
                .unwrap_or(usize::MAX);
            band_a
                .cmp(&band_b)
                .then_with(|| a.call.cmp(&b.call))
                .then_with(|| a.freq_mhz.total_cmp(&b.freq_mhz))
        });
    }

    let mut entities: Vec<DxccMatrixEntity> = propagation::dxcc::dxcc_entity_names()
        .into_iter()
        .map(|name| {
            let prefix = prefixes.get(name).cloned().unwrap_or_default();
            let rank = dx_chaser::clublog_rank(name);
            let worked: Vec<bool> = DXCC_MATRIX_BANDS
                .iter()
                .map(|band| worked.contains(&(name.to_string(), (*band).to_string())))
                .collect();
            let active_spots = spots_by_entity.remove(name).unwrap_or_default();
            let worked_any = worked.iter().any(|value| *value);
            let worked_all = worked.iter().all(|value| *value);
            let has_active_spot = !active_spots.is_empty();
            let search_text = format!(
                "{} {} {}",
                prefix,
                name,
                rank.map_or_else(String::new, |value| value.to_string())
            )
            .to_lowercase();
            DxccMatrixEntity {
                name: name.to_string(),
                prefix,
                rank,
                worked,
                active_spots,
                worked_any,
                worked_all,
                has_active_spot,
                search_text,
            }
        })
        .collect();
    entities.sort_by(|a, b| {
        let needed_a = a.worked.iter().filter(|worked| !**worked).count();
        let needed_b = b.worked.iter().filter(|worked| !**worked).count();
        needed_b.cmp(&needed_a)
    });
    let worked_entities = entities.iter().filter(|entity| entity.worked_any).count();
    let spotted_unworked_entities = entities
        .iter()
        .filter(|entity| entity.has_active_spot)
        .count();
    DxccMatrix {
        bands: DXCC_MATRIX_BANDS
            .iter()
            .map(|band| (*band).into())
            .collect(),
        filters: DxccMatrixFilters {
            total_entities: entities.len(),
            worked_entities,
            unworked_entities: entities.len() - worked_entities,
            spotted_unworked_entities,
        },
        entities,
        generated_at: now,
    }
}

/// Read-only DXCC worked/spot matrix. The Engine log store and SharedSpots remain the only
/// authorities; this command snapshots and joins their facts and never invokes a radio verb.
#[tauri::command]
pub(crate) async fn dx_chaser_get_dxcc_matrix(
    state: State<'_, SharedEngine>,
    spots: State<'_, SharedSpots>,
    live_paths: State<'_, SharedLivePaths>,
    region_paths: State<'_, SharedRegionPaths>,
) -> Result<DxccMatrix, String> {
    let (engine, spots, live_paths, region_paths) = (
        Arc::clone(&state),
        Arc::clone(&spots),
        Arc::clone(&live_paths),
        region_paths.inner().clone(),
    );
    tauri::async_runtime::spawn_blocking(move || {
        let now = now_unix().max(0) as u64;
        // Clone the in-memory spot snapshot before the log pass; no shared lock is held while
        // waiting on the logbook store.
        let (mycall, cluster_active, pskr_evidence_active) = {
            let engine = engine_lock(&engine);
            (
                engine.snapshot().mycall,
                engine.settings().cluster_active(),
                engine.settings().pskr_evidence_active(),
            )
        };
        let mut spot_facts = if cluster_active {
            read_matrix_spots(&spots, now)
        } else {
            Vec::new()
        };
        if pskr_evidence_active {
            spot_facts.extend(read_matrix_pskr_spots(
                &live_paths,
                &region_paths,
                &mycall,
                now,
            ));
        }
        let worked = read_matrix_worked(&engine)?;
        Ok(build_dxcc_matrix(&worked, spot_facts, now))
    })
    .await
    .map_err(|error| format!("the DXCC matrix read did not finish: {error}"))?
}

fn feed_existing_authorities(
    chaser: &mut ChaserState,
    engine: &SharedEngine,
    spots: &SharedSpots,
    live_paths: &SharedLivePaths,
    region_paths: &SharedRegionPaths,
) {
    let now = now_unix().max(0) as u64;
    let (snap, cluster_active, pskr_evidence_active) = {
        let eng = engine_lock(engine);
        (
            eng.snapshot(),
            eng.settings().cluster_active(),
            eng.settings().pskr_evidence_active(),
        )
    };
    let country = propagation::dxcc::resolve(&snap.mycall)
        .map(|entity| entity.entity.to_string())
        .unwrap_or_default();
    chaser.engine.set_station(country, snap.mygrid.clone());
    chaser.engine.set_operating_mode(snap.link.tier.label());

    let fingerprints: Vec<String> = snap
        .recent_decodes
        .iter()
        .map(|row| {
            format!(
                "{}|{}|{}|{}",
                row.from.as_deref().unwrap_or(""),
                row.message,
                row.snr,
                row.freq_hz
            )
        })
        .collect();
    for (index, row) in snap.recent_decodes.iter().enumerate() {
        let fingerprint = &fingerprints[index];
        if chaser
            .last_decode_fingerprint
            .iter()
            .any(|old| old == fingerprint)
        {
            continue;
        }
        let Some(call) = row.from.as_ref() else {
            continue;
        };
        chaser.engine.ingest_decode(Decode {
            at: now,
            call: call.clone(),
            band: snap.radio.band.clone(),
            dial_mhz: snap.radio.dial_mhz,
            audio_hz: row.freq_hz.max(0.0) as u32,
            snr: row.snr,
            message: row.message.clone(),
            is_cq: row.is_cq,
            directed_to_me: row.directed_to_me,
            country: row.country.clone().unwrap_or_default(),
            grid: row.grid.clone(),
            new_dxcc: row.new_dxcc,
            new_band: row.new_band,
            new_grid: row.new_grid,
            rarity_rank: row.country.as_deref().and_then(dx_chaser::clublog_rank),
        });
    }
    chaser.last_decode_fingerprint = fingerprints;

    let recent = if cluster_active {
        spots
            .lock()
            .ok()
            .map(|buffer| {
                buffer.recent_within(
                    std::time::Instant::now(),
                    std::time::Duration::from_secs(chaser.engine.settings().spot_ttl_secs),
                )
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let eng = engine_lock(engine);
    for cluster in recent {
        let mode = cluster.skimmer_mode().map(str::to_string);
        if !is_chaser_cluster_spot(mode.as_deref(), cluster.freq_mhz(), cluster.rbn) {
            continue;
        }
        let band = band_for_mhz(cluster.freq_mhz());
        if band.is_empty() {
            continue;
        }
        let (new_dxcc, new_band) = eng.dx_chaser_need(&cluster.dx_call, &band);
        let country =
            propagation::dxcc::resolve(&cluster.dx_call).map(|entity| entity.entity.to_string());
        chaser.engine.ingest_spot(Spot {
            at: cluster.received_unix.max(now.saturating_sub(1)),
            call: cluster.dx_call.clone(),
            band,
            freq_mhz: cluster.freq_mhz(),
            source: if cluster.rbn { "RBN" } else { "DX cluster" }.into(),
            spotter: cluster.spotter.clone(),
            mode: mode.clone(),
            snr: None,
            rarity_rank: country.as_deref().and_then(dx_chaser::clublog_rank),
            country,
            grid: None,
            new_dxcc,
            new_band,
        });
        for corroborator in &cluster.corroborators {
            chaser.engine.ingest_spot(Spot {
                at: cluster.received_unix.max(now.saturating_sub(1)),
                call: cluster.dx_call.clone(),
                band: band_for_mhz(cluster.freq_mhz()),
                freq_mhz: cluster.freq_mhz(),
                source: "DX cluster".into(),
                spotter: corroborator.clone(),
                mode: mode.clone(),
                snr: None,
                country: None,
                grid: None,
                new_dxcc,
                new_band,
                rarity_rank: None,
            });
        }
    }
    drop(eng);

    if !pskr_evidence_active {
        return;
    }

    let ttl = chaser.engine.settings().spot_ttl_secs as i64;
    let live: Vec<_> = live_paths
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .recent(now as i64, ttl);
    let regional: Vec<_> = region_paths
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .recent(now as i64, ttl);
    let eng = engine_lock(engine);
    for path in live
        .into_iter()
        .filter(|path| path.side(&snap.mycall) == propagation::Side::IHeard)
        .chain(regional)
    {
        if !matches!(path.mode.as_deref(), Some(mode) if mode.eq_ignore_ascii_case("FT8") || mode.eq_ignore_ascii_case("FT4"))
        {
            continue;
        }
        let call = path.tx_call.trim().to_ascii_uppercase();
        if call.is_empty() {
            continue;
        }
        let band = path.band.label().to_string();
        let (new_dxcc, new_band) = eng.dx_chaser_need(&call, &band);
        if !new_dxcc && !new_band {
            continue;
        }
        let country = propagation::dxcc::resolve(&call).map(|entity| entity.entity.to_string());
        chaser.engine.ingest_spot(Spot {
            at: path.time.max(0) as u64,
            call,
            band,
            freq_mhz: path
                .freq_mhz
                .unwrap_or_else(|| propagation::band_digital_mhz(path.band)),
            source: "PSK Reporter".into(),
            spotter: path.rx_call,
            mode: path.mode,
            snr: path.snr.map(|value| value.round() as i32),
            rarity_rank: country.as_deref().and_then(dx_chaser::clublog_rank),
            country,
            grid: path.tx_grid,
            new_dxcc,
            new_band,
        });
    }
}

fn ensure_loaded(chaser: &mut ChaserState) {
    if !chaser.loaded {
        let settings = load_settings();
        chaser.sms_outbox.configure(settings.sms_min_interval_secs);
        chaser.engine.configure(settings);
        if let Some(history) = load_history() {
            chaser.engine.restore_persistent_state(history);
        }
        chaser.loaded = true;
    }
}

fn sync_arm_with_global_halt(chaser: &mut ChaserState, state: &SharedEngine) {
    let generation = engine_lock(state).tx_halt_generation();
    if chaser
        .arm_halt_generation
        .is_some_and(|armed_at| armed_at != generation)
    {
        chaser
            .engine
            .set_operator_arm(false, now_unix().max(0) as u64);
        chaser.arm_halt_generation = None;
        chaser.active_attempt = None;
    }
}

/// Reconcile the portable Chaser target with Nexus's authoritative FT "Now" state.
///
/// Nexus exposes one in-progress FT exchange rather than MSHV's separate Queue and Now
/// collections. A matching `qso.dxcall` therefore proves that Chaser's insertion is present.
/// An unrelated exchange must remain untouched: retain the Chaser target and let `cycle` defer
/// it until the operator's exchange finishes. Once the slot is free, a missing engaged/scarce
/// target can be reinserted by the portable engine; ordinary removals retire the adapter's stale
/// attempt marker together with the portable target.
fn observe_nexus_queue(
    chaser: &mut ChaserState,
    snapshot: &tempo_app::dto::AppSnapshot,
    now: u64,
) -> Vec<Action> {
    let Some(attempt) = chaser.active_attempt.as_ref() else {
        return Vec::new();
    };
    let qso_call = snapshot.qso.as_ref().and_then(|qso| qso.dxcall.as_deref());
    let presence = nexus_queue_presence(attempt, qso_call, snapshot.qso.is_some());
    if presence == NexusQueuePresence::BlockedByUnrelatedQso {
        return Vec::new();
    }
    let own_attempt_present = presence == NexusQueuePresence::Present;

    let had_target = chaser
        .engine
        .snapshot(now, snapshot.radio.tx_enabled)
        .target
        .is_some();
    let actions = chaser.engine.observe_queue(now, own_attempt_present);
    let target_remains = chaser
        .engine
        .snapshot(now, snapshot.radio.tx_enabled)
        .target
        .is_some();
    if !own_attempt_present && had_target && !target_remains && actions.is_empty() {
        chaser.active_attempt = None;
    }
    actions
}

#[tauri::command(async)]
pub(crate) fn dx_chaser_get(
    state: State<'_, SharedEngine>,
    spots: State<'_, SharedSpots>,
    live_paths: State<'_, SharedLivePaths>,
    region_paths: State<'_, SharedRegionPaths>,
    chaser: State<'_, SharedDxChaser>,
) -> Snapshot {
    let mut dx = chaser_lock(&chaser);
    ensure_loaded(&mut dx);
    sync_arm_with_global_halt(&mut dx, &state);
    feed_existing_authorities(&mut dx, &state, &spots, &live_paths, &region_paths);
    let armed = engine_lock(&state).tx_enabled();
    dx.engine.snapshot(now_unix().max(0) as u64, armed)
}

#[tauri::command(async)]
pub(crate) fn dx_chaser_configure(
    settings: Settings,
    state: State<'_, SharedEngine>,
    chaser: State<'_, SharedDxChaser>,
) -> Result<Snapshot, String> {
    configure_transaction_with(
        settings,
        &chaser,
        save_settings,
        || engine_lock(&state).halt_tx(),
        || engine_lock(&state).tx_enabled(),
    )
}

fn configure_transaction_with<Save, Halt, TxEnabled>(
    settings: Settings,
    chaser: &SharedDxChaser,
    save: Save,
    halt_tx: Halt,
    tx_enabled: TxEnabled,
) -> Result<Snapshot, String>
where
    Save: FnOnce(&Settings) -> Result<(), String>,
    Halt: FnOnce(),
    TxEnabled: FnOnce() -> bool,
{
    let _transaction = CONFIGURE_TRANSACTION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let settings = settings.normalized();

    // Durable state is the commit point. If it cannot be installed, runtime state is untouched.
    save(&settings)?;
    if !settings.enabled {
        halt_tx();
    }
    let armed = tx_enabled();
    let mut dx = chaser_lock(chaser);
    dx.sms_outbox.configure(settings.sms_min_interval_secs);
    dx.engine.configure(settings);
    if !dx.engine.settings().enabled {
        dx.arm_halt_generation = None;
        dx.active_attempt = None;
    }
    dx.loaded = true;
    Ok(dx.engine.snapshot(now_unix().max(0) as u64, armed))
}

#[tauri::command(async)]
pub(crate) fn dx_chaser_cycle(
    state: State<'_, SharedEngine>,
    spots: State<'_, SharedSpots>,
    live_paths: State<'_, SharedLivePaths>,
    region_paths: State<'_, SharedRegionPaths>,
    chaser: State<'_, SharedDxChaser>,
) -> Result<Snapshot, String> {
    let now = now_unix().max(0) as u64;
    let mut dx = chaser_lock(&chaser);
    ensure_loaded(&mut dx);
    sync_arm_with_global_halt(&mut dx, &state);
    feed_existing_authorities(&mut dx, &state, &spots, &live_paths, &region_paths);
    let mut snap = engine_lock(&state).snapshot();
    let queue_actions = observe_nexus_queue(&mut dx, &snap, now);
    let queue_reinserted = !queue_actions.is_empty();
    drop(dx);
    dispatch(queue_actions, &state, &chaser)?;
    if queue_reinserted {
        let mut dx = chaser_lock(&chaser);
        feed_existing_authorities(&mut dx, &state, &spots, &live_paths, &region_paths);
        save_history(&dx.engine)?;
        let armed = engine_lock(&state).tx_enabled();
        return Ok(dx.engine.snapshot(now, armed));
    }

    // Any Nexus QSO blocks new Chaser work. The matching check above is deliberately separate:
    // only Chaser's own exchange proves queue presence, while an unrelated operator exchange
    // still protects the transmitter from being replaced.
    snap = engine_lock(&state).snapshot();
    let mut dx = chaser_lock(&chaser);
    let actions = dx.engine.cycle(
        now,
        &snap.radio.band,
        snap.radio.dial_mhz,
        snap.radio.tx_enabled,
        snap.qso.is_some(),
    );
    drop(dx);
    dispatch(actions, &state, &chaser)?;
    let mut dx = chaser_lock(&chaser);
    feed_existing_authorities(&mut dx, &state, &spots, &live_paths, &region_paths);
    save_history(&dx.engine)?;
    let armed = engine_lock(&state).tx_enabled();
    Ok(dx.engine.snapshot(now, armed))
}

fn execute(
    action: Action,
    state: &SharedEngine,
    armed_generation: Option<u64>,
) -> Result<(Option<ActiveAttempt>, Option<u64>), String> {
    let mut eng = engine_lock(state);
    match action {
        Action::Call {
            call,
            band,
            grid,
            message,
            snr,
            audio_hz,
            tx_audio_hz,
        } => {
            let generation = armed_generation
                .ok_or_else(|| "DX Chaser call requires a live operator arm binding".to_string())?;
            let baseline_log_tick = eng.snapshot().log_tick;
            eng.dx_chaser_call_while_armed_ctx(
                generation,
                &call,
                grid.as_deref(),
                Some(&message),
                Some(snr),
                Some(audio_hz as f32),
                tx_audio_hz as f32,
            )?;
            Ok((
                Some(ActiveAttempt {
                    call,
                    band,
                    started_at: now_unix().max(0) as u64,
                    baseline_log_tick,
                }),
                None,
            ))
        }
        Action::ChangeBand { band, dial_mhz, .. }
        | Action::ChangeDial { band, dial_mhz, .. }
        | Action::ReturnToStandard { band, dial_mhz } => {
            let generation = armed_generation
                .ok_or_else(|| "DX Chaser QSY requires a live operator arm binding".to_string())?;
            let tier = eng.snapshot().link.tier;
            let rebound = eng.dx_chaser_qsy_while_armed(generation, tier, dial_mhz, &band)?;
            Ok((None, Some(rebound)))
        }
        Action::Notify { title, body } => {
            tempo_core::applog::info("dx-chaser", &format!("{title}: {body}"));
            Ok((None, None))
        }
        Action::Sms { .. } => Err("internal DX Chaser SMS dispatch error".into()),
    }
}

fn dispatch(
    actions: Vec<Action>,
    state: &SharedEngine,
    chaser: &SharedDxChaser,
) -> Result<(), String> {
    let original = {
        let snapshot = engine_lock(state).snapshot();
        (snapshot.radio.band, snapshot.radio.dial_mhz)
    };
    let mut radio_mutated = false;
    let twilio_credentials = actions
        .iter()
        .any(|action| matches!(action, Action::Sms { .. }))
        .then(load_twilio_credentials)
        .and_then(Result::ok)
        .filter(TwilioCredentials::complete);
    for action in actions {
        if let Action::Sms { body } = action {
            let mut dx = chaser_lock(chaser);
            enqueue_sms_if_configured(
                &mut dx,
                body,
                now_unix().max(0) as u64,
                twilio_credentials.as_ref(),
            );
        } else {
            radio_mutated |= matches!(
                &action,
                Action::Call { .. }
                    | Action::ChangeBand { .. }
                    | Action::ChangeDial { .. }
                    | Action::ReturnToStandard { .. }
            );
            let armed_generation = chaser_lock(chaser).arm_halt_generation;
            match execute(action, state, armed_generation) {
                Ok((attempt, rebound)) => {
                    let mut dx = chaser_lock(chaser);
                    if let Some(attempt) = attempt {
                        dx.active_attempt = Some(attempt);
                    }
                    if let Some(rebound) = rebound {
                        dx.arm_halt_generation = Some(rebound);
                    }
                }
                Err(error) => {
                    let mut engine = engine_lock(state);
                    engine.halt_tx();
                    if radio_mutated {
                        engine.work_spot_tiered(None, "digital", original.1, &original.0, None);
                    }
                    drop(engine);
                    chaser_lock(chaser)
                        .engine
                        .action_failed(now_unix().max(0) as u64, error.clone());
                    return Err(error);
                }
            }
        }
    }
    Ok(())
}

fn enqueue_sms_if_configured(
    dx: &mut ChaserState,
    body: String,
    now: u64,
    credentials: Option<&TwilioCredentials>,
) -> bool {
    if !credentials.is_some_and(TwilioCredentials::complete) {
        return false;
    }
    dx.sms_outbox.enqueue(body, now);
    dx.engine.sms_queued();
    true
}

fn remember_qso_signature(seen: &mut VecDeque<String>, signature: String) -> bool {
    if seen.iter().any(|old| old == &signature) {
        return false;
    }
    seen.push_back(signature);
    while seen.len() > 1_024 {
        seen.pop_front();
    }
    true
}

fn fresh_logged_qsos(
    state: &SharedEngine,
    seen: &mut VecDeque<String>,
    min_when_unix: u64,
) -> Result<Vec<LoggedQsoFact>, String> {
    let rows = engine_lock(state).log_rows();
    let mut facts = Vec::new();
    rows.each_record(std::time::Duration::from_secs(2), &mut |record| {
        if record.when_unix >= min_when_unix
            && matches!(record.mode.to_ascii_uppercase().as_str(), "FT8" | "FT4")
        {
            facts.push(LoggedQsoFact {
                signature: format!(
                    "{}|{}|{}|{}",
                    record.call.to_ascii_uppercase(),
                    record.band.to_ascii_lowercase(),
                    record.mode.to_ascii_uppercase(),
                    record.when_unix
                ),
                call: record.call.clone(),
                entity: record.country.clone().unwrap_or_default(),
                band: record.band.clone(),
                mode: record.mode.clone(),
                grid: record.grid.clone(),
                when_unix: record.when_unix,
            });
        }
        ControlFlow::Continue(())
    })
    .map_err(|error| error.to_string())?;
    facts.sort_by_key(|fact| fact.when_unix);
    facts.retain(|fact| remember_qso_signature(seen, fact.signature.clone()));
    Ok(facts)
}

fn flush_sms(chaser: &SharedDxChaser) {
    let now = now_unix().max(0) as u64;
    let body = chaser_lock(chaser)
        .sms_outbox
        .due(now)
        .map(|job| job.body.clone());
    let Some(body) = body else { return };
    match send_twilio_delivery(&body) {
        Ok(()) => {
            chaser_lock(chaser).sms_outbox.delivered(now);
        }
        Err(error) => {
            let dropped = if error.retryable {
                chaser_lock(chaser).sms_outbox.failed(now)
            } else {
                chaser_lock(chaser).sms_outbox.discard()
            };
            tempo_core::applog::warn("dx-chaser", &format!("Twilio delivery failed: {error}"));
            if dropped.is_some() {
                tempo_core::applog::warn(
                    "dx-chaser",
                    "Twilio message dropped after bounded retries",
                );
            }
        }
    }
}

/// One background service tick. Only a new FT slot evaluates candidates, so a fast
/// supervisor poll cannot consume missed-cycle budgets early. This keeps DX Chaser running
/// when its screen is closed while leaving every radio mutation in the existing Engine.
pub(crate) fn background_tick(
    state: &SharedEngine,
    spots: &SharedSpots,
    live_paths: &SharedLivePaths,
    region_paths: &SharedRegionPaths,
    chaser: &SharedDxChaser,
) -> Result<(), String> {
    let snap = engine_lock(state).snapshot();
    let now = now_unix().max(0) as u64;
    let mut dx = chaser_lock(chaser);
    ensure_loaded(&mut dx);
    sync_arm_with_global_halt(&mut dx, state);
    feed_existing_authorities(&mut dx, state, spots, live_paths, region_paths);

    let qso_active = snap.qso.is_some();
    if !dx.log_baselined {
        let baseline = fresh_logged_qsos(state, &mut dx.observed_qsos, 0)?;
        dx.log_high_water = baseline
            .iter()
            .map(|fact| fact.when_unix)
            .max()
            .unwrap_or(0);
        dx.last_log_tick = snap.log_tick;
        dx.log_baselined = true;
    }
    let logged_qsos = if snap.log_tick != dx.last_log_tick && dx.engine.settings().enabled {
        let log_high_water = dx.log_high_water;
        let facts = fresh_logged_qsos(state, &mut dx.observed_qsos, log_high_water)?;
        dx.log_high_water = facts
            .iter()
            .map(|fact| fact.when_unix)
            .max()
            .unwrap_or(dx.log_high_water);
        facts
    } else {
        Vec::new()
    };
    let mut logged_actions = Vec::new();
    for fact in logged_qsos {
        let matching_attempt = dx.active_attempt.as_ref().is_some_and(|attempt| {
            snap.log_tick != attempt.baseline_log_tick
                && dx_chaser::logged_qso_matches_attempt(
                    &fact.call,
                    &fact.band,
                    &fact.mode,
                    fact.when_unix,
                    &attempt.call,
                    &attempt.band,
                    attempt.started_at,
                )
        });
        let target_present = dx
            .engine
            .snapshot(now, snap.radio.tx_enabled)
            .target
            .is_some();
        if matching_attempt && target_present {
            logged_actions.extend(dx.engine.qso_complete_with_facts(
                now,
                &fact.call,
                &fact.entity,
                &fact.band,
                fact.grid.as_deref(),
            ));
            dx.active_attempt = None;
            dx.last_qso_active = false;
        } else {
            logged_actions.extend(dx.engine.observe_logged_qso(
                now,
                &fact.call,
                &fact.entity,
                &fact.band,
                fact.grid.as_deref(),
            ));
        }
    }
    dx.last_qso_active = qso_active;
    dx.last_log_tick = snap.log_tick;

    if !logged_actions.is_empty() {
        drop(dx);
        dispatch(logged_actions, state, chaser)?;
        dx = chaser_lock(chaser);
        save_history(&dx.engine)?;
        if dx.last_slot == Some(snap.radio.slot) {
            return Ok(());
        }
    }

    let queue_actions = observe_nexus_queue(&mut dx, &snap, now);
    let queue_reinserted = !queue_actions.is_empty();
    drop(dx);
    dispatch(queue_actions, state, chaser)?;
    if queue_reinserted {
        let dx = chaser_lock(chaser);
        save_history(&dx.engine)?;
        return Ok(());
    }

    let mut dx = chaser_lock(chaser);
    if dx.last_slot == Some(snap.radio.slot) {
        return Ok(());
    }
    dx.last_slot = Some(snap.radio.slot);
    let actions = dx.engine.cycle(
        now,
        &snap.radio.band,
        snap.radio.dial_mhz,
        snap.radio.tx_enabled,
        qso_active,
    );
    drop(dx);
    dispatch(actions, state, chaser)?;
    flush_sms(chaser);
    save_history(&chaser_lock(chaser).engine)?;
    Ok(())
}

#[tauri::command(async)]
pub(crate) fn dx_chaser_stop(
    state: State<'_, SharedEngine>,
    chaser: State<'_, SharedDxChaser>,
) -> Snapshot {
    engine_lock(&state).halt_tx();
    let mut dx = chaser_lock(&chaser);
    dx.arm_halt_generation = None;
    dx.active_attempt = None;
    dx.engine
        .stop(now_unix().max(0) as u64, "stopped by operator");
    dx.engine.snapshot(now_unix().max(0) as u64, false)
}

#[tauri::command(async)]
pub(crate) fn dx_chaser_set_armed(
    armed: bool,
    state: State<'_, SharedEngine>,
    chaser: State<'_, SharedDxChaser>,
) -> Result<Snapshot, String> {
    let now = now_unix().max(0) as u64;
    set_armed_transaction_with(
        armed,
        &chaser,
        now,
        || {
            let engine = engine_lock(&state);
            (engine.tx_enabled(), engine.tx_halt_generation())
        },
        || engine_lock(&state).halt_tx(),
        || engine_lock(&state).tx_enabled(),
    )
}

fn set_armed_transaction_with<Observe, Halt, TxEnabled>(
    armed: bool,
    chaser: &SharedDxChaser,
    now: u64,
    observe_tx: Observe,
    halt_tx: Halt,
    tx_enabled: TxEnabled,
) -> Result<Snapshot, String>
where
    Observe: FnOnce() -> (bool, u64),
    Halt: FnOnce(),
    TxEnabled: FnOnce() -> bool,
{
    {
        let mut dx = chaser_lock(chaser);
        ensure_loaded(&mut dx);
    }
    // TX authority and its generation are one Engine observation. If Stop lands after this
    // lock is released, the captured generation is stale and both sync and execution reject it.
    let (enabled, generation) = observe_tx();
    if armed && !enabled {
        return Err("Enable TX must be armed before DX Chaser automation".into());
    }
    {
        let mut dx = chaser_lock(chaser);
        dx.engine.set_operator_arm(armed, now);
        dx.active_attempt = None;
        dx.arm_halt_generation = dx.engine.automation_armed().then_some(generation);
    }
    if !armed {
        halt_tx();
    }
    let enabled = tx_enabled();
    Ok(chaser_lock(chaser).engine.snapshot(now, enabled))
}

#[tauri::command(async)]
pub(crate) fn dx_chaser_reset_statistics(
    chaser: State<'_, SharedDxChaser>,
) -> Result<Snapshot, String> {
    let mut dx = chaser_lock(&chaser);
    let now = now_unix().max(0) as u64;
    dx.engine.reset_statistics(now);
    save_history(&dx.engine)?;
    Ok(dx.engine.snapshot(now, false))
}

fn credential_entry(name: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, name).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub(crate) fn dx_chaser_set_twilio_credentials(
    credentials: TwilioCredentials,
) -> Result<(), String> {
    let credentials = credentials.normalized();
    let bundle = serde_json::to_string(&credentials).map_err(|e| e.to_string())?;
    credential_entry(KEYRING_BUNDLE)?
        .set_password(&bundle)
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub(crate) fn dx_chaser_clear_twilio_credentials() -> Result<(), String> {
    for name in [
        KEYRING_BUNDLE,
        "accountSid",
        "authToken",
        "fromNumber",
        "toNumber",
    ] {
        let entry = credential_entry(name)?;
        if let Err(e) = entry.delete_credential() {
            if !matches!(e, keyring::Error::NoEntry) {
                return Err(e.to_string());
            }
        }
    }
    Ok(())
}

fn load_twilio_credentials() -> Result<TwilioCredentials, String> {
    match credential_entry(KEYRING_BUNDLE)?.get_password() {
        Ok(bundle) => serde_json::from_str::<TwilioCredentials>(&bundle)
            .map(TwilioCredentials::normalized)
            .map_err(|e| e.to_string()),
        Err(keyring::Error::NoEntry) => {
            let legacy = TwilioCredentials {
                account_sid: credential_entry("accountSid")?
                    .get_password()
                    .map_err(|e| e.to_string())?,
                auth_token: credential_entry("authToken")?
                    .get_password()
                    .map_err(|e| e.to_string())?,
                from_number: credential_entry("fromNumber")?
                    .get_password()
                    .map_err(|e| e.to_string())?,
                to_number: credential_entry("toNumber")?
                    .get_password()
                    .map_err(|e| e.to_string())?,
            }
            .normalized();
            dx_chaser_set_twilio_credentials(legacy.clone())?;
            for name in ["accountSid", "authToken", "fromNumber", "toNumber"] {
                let _ = credential_entry(name)?.delete_credential();
            }
            Ok(legacy)
        }
        Err(error) => Err(error.to_string()),
    }
}

#[tauri::command(async)]
pub(crate) fn dx_chaser_test_twilio() -> Result<(), String> {
    send_twilio_delivery("MSHV AutoDxer: Test message - SMS notifications are working!")
        .map_err(|error| error.to_string())
}

fn form(value: &str) -> String {
    value
        .bytes()
        .flat_map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                vec![b as char]
            } else {
                format!("%{b:02X}").chars().collect()
            }
        })
        .collect()
}
#[derive(Debug)]
struct TwilioSendError {
    message: String,
    retryable: bool,
}

fn twilio_status_retryable(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS
}

fn redacted_twilio_client_error(_error: &impl std::fmt::Display) -> TwilioSendError {
    TwilioSendError {
        message: TWILIO_CLIENT_FAILURE.into(),
        retryable: false,
    }
}

fn redacted_twilio_transport_error(_error: &impl std::fmt::Display) -> TwilioSendError {
    // reqwest transport errors commonly include the request URL. The Twilio Account SID is a
    // path component, so neither the source error nor its chain may cross this boundary.
    TwilioSendError {
        message: TWILIO_AMBIGUOUS_FAILURE.into(),
        retryable: false,
    }
}

impl std::fmt::Display for TwilioSendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

fn send_twilio_delivery(body: &str) -> Result<(), TwilioSendError> {
    let credentials = load_twilio_credentials().map_err(|message| TwilioSendError {
        message,
        retryable: false,
    })?;
    let sid = credentials.account_sid;
    let token = credentials.auth_token;
    let from = credentials.from_number;
    let to = credentials.to_number;
    let response = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|error| redacted_twilio_client_error(&error))?
        .post(format!(
            "https://api.twilio.com/2010-04-01/Accounts/{sid}/Messages.json"
        ))
        .basic_auth(sid, Some(token))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(format!(
            "From={}&To={}&Body={}",
            form(&from),
            form(&to),
            form(body)
        ))
        .send()
        .map_err(|error| redacted_twilio_transport_error(&error))?;
    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        Err(TwilioSendError {
            message: format!("Twilio returned {status}"),
            retryable: twilio_status_retryable(status),
        })
    }
}

#[cfg(test)]
mod matrix_tests {
    use super::*;

    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "nexus-dx-chaser-{name}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn settings_loader_handles_missing_old_corrupt_and_truncated_files() {
        let root = scratch_dir("settings-load");
        let path = root.join("dx-chaser.json");
        assert!(load_json::<Settings>(&path, "test settings").is_none());

        std::fs::write(&path, b"{}").unwrap();
        let old = load_json::<Settings>(&path, "test settings").unwrap();
        assert_eq!(old.sms_min_interval_secs, 60);
        assert!(old.use_rbn && old.use_psk_reporter && old.use_dx_cluster);

        std::fs::write(&path, b"{not-json}").unwrap();
        assert!(load_json::<Settings>(&path, "test settings").is_none());
        std::fs::write(&path, br#"{"enabled":true"#).unwrap();
        assert!(load_json::<Settings>(&path, "test settings").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn atomic_json_failure_preserves_last_known_good_file() {
        let root = scratch_dir("atomic");
        let path = root.join("dx-chaser.json");
        let original = Settings::default();
        write_json_atomic(&path, &original).unwrap();
        let original_bytes = std::fs::read(&path).unwrap();

        let changed = Settings {
            enabled: true,
            ..Settings::default()
        };
        assert!(write_json_atomic_with(&path, &changed, |_, _| {
            Err(std::io::Error::other("injected install failure"))
        })
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original_bytes);

        write_json_atomic(&path, &changed).unwrap();
        assert!(
            load_json::<Settings>(&path, "test settings")
                .unwrap()
                .enabled
        );
        assert!(std::fs::read_dir(&root).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_atomic_writers_leave_one_complete_json_and_no_temp_debris() {
        let root = scratch_dir("atomic-concurrent");
        let path = root.join("dx-chaser.json");
        let barrier = Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (1..=8)
            .map(|missed_cycles| {
                let path = path.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let value = Settings {
                        missed_cycles,
                        ..Settings::default()
                    };
                    barrier.wait();
                    write_json_atomic(&path, &value).unwrap();
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let saved = load_json::<Settings>(&path, "concurrent settings").unwrap();
        assert!((1..=8).contains(&saved.missed_cycles));
        assert!(std::fs::read_dir(&root).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn configuration_transaction_keeps_last_durable_and_runtime_settings_identical() {
        let root = scratch_dir("configure-transaction");
        let path = root.join("dx-chaser.json");
        let chaser: SharedDxChaser = Arc::new(std::sync::Mutex::new(ChaserState::default()));
        let first_in_save = Arc::new(std::sync::Barrier::new(2));
        let release_first = Arc::new(std::sync::Barrier::new(2));

        let first = {
            let chaser = Arc::clone(&chaser);
            let path = path.clone();
            let first_in_save = Arc::clone(&first_in_save);
            let release_first = Arc::clone(&release_first);
            std::thread::spawn(move || {
                configure_transaction_with(
                    Settings {
                        sms_min_interval_secs: 61,
                        ..Settings::default()
                    },
                    &chaser,
                    |settings| {
                        first_in_save.wait();
                        release_first.wait();
                        write_json_atomic(&path, settings)
                    },
                    || {},
                    || false,
                )
                .unwrap();
            })
        };

        // The first caller is holding CONFIGURE_TRANSACTION inside its durable-save step. Start
        // the second now: it must not save until the first has also applied its runtime state.
        first_in_save.wait();
        let second = {
            let chaser = Arc::clone(&chaser);
            let path = path.clone();
            std::thread::spawn(move || {
                configure_transaction_with(
                    Settings {
                        sms_min_interval_secs: 62,
                        ..Settings::default()
                    },
                    &chaser,
                    |settings| write_json_atomic(&path, settings),
                    || {},
                    || false,
                )
                .unwrap();
            })
        };
        release_first.wait();
        first.join().unwrap();
        second.join().unwrap();

        let saved = load_json::<Settings>(&path, "transaction settings").unwrap();
        let runtime = chaser_lock(&chaser).engine.settings().clone();
        assert_eq!(saved.sms_min_interval_secs, 62);
        assert_eq!(runtime.sms_min_interval_secs, 62);
        assert_eq!(saved, runtime);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn configuration_save_failure_leaves_durable_and_runtime_settings_unchanged() {
        let root = scratch_dir("configure-failure");
        let path = root.join("dx-chaser.json");
        let chaser: SharedDxChaser = Arc::new(std::sync::Mutex::new(ChaserState::default()));
        configure_transaction_with(
            Settings {
                sms_min_interval_secs: 73,
                ..Settings::default()
            },
            &chaser,
            |settings| write_json_atomic(&path, settings),
            || {},
            || false,
        )
        .unwrap();
        let before = std::fs::read(&path).unwrap();

        let error = configure_transaction_with(
            Settings {
                enabled: true,
                sms_min_interval_secs: 99,
                ..Settings::default()
            },
            &chaser,
            |_| Err("injected durable-save failure".into()),
            || panic!("a failed durable commit must not halt TX"),
            || panic!("a failed durable commit must not inspect or apply runtime state"),
        )
        .unwrap_err();
        assert_eq!(error, "injected durable-save failure");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let saved = load_json::<Settings>(&path, "transaction settings").unwrap();
        let runtime = chaser_lock(&chaser).engine.settings().clone();
        assert_eq!(saved.sms_min_interval_secs, 73);
        assert_eq!(saved, runtime);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stop_during_arm_observation_cannot_bind_authority_for_a_later_reenable() {
        let engine: SharedEngine = Arc::new(Mutex::new(tempo_app::engine::Engine::new(
            "W9XYZ", "EN37", 0,
        )));
        engine_lock(&engine).set_tx_enabled(true);
        let chaser: SharedDxChaser = Arc::new(Mutex::new(ChaserState::default()));
        {
            let mut dx = chaser_lock(&chaser);
            dx.engine.configure(Settings {
                enabled: true,
                ..Settings::default()
            });
            dx.loaded = true;
        }
        let observed = Arc::new(std::sync::Barrier::new(2));
        let resume = Arc::new(std::sync::Barrier::new(2));
        let arm_thread = {
            let engine_for_observe = Arc::clone(&engine);
            let engine_for_snapshot = Arc::clone(&engine);
            let chaser = Arc::clone(&chaser);
            let observed = Arc::clone(&observed);
            let resume = Arc::clone(&resume);
            std::thread::spawn(move || {
                set_armed_transaction_with(
                    true,
                    &chaser,
                    10,
                    || {
                        let binding = {
                            let engine = engine_lock(&engine_for_observe);
                            (engine.tx_enabled(), engine.tx_halt_generation())
                        };
                        observed.wait();
                        resume.wait();
                        binding
                    },
                    || unreachable!("arming never invokes halt"),
                    || engine_lock(&engine_for_snapshot).tx_enabled(),
                )
                .unwrap()
            })
        };

        observed.wait();
        {
            let mut engine = engine_lock(&engine);
            engine.halt_tx();
            engine.set_tx_enabled(true);
        }
        let current_generation = engine_lock(&engine).tx_halt_generation();
        resume.wait();
        let snapshot = arm_thread.join().unwrap();
        assert!(
            snapshot.automation_armed,
            "the forced race applied its stale arm"
        );
        let stale_generation = chaser_lock(&chaser).arm_halt_generation.unwrap();
        assert_ne!(stale_generation, current_generation);

        {
            let mut dx = chaser_lock(&chaser);
            sync_arm_with_global_halt(&mut dx, &engine);
            assert!(!dx.engine.automation_armed());
            assert!(dx.arm_halt_generation.is_none());
        }
        assert!(engine_lock(&engine)
            .dx_chaser_call_while_armed_ctx(
                stale_generation,
                "F4ABC",
                Some("JN18"),
                None,
                None,
                None,
                1_500.0,
            )
            .is_err());
    }

    #[test]
    fn twilio_retries_only_an_explicit_rate_limit_response() {
        assert!(twilio_status_retryable(
            reqwest::StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(!twilio_status_retryable(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR
        ));
        assert!(!twilio_status_retryable(
            reqwest::StatusCode::REQUEST_TIMEOUT
        ));
    }

    #[test]
    fn twilio_transport_errors_never_expose_the_account_url_or_source_chain() {
        let sentinel_sid = "AC_SENTINEL_MUST_NOT_ESCAPE";
        let sentinel_url =
            format!("https://api.twilio.com/2010-04-01/Accounts/{sentinel_sid}/Messages.json");
        let source = format!("request failed for {sentinel_url}; auth=TOKEN_SENTINEL");
        let error = redacted_twilio_transport_error(&source);
        let public = error.to_string();
        assert_eq!(public, TWILIO_AMBIGUOUS_FAILURE);
        assert!(!error.retryable);
        for secret in [sentinel_sid, sentinel_url.as_str(), "TOKEN_SENTINEL"] {
            assert!(!public.contains(secret));
        }
        let client_error = redacted_twilio_client_error(&source).to_string();
        assert_eq!(client_error, TWILIO_CLIENT_FAILURE);
        assert!(!client_error.contains(sentinel_sid));
        assert!(!client_error.contains("TOKEN_SENTINEL"));
    }

    #[test]
    fn twilio_phone_numbers_are_trimmed_and_normalized_to_e164() {
        assert_eq!(normalize_e164(" 15551234567 "), "+15551234567");
        assert_eq!(normalize_e164(" +15557654321 "), "+15557654321");
        assert_eq!(normalize_e164("   "), "");
    }

    #[test]
    fn unconfigured_twilio_drops_tx_and_qso_actions_without_queue_or_retry() {
        let mut state = ChaserState::default();
        assert!(!enqueue_sms_if_configured(
            &mut state,
            "MSHV TX: Calling W1AW on 20m FT8\n12:00z".into(),
            1,
            None,
        ));
        let incomplete = TwilioCredentials {
            account_sid: "AC123".into(),
            auth_token: String::new(),
            from_number: "+10000000000".into(),
            to_number: "+10000000001".into(),
        };
        assert!(!enqueue_sms_if_configured(
            &mut state,
            "MSHV QSO: W1AW (United States)\n20m FN31\n12:01z".into(),
            2,
            Some(&incomplete),
        ));
        assert_eq!(state.sms_outbox.len(), 0);
        let snapshot = state.engine.snapshot(2, false);
        assert_eq!(snapshot.session_statistics.sms_queued, 0);
        assert_eq!(snapshot.all_time_statistics.sms_queued, 0);
    }

    #[test]
    fn logged_qso_signature_suppresses_replayed_log_ticks() {
        let mut seen = VecDeque::new();
        assert!(remember_qso_signature(&mut seen, "W1AW|20m|FT8|100".into()));
        assert!(!remember_qso_signature(
            &mut seen,
            "W1AW|20m|FT8|100".into()
        ));
        assert!(remember_qso_signature(&mut seen, "W1AW|20m|FT8|101".into()));
    }

    #[test]
    fn adapter_ingest_respects_us_spotters_and_rare_dx_exemption() {
        let mut chaser = Chaser::default();
        let spot = |call: &str, spotter: &str| Spot {
            at: 10,
            call: call.into(),
            band: "20m".into(),
            freq_mhz: 14.074,
            source: "DX cluster".into(),
            spotter: spotter.into(),
            mode: Some("FT8".into()),
            snr: Some(-12),
            country: None,
            grid: None,
            new_dxcc: true,
            new_band: true,
            rarity_rank: None,
        };
        chaser.ingest_spot(spot("F4ABC", "DL1ABC"));
        assert!(chaser.snapshot(10, false).sources.is_empty());
        chaser.ingest_spot(spot("F4ABC", "K1ABC"));
        chaser.ingest_spot(spot("3Y0J", "DL1ABC"));
        let snapshot = chaser.snapshot(10, false);
        assert_eq!(
            snapshot
                .sources
                .iter()
                .map(|source| source.items)
                .sum::<u32>(),
            2
        );
    }

    #[test]
    fn queue_presence_binds_only_the_active_attempt_base_call() {
        let attempt = ActiveAttempt {
            call: "KH8/W1AW".into(),
            band: "20m".into(),
            started_at: 10,
            baseline_log_tick: 3,
        };
        assert!(attempt_matches_qso(&attempt, Some("W1AW/P")));
        assert!(!attempt_matches_qso(&attempt, Some("K2ABC")));
        assert!(!attempt_matches_qso(&attempt, None));
        assert_eq!(
            nexus_queue_presence(&attempt, Some("K2ABC"), true),
            NexusQueuePresence::BlockedByUnrelatedQso
        );
        assert_eq!(
            nexus_queue_presence(&attempt, None, false),
            NexusQueuePresence::Missing
        );
    }

    #[test]
    fn chaser_external_sources_accept_only_ft8_and_ft4() {
        assert!(is_chaser_digital_mode(Some("FT8")));
        assert!(is_chaser_digital_mode(Some("ft4")));
        assert!(!is_chaser_digital_mode(Some("CW")));
        assert!(!is_chaser_digital_mode(Some("SSB")));
        assert!(!is_chaser_digital_mode(None));
        assert!(is_chaser_cluster_spot(Some("FT8"), 14.2, false));
        assert!(is_chaser_cluster_spot(Some("ft4"), 14.2, true));
        assert!(is_chaser_cluster_spot(None, 14.074, false));
        assert!(is_chaser_cluster_spot(None, 14.0989, false));
        assert!(!is_chaser_cluster_spot(None, 14.1001, false));
        assert!(!is_chaser_cluster_spot(None, 14.074, true));
        assert!(!is_chaser_cluster_spot(Some("CW"), 14.074, false));
    }

    fn spot(call: &str, band: &str) -> DxccMatrixSpot {
        DxccMatrixSpot {
            call: call.into(),
            band: band.into(),
            freq_mhz: if band == "20m" { 14.025 } else { 7.025 },
            source: "DX cluster".into(),
            spotter: "W1AW".into(),
            mode: Some("CW".into()),
            received_unix: 90,
            age_seconds: 10,
        }
    }

    #[test]
    fn primary_prefixes_follow_cty_headers() {
        let prefixes = dxcc_primary_prefixes();
        assert_eq!(prefixes.get("United States").map(String::as_str), Some("K"));
        assert_eq!(prefixes.get("Bouvet").map(String::as_str), Some("3Y/B"));
    }

    #[test]
    fn matrix_keeps_band_order_and_only_overlays_unworked_spots() {
        assert_eq!(DXCC_MATRIX_SPOT_AGE, Duration::from_secs(300));
        let worked = HashSet::from([
            ("Japan".to_string(), "20m".to_string()),
            ("United States".to_string(), "40m".to_string()),
        ]);
        let facts = vec![
            MatrixSpotFact {
                entity: "Japan".into(),
                spot: spot("JA1DX", "20m"),
            },
            MatrixSpotFact {
                entity: "Japan".into(),
                spot: spot("JA1DX", "40m"),
            },
        ];
        let matrix = build_dxcc_matrix(&worked, facts, 100);
        assert_eq!(
            matrix.bands,
            ["160m", "80m", "40m", "30m", "20m", "17m", "15m", "12m", "10m"]
        );
        assert_eq!(
            matrix.entities.len(),
            propagation::dxcc::current_dxcc_entities()
        );
        let japan = matrix
            .entities
            .iter()
            .find(|entity| entity.name == "Japan")
            .expect("Japan is a current DXCC entity");
        assert_eq!(
            japan.worked,
            [false, false, false, false, true, false, false, false, false]
        );
        assert_eq!(japan.active_spots.len(), 1);
        assert_eq!(japan.active_spots[0].band, "40m");
        assert!(japan.has_active_spot);
        assert!(japan.search_text.contains("japan"));
        let needed: Vec<_> = matrix
            .entities
            .iter()
            .map(|entity| entity.worked.iter().filter(|worked| !**worked).count())
            .collect();
        assert!(needed.windows(2).all(|pair| pair[0] >= pair[1]));
    }

    #[test]
    fn matrix_includes_received_and_regional_psk_reporter_paths_only() {
        let now = 1_000;
        let live: SharedLivePaths =
            Arc::new(std::sync::Mutex::new(propagation::LiveSpots::default()));
        let region = SharedRegionPaths(Arc::new(std::sync::Mutex::new(
            propagation::LiveSpots::default(),
        )));
        for (tx, rx) in [("JA1ABC", "N0CALL"), ("N0CALL", "JA1ABC")] {
            live.lock().unwrap().push(propagation::PathSpot {
                time: now as i64,
                tx_call: tx.into(),
                tx_grid: Some("PM95".into()),
                rx_call: rx.into(),
                rx_grid: Some("EN52".into()),
                band: propagation::Band::B20,
                mode: Some("FT8".into()),
                snr: Some(-12.0),
                freq_mhz: None,
            });
        }
        region.0.lock().unwrap().push(propagation::PathSpot {
            time: now as i64,
            tx_call: "F4ABC".into(),
            tx_grid: Some("JN18".into()),
            rx_call: "DL1ABC".into(),
            rx_grid: Some("JO31".into()),
            band: propagation::Band::B40,
            mode: Some("FT4".into()),
            snr: Some(-8.0),
            freq_mhz: None,
        });
        let facts = read_matrix_pskr_spots(&live, &region, "N0CALL", now);
        assert_eq!(facts.len(), 2, "who-heard-me must not become a chase spot");
        assert!(facts.iter().any(|fact| fact.spot.call == "JA1ABC"));
        assert!(facts.iter().any(|fact| fact.spot.call == "F4ABC"));
        assert!(facts.iter().all(|fact| fact.spot.source == "PSK Reporter"));
    }
}
