//! MSHV Auto DX Chaser compatible decision engine.
//!
//! This crate deliberately owns decisions, never radio or logbook I/O. Nexus feeds it
//! decoded messages, spot intelligence and worked-state facts, then executes returned
//! actions through the existing Engine safety gates. That makes the same code part of
//! every Nexus binary on macOS, Windows and Linux without embedding a second Qt app,
//! decoder, CAT stack, transmitter, spot client or logbook.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

pub const UPSTREAM_REVISION: &str = "d960ae22de78940c6be9d95bd4817d233d02ee39";
pub const STANDARD_FT8_MHZ: &[(&str, f64)] = &[
    ("160m", 1.840),
    ("80m", 3.573),
    ("60m", 5.357),
    ("40m", 7.074),
    ("30m", 10.136),
    ("20m", 14.074),
    ("17m", 18.100),
    ("15m", 21.074),
    ("12m", 24.915),
    ("10m", 28.074),
    ("6m", 50.313),
];
const DEFAULT_CHASER_BANDS: &[&str] = &[
    "160m", "80m", "40m", "30m", "20m", "17m", "15m", "12m", "10m",
];
const EXTERNAL_DECISION_TTL_SECS: u64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum RunState {
    #[default]
    Idle,
    Listening,
    WaitingForQso,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SmsMode {
    #[default]
    Disabled,
    EveryTx,
    QsoCompleteOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AtnoChasePolicy {
    Disabled,
    AllTime,
    BandSlot,
    #[default]
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub enabled: bool,
    pub enabled_bands: Vec<String>,
    pub use_rbn: bool,
    pub use_psk_reporter: bool,
    pub use_dx_cluster: bool,
    pub us_spotters_only: bool,
    pub own_country_filter: bool,
    pub us_grid_only: bool,
    pub missed_cycles: u16,
    pub scarce_missed_cycles: u16,
    pub scarce_mode: bool,
    pub atno_hard_max_cycles: u16,
    pub scarce_threshold: u16,
    pub cooldown_secs: u64,
    pub atno_cooldown_secs: u64,
    pub decode_ttl_secs: u64,
    pub spot_ttl_secs: u64,
    pub cross_band_alert_cooldown_secs: u64,
    pub local_verification_secs: u64,
    pub nonstandard_qsy_cooldown_secs: u64,
    pub nonstandard_threshold_hz: u32,
    pub min_external_spotters: u16,
    pub slot_min_hz: u32,
    pub slot_max_hz: u32,
    pub slot_width_hz: u32,
    pub tx_offset_delta_hz: i32,
    pub score_new_dxcc: i32,
    pub score_new_band: i32,
    pub score_new_grid: i32,
    pub score_cq: i32,
    pub score_snr_per_db: i32,
    pub score_repeat: i32,
    pub score_spotter: i32,
    pub score_calling_me: i32,
    pub min_snr: i32,
    pub atno_chase_policy: AtnoChasePolicy,
    pub auto_band: bool,
    pub hunter_mode: bool,
    pub min_dwell_secs: u64,
    pub hysteresis: f64,
    pub quiet_spot_threshold: u32,
    pub pounce_cooldown_secs: u64,
    pub deferred_pounce_expire_secs: u64,
    pub max_deferred_pounces: usize,
    pub time_based_exclusions: bool,
    pub utc_offset_hours: i8,
    pub decode_history_retention_cycles: u16,
    pub auto_qsy_nonstandard: bool,
    pub sms_mode: SmsMode,
    pub sms_min_interval_secs: u64,
    pub quiet_hours_start: Option<u8>,
    pub quiet_hours_end: Option<u8>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            // Upstream deliberately excludes 60 m and 6 m from automatic operation.
            enabled_bands: DEFAULT_CHASER_BANDS
                .iter()
                .map(|band| (*band).into())
                .collect(),
            use_rbn: true,
            use_psk_reporter: true,
            use_dx_cluster: true,
            us_spotters_only: true,
            own_country_filter: true,
            us_grid_only: true,
            missed_cycles: 2,
            scarce_missed_cycles: 10,
            scarce_mode: true,
            atno_hard_max_cycles: 20,
            scarce_threshold: 5,
            cooldown_secs: 300,
            atno_cooldown_secs: 600,
            decode_ttl_secs: 120,
            spot_ttl_secs: 300,
            cross_band_alert_cooldown_secs: 120,
            local_verification_secs: 60,
            nonstandard_qsy_cooldown_secs: 30,
            nonstandard_threshold_hz: 3_000,
            min_external_spotters: 1,
            slot_min_hz: 500,
            slot_max_hz: 2_500,
            slot_width_hz: 60,
            tx_offset_delta_hz: 1_000,
            score_new_dxcc: 10_000,
            score_new_band: 1_000,
            score_new_grid: 500,
            score_cq: 200,
            score_snr_per_db: 10,
            score_repeat: 50,
            score_spotter: 25,
            score_calling_me: 200,
            min_snr: -20,
            atno_chase_policy: AtnoChasePolicy::Both,
            auto_band: true,
            hunter_mode: false,
            min_dwell_secs: 120,
            hysteresis: 0.25,
            quiet_spot_threshold: 3,
            pounce_cooldown_secs: 60,
            deferred_pounce_expire_secs: 120,
            max_deferred_pounces: 10,
            time_based_exclusions: true,
            utc_offset_hours: 0,
            decode_history_retention_cycles: 8,
            auto_qsy_nonstandard: true,
            sms_mode: SmsMode::Disabled,
            sms_min_interval_secs: 60,
            quiet_hours_start: None,
            quiet_hours_end: None,
        }
    }
}

impl Settings {
    pub fn normalized(mut self) -> Self {
        self.missed_cycles = self.missed_cycles.clamp(1, 100);
        self.scarce_missed_cycles = self.scarce_missed_cycles.clamp(1, 100);
        self.atno_hard_max_cycles = self.atno_hard_max_cycles.clamp(1, 1_000);
        self.scarce_threshold = self.scarce_threshold.min(20);
        self.cooldown_secs = self.cooldown_secs.clamp(1, 86_400);
        self.atno_cooldown_secs = self.atno_cooldown_secs.clamp(1, 604_800);
        self.decode_ttl_secs = self.decode_ttl_secs.clamp(1, 3_600);
        self.spot_ttl_secs = self.spot_ttl_secs.clamp(1, 3_600);
        self.cross_band_alert_cooldown_secs = self.cross_band_alert_cooldown_secs.clamp(1, 86_400);
        self.local_verification_secs = self.local_verification_secs.clamp(1, 3_600);
        self.nonstandard_qsy_cooldown_secs = self.nonstandard_qsy_cooldown_secs.clamp(1, 86_400);
        self.nonstandard_threshold_hz = self.nonstandard_threshold_hz.clamp(1, 100_000);
        self.min_external_spotters = self.min_external_spotters.clamp(1, 20);
        self.slot_min_hz = self.slot_min_hz.min(3_000);
        self.slot_max_hz = self.slot_max_hz.min(3_000);
        self.slot_width_hz = self.slot_width_hz.clamp(1, 1_000);
        self.tx_offset_delta_hz = self.tx_offset_delta_hz.clamp(-3_000, 3_000);
        self.score_new_dxcc = self.score_new_dxcc.clamp(0, 100_000);
        self.score_new_band = self.score_new_band.clamp(0, 100_000);
        self.score_new_grid = self.score_new_grid.clamp(0, 100_000);
        self.score_cq = self.score_cq.clamp(0, 100_000);
        self.score_snr_per_db = self.score_snr_per_db.clamp(0, 100_000);
        self.score_repeat = self.score_repeat.clamp(0, 100_000);
        self.score_spotter = self.score_spotter.clamp(0, 100_000);
        self.score_calling_me = self.score_calling_me.clamp(0, 100_000);
        self.min_dwell_secs = self.min_dwell_secs.clamp(30, 86_400);
        self.hysteresis = self.hysteresis.clamp(0.10, 0.50);
        self.quiet_spot_threshold = self.quiet_spot_threshold.clamp(1, 100_000);
        self.pounce_cooldown_secs = self.pounce_cooldown_secs.clamp(1, 86_400);
        self.deferred_pounce_expire_secs = self.deferred_pounce_expire_secs.clamp(1, 86_400);
        self.max_deferred_pounces = self.max_deferred_pounces.clamp(1, 10);
        self.utc_offset_hours = self.utc_offset_hours.clamp(-12, 14);
        self.decode_history_retention_cycles = self.decode_history_retention_cycles.clamp(2, 1_000);
        if self.slot_min_hz > self.slot_max_hz {
            std::mem::swap(&mut self.slot_min_hz, &mut self.slot_max_hz);
        }
        self.min_snr = self.min_snr.clamp(-30, 0);
        self.sms_min_interval_secs = self.sms_min_interval_secs.clamp(10, 3_600);
        self.quiet_hours_start = self.quiet_hours_start.filter(|hour| *hour < 24);
        self.quiet_hours_end = self.quiet_hours_end.filter(|hour| *hour < 24);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Decode {
    pub at: u64,
    pub call: String,
    pub band: String,
    pub dial_mhz: f64,
    pub audio_hz: u32,
    pub snr: i32,
    pub message: String,
    pub is_cq: bool,
    pub directed_to_me: bool,
    pub country: String,
    pub grid: Option<String>,
    pub new_dxcc: bool,
    pub new_band: bool,
    pub new_grid: bool,
    pub rarity_rank: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Spot {
    pub at: u64,
    pub call: String,
    pub band: String,
    pub freq_mhz: f64,
    pub source: String,
    pub spotter: String,
    pub mode: Option<String>,
    pub snr: Option<i32>,
    pub country: Option<String>,
    pub grid: Option<String>,
    pub new_dxcc: bool,
    pub new_band: bool,
    pub rarity_rank: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub call: String,
    pub band: String,
    pub dial_mhz: f64,
    pub audio_hz: u32,
    pub score: i32,
    pub rarity_multiplier: u8,
    pub decode_count: u16,
    pub spotter_count: u16,
    pub locally_decoded: bool,
    pub new_dxcc: bool,
    pub new_band: bool,
    pub new_grid: bool,
    pub is_cq: bool,
    pub directed_to_me: bool,
    pub snr: i32,
    pub country: String,
    pub grid: Option<String>,
    pub message: String,
    pub last_seen_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub candidate: Candidate,
    pub selected_at: u64,
    pub last_seen_at: u64,
    pub missed_cycles: u16,
    pub total_cycles: u16,
    pub last_cycle_at: u64,
    pub addressed_me: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BandActivity {
    pub band: String,
    pub decodes: u32,
    pub unique_calls: u32,
    pub spots: u32,
    pub score: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BandScoreInput {
    pub total_spots: u32,
    pub unique_dxcc: u32,
    pub needed_dxcc: u32,
    pub average_snr: f64,
    pub last_spot_at: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BandScoreWeights {
    pub spot_count: f64,
    pub unique_dxcc: f64,
    pub needed_dxcc: f64,
    pub signal_quality: f64,
    pub time_of_day: f64,
    pub grayline: f64,
    pub recency_decay: f64,
}

impl Default for BandScoreWeights {
    fn default() -> Self {
        Self {
            spot_count: 1.0,
            unique_dxcc: 2.0,
            needed_dxcc: 5.0,
            signal_quality: 1.5,
            time_of_day: 1.0,
            grayline: 2.0,
            recency_decay: 0.1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DecodePeriodStats {
    pub decode_count: u32,
    pub unique_callsigns: u32,
    pub new_dxcc_count: u32,
    pub new_grid_count: u32,
    pub average_snr: f64,
    pub max_snr: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct BandPerformance {
    pub recent_periods: Vec<DecodePeriodStats>,
    pub total_decodes: u32,
    pub total_periods: u32,
    pub average_decodes_per_period: f64,
    pub consecutive_empty_periods: u32,
    pub expected_spots: u32,
    pub actual_decodes: u32,
    pub performance_ratio: f64,
    pub current_penalty: f64,
    pub penalty_expiry: u64,
    pub hourly_performance: BTreeMap<u8, f64>,
}

impl BandPerformance {
    pub fn finish_period(
        &mut self,
        stats: DecodePeriodStats,
        expected_spots: u32,
        hour_utc: u8,
        now: u64,
    ) {
        self.actual_decodes = self.actual_decodes.saturating_add(stats.decode_count);
        self.expected_spots = expected_spots;
        self.consecutive_empty_periods = if stats.decode_count == 0 {
            self.consecutive_empty_periods.saturating_add(1)
        } else {
            0
        };
        self.recent_periods.push(stats);
        if self.recent_periods.len() > 20 {
            self.recent_periods.remove(0);
        }
        self.total_decodes = self
            .recent_periods
            .iter()
            .map(|period| period.decode_count)
            .sum();
        self.total_periods = self.recent_periods.len() as u32;
        self.average_decodes_per_period = if self.total_periods == 0 {
            0.0
        } else {
            f64::from(self.total_decodes) / f64::from(self.total_periods)
        };
        self.performance_ratio = if expected_spots == 0 {
            1.0
        } else {
            f64::from(self.actual_decodes) / f64::from(expected_spots)
        };
        let prior = self
            .hourly_performance
            .get(&(hour_utc % 24))
            .copied()
            .unwrap_or(0.0);
        self.hourly_performance.insert(
            hour_utc % 24,
            0.3 * self.average_decodes_per_period + 0.7 * prior,
        );
        if expected_spots > 0 && self.performance_ratio < 0.3 {
            let penalty = (1.0 - self.performance_ratio).clamp(0.0, 1.0);
            if penalty > self.current_penalty {
                self.current_penalty = penalty;
                self.penalty_expiry = now.saturating_add(600);
            }
        }
    }

    pub fn penalty_at(&self, now: u64) -> f64 {
        if self.penalty_expiry != 0 && now > self.penalty_expiry {
            0.0
        } else {
            self.current_penalty
        }
    }

    pub fn should_bailout(&self) -> bool {
        self.consecutive_empty_periods >= 2
            || (self.expected_spots >= 10
                && self.performance_ratio < 0.1
                && self.recent_periods.len() >= 2)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SlotAnalysis {
    pub suggested_hz: u32,
    pub alternate_hz: u32,
    pub confidence: f64,
    pub total_slots: u32,
    pub occupied_slots: u32,
    pub periods_collected: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MultistreamInfo {
    pub call: String,
    pub tx_frequencies: Vec<u32>,
    pub lowest_tx: u32,
    pub highest_tx: u32,
    pub is_multistream: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CallableRange {
    pub min_hz: u32,
    pub max_hz: u32,
    pub estimated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    pub source: String,
    pub items: u32,
    pub last_seen_at: u64,
    pub healthy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BandHistoryEntry {
    pub band: String,
    pub started_at: u64,
    pub ended_at: u64,
    pub decode_count: u32,
    pub unique_calls: u32,
    pub new_dxcc_count: u32,
    pub new_grid_count: u32,
    pub average_snr: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecodeHistoryEntry {
    pub call: String,
    pub grid: Option<String>,
    pub snr: i32,
    pub audio_hz: u32,
    pub last_heard_at: u64,
    pub times_heard: u16,
    pub is_cq: bool,
    pub new_dxcc: bool,
    pub new_grid: bool,
    pub periods_since_last_heard: u16,
    pub was_worked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PersistentState {
    pub version: u32,
    #[serde(rename = "statistics", skip_serializing)]
    pub legacy_statistics: Statistics,
    pub all_time_statistics: Statistics,
    pub band_history: Vec<BandHistoryEntry>,
    pub band_performance: BTreeMap<String, BandPerformance>,
}

#[derive(Debug, Clone)]
struct DeferredPounce {
    call: String,
    band: String,
    freq_mhz: f64,
    deferred_at: u64,
}

#[derive(Debug, Clone)]
struct PendingVerification {
    call: String,
    target_band: String,
    return_band: String,
    return_dial_mhz: f64,
    started_at: u64,
    expires_at: u64,
}

#[derive(Debug, Clone)]
struct Cooldown {
    at: u64,
    duration_secs: u64,
}

#[derive(Debug, Clone)]
struct DecodeObservation {
    at: u64,
    band: String,
    call: String,
    audio_hz: u32,
    snr: i32,
    new_dxcc: bool,
    new_grid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub at: u64,
    pub kind: String,
    pub call: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Statistics {
    pub started_at: Option<u64>,
    pub evaluated: u64,
    pub selected: u64,
    pub completed: u64,
    pub abandoned: u64,
    pub band_changes: u64,
    pub nonstandard_qsys: u64,
    pub sms_queued: u64,
    pub new_dxcc_worked: u64,
    pub new_band_slots: u64,
    pub new_grids: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SmsJob {
    pub body: String,
    pub attempts: u8,
    pub next_attempt_at: u64,
}

/// Deterministic Twilio outbox: one message per rate window, exponential retry, bounded
/// attempts. Network and credentials remain adapter concerns, so every transition is unit
/// testable without an account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmsOutbox {
    queue: VecDeque<SmsJob>,
    last_sent_at: Option<u64>,
    pub min_interval_secs: u64,
    pub max_attempts: u8,
    pub base_backoff_secs: u64,
}

impl Default for SmsOutbox {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            last_sent_at: None,
            min_interval_secs: 60,
            max_attempts: 3,
            base_backoff_secs: 30,
        }
    }
}

impl SmsOutbox {
    pub fn configure(&mut self, min_interval_secs: u64) {
        self.min_interval_secs = min_interval_secs.clamp(10, 3_600);
        self.max_attempts = 3;
    }
    pub fn enqueue(&mut self, body: String, now: u64) {
        if !body.trim().is_empty() {
            self.queue.push_back(SmsJob {
                body,
                attempts: 0,
                next_attempt_at: now,
            });
        }
    }
    pub fn due(&self, now: u64) -> Option<&SmsJob> {
        let rate_ready = self
            .last_sent_at
            .is_none_or(|at| now.saturating_sub(at) >= self.min_interval_secs);
        self.queue
            .front()
            .filter(|job| rate_ready && now >= job.next_attempt_at)
    }
    pub fn delivered(&mut self, now: u64) -> Option<SmsJob> {
        let job = self.queue.pop_front()?;
        self.last_sent_at = Some(now);
        Some(job)
    }
    pub fn failed(&mut self, now: u64) -> Option<SmsJob> {
        let job = self.queue.front_mut()?;
        job.attempts = job.attempts.saturating_add(1);
        if job.attempts >= self.max_attempts {
            return self.queue.pop_front();
        }
        let exponent = u32::from(job.attempts.saturating_sub(1));
        job.next_attempt_at = now.saturating_add(
            self.base_backoff_secs
                .saturating_mul(2_u64.saturating_pow(exponent)),
        );
        None
    }
    pub fn discard(&mut self) -> Option<SmsJob> {
        self.queue.pop_front()
    }
    pub fn len(&self) -> usize {
        self.queue.len()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    Call {
        call: String,
        band: String,
        grid: Option<String>,
        message: String,
        snr: i32,
        audio_hz: u32,
        tx_audio_hz: u32,
    },
    ChangeBand {
        band: String,
        dial_mhz: f64,
        reason: String,
    },
    ChangeDial {
        band: String,
        dial_mhz: f64,
        reason: String,
    },
    ReturnToStandard {
        band: String,
        dial_mhz: f64,
    },
    Notify {
        title: String,
        body: String,
    },
    Sms {
        body: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub upstream_revision: String,
    pub state: RunState,
    pub automation_armed: bool,
    pub last_error: Option<String>,
    pub settings: Settings,
    pub target: Option<Target>,
    pub candidates: Vec<Candidate>,
    pub bands: Vec<BandActivity>,
    pub session_statistics: Statistics,
    pub all_time_statistics: Statistics,
    pub band_history: Vec<BandHistoryEntry>,
    pub decode_history: Vec<DecodeHistoryEntry>,
    pub deferred_pounces: usize,
    pub target_cycles_remaining: Option<u16>,
    pub local_verification_remaining_secs: Option<u64>,
    pub sources: Vec<SourceStatus>,
    /// Distinct active entity-and-band pairs represented by external spots.
    pub active_spot_pairs: usize,
    pub log: Vec<LogEntry>,
    pub tx_guard: String,
}

#[derive(Debug, Clone)]
pub struct Chaser {
    settings: Settings,
    state: RunState,
    automation_armed: bool,
    last_error: Option<String>,
    my_country: String,
    my_grid: String,
    decodes: VecDeque<Decode>,
    decode_counts: HashMap<(String, String), u16>,
    spots: VecDeque<Spot>,
    target: Option<Target>,
    recently_worked: HashMap<String, Cooldown>,
    scarce_target_mode: bool,
    last_cross_band_alert_at: u64,
    last_nonstandard_qsy: u64,
    original_dial: Option<(String, f64)>,
    pending_verification: Option<PendingVerification>,
    current_band: Option<String>,
    current_band_since: u64,
    last_pounce_at: u64,
    deferred_pounces: VecDeque<DeferredPounce>,
    decode_history: HashMap<String, DecodeHistoryEntry>,
    band_history: VecDeque<BandHistoryEntry>,
    band_performance: BTreeMap<String, BandPerformance>,
    slot_observations: VecDeque<DecodeObservation>,
    feedback_cursor: BTreeMap<String, u64>,
    feedback_expected_spots: BTreeMap<String, u32>,
    statistics: Statistics,
    all_time_statistics: Statistics,
    operating_mode: String,
    log: VecDeque<LogEntry>,
}

impl Default for Chaser {
    fn default() -> Self {
        Self::new(Settings::default())
    }
}

impl Chaser {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings: settings.normalized(),
            state: RunState::Idle,
            automation_armed: false,
            last_error: None,
            my_country: String::new(),
            my_grid: String::new(),
            decodes: VecDeque::new(),
            decode_counts: HashMap::new(),
            spots: VecDeque::new(),
            target: None,
            recently_worked: HashMap::new(),
            scarce_target_mode: false,
            last_cross_band_alert_at: 0,
            last_nonstandard_qsy: 0,
            original_dial: None,
            pending_verification: None,
            current_band: None,
            current_band_since: 0,
            last_pounce_at: 0,
            deferred_pounces: VecDeque::new(),
            decode_history: HashMap::new(),
            band_history: VecDeque::new(),
            band_performance: BTreeMap::new(),
            slot_observations: VecDeque::new(),
            feedback_cursor: BTreeMap::new(),
            feedback_expected_spots: BTreeMap::new(),
            statistics: Statistics::default(),
            all_time_statistics: Statistics::default(),
            operating_mode: "FT8/FT4".into(),
            log: VecDeque::new(),
        }
    }
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    pub fn configure(&mut self, settings: Settings) {
        self.settings = settings.normalized();
        if !self.settings.enabled {
            self.stop(0, "disabled");
        }
    }
    pub fn set_operator_arm(&mut self, armed: bool, now: u64) {
        self.automation_armed = armed && self.settings.enabled;
        self.record(
            now,
            "arm",
            None,
            if self.automation_armed {
                "DX Chaser automation armed by operator"
            } else {
                "DX Chaser automation disarmed"
            },
        );
    }
    pub fn automation_armed(&self) -> bool {
        self.automation_armed
    }
    pub fn set_station(&mut self, country: impl Into<String>, grid: impl Into<String>) {
        self.my_country = country.into();
        self.my_grid = grid.into().to_uppercase();
    }
    pub fn set_operating_mode(&mut self, mode: impl Into<String>) {
        let mode = mode.into();
        self.operating_mode = if mode.trim().is_empty() {
            "FT8/FT4".into()
        } else {
            mode.trim().to_uppercase()
        };
    }
    pub fn start(&mut self, now: u64) {
        if self.settings.enabled {
            self.statistics.started_at.get_or_insert(now);
            self.all_time_statistics.started_at.get_or_insert(now);
            self.state = RunState::Listening;
            self.record(now, "state", None, "DX Chaser listening");
        }
    }
    pub fn stop(&mut self, now: u64, why: &str) {
        self.state = RunState::Idle;
        self.automation_armed = false;
        self.target = None;
        self.record(now, "state", None, why);
    }
    pub fn ingest_decode(&mut self, mut d: Decode) {
        d.call = normalize_call(&d.call);
        d.band = d.band.to_lowercase();
        d.country = d.country.trim().to_string();
        if d.call.is_empty() {
            return;
        }
        self.slot_observations.push_back(DecodeObservation {
            at: d.at,
            band: d.band.clone(),
            call: d.call.clone(),
            audio_hz: d.audio_hz,
            snr: d.snr,
            new_dxcc: d.new_dxcc,
            new_grid: d.new_grid,
        });
        let cache_key = (d.call.clone(), d.band.clone());
        if let Some(position) = self
            .decodes
            .iter()
            .position(|old| old.call == d.call && old.band == d.band)
        {
            let old = self
                .decodes
                .remove(position)
                .expect("decode position came from this deque");
            d.grid = d.grid.or(old.grid);
            if d.country.is_empty() {
                d.country = old.country;
            }
            if d.message.is_empty() {
                d.message = old.message;
            }
            d.new_dxcc |= old.new_dxcc;
            d.new_band |= old.new_band;
            d.new_grid |= old.new_grid;
            d.directed_to_me |= old.directed_to_me;
            d.rarity_rank = d.rarity_rank.or(old.rarity_rank);
        }
        self.decode_counts
            .entry(cache_key)
            .and_modify(|count| *count = count.saturating_add(1))
            .or_insert(1);
        let history = self
            .decode_history
            .entry(d.call.clone())
            .or_insert_with(|| DecodeHistoryEntry {
                call: d.call.clone(),
                grid: d.grid.clone(),
                snr: d.snr,
                audio_hz: d.audio_hz,
                last_heard_at: d.at,
                times_heard: 0,
                is_cq: d.is_cq,
                new_dxcc: d.new_dxcc,
                new_grid: d.new_grid,
                periods_since_last_heard: 0,
                was_worked: false,
            });
        history.grid = d.grid.clone().or_else(|| history.grid.clone());
        history.snr = d.snr;
        history.audio_hz = d.audio_hz;
        history.last_heard_at = d.at;
        history.times_heard = history.times_heard.saturating_add(1);
        history.is_cq = d.is_cq;
        history.new_dxcc |= d.new_dxcc;
        history.new_grid |= d.new_grid;
        history.periods_since_last_heard = 0;
        if let Some(t) = &mut self.target {
            if same_call(&t.candidate.call, &d.call) {
                t.last_seen_at = d.at;
                t.missed_cycles = 0;
                t.addressed_me |= d.directed_to_me;
            }
        }
        self.decodes.push_back(d);
    }
    pub fn ingest_spot(&mut self, mut s: Spot) {
        s.call = normalize_call(&s.call);
        s.band = s.band.to_lowercase();
        if !self.source_enabled(&s.source) {
            return;
        }
        if self.settings.us_spotters_only
            && !is_us_callsign(&s.spotter)
            && !is_rare_dx_callsign(&s.call)
        {
            return;
        }
        let duplicate = self.spots.iter().any(|old| {
            old.at == s.at
                && old.call == s.call
                && old.band == s.band
                && old.spotter.eq_ignore_ascii_case(&s.spotter)
        });
        if !s.call.is_empty() && !duplicate {
            self.spots
                .retain(|spot| s.at.saturating_sub(spot.at) <= self.settings.spot_ttl_secs);
            self.spots.push_back(s);
            while self.spots.len() > 3_000 {
                self.spots.pop_front();
            }
        }
    }
    fn source_enabled(&self, source: &str) -> bool {
        let source = source.to_ascii_lowercase();
        if source.contains("psk") {
            self.settings.use_psk_reporter
        } else if source.contains("rbn") || source.contains("reverse beacon") {
            self.settings.use_rbn
        } else if source.contains("cluster") || source == "dxc" {
            self.settings.use_dx_cluster
        } else {
            true
        }
    }
    pub fn observe_logged_qso(
        &mut self,
        now: u64,
        call: &str,
        entity: &str,
        band: &str,
        grid: Option<&str>,
    ) -> Vec<Action> {
        let call = normalize_call(call);
        self.recently_worked.insert(
            call_base(&call).to_string(),
            Cooldown {
                at: now,
                duration_secs: self.settings.cooldown_secs,
            },
        );
        if let Some(history) = self.decode_history.get_mut(&call) {
            history.was_worked = true;
        }
        self.statistics.completed += 1;
        self.all_time_statistics.completed += 1;
        self.record(now, "qso", Some(call.clone()), "logged FT QSO observed");
        if self.settings.sms_mode == SmsMode::Disabled {
            return Vec::new();
        }
        vec![Action::Sms {
            body: format_qso_sms(now, &call, entity, band, grid),
        }]
    }

    pub fn qso_complete(&mut self, now: u64, call: &str) -> Vec<Action> {
        let completed_target = self.target.clone();
        let (entity, band, grid) = completed_target.as_ref().map_or_else(
            || (String::new(), String::new(), None),
            |target| {
                (
                    target.candidate.country.clone(),
                    target.candidate.band.clone(),
                    target.candidate.grid.clone(),
                )
            },
        );
        self.qso_complete_with_facts(now, call, &entity, &band, grid.as_deref())
    }

    pub fn qso_complete_with_facts(
        &mut self,
        now: u64,
        call: &str,
        entity: &str,
        band: &str,
        grid: Option<&str>,
    ) -> Vec<Action> {
        if let Some(target) = &self.target {
            self.statistics.new_dxcc_worked += u64::from(target.candidate.new_dxcc);
            self.statistics.new_band_slots += u64::from(target.candidate.new_band);
            self.statistics.new_grids += u64::from(target.candidate.new_grid);
            self.all_time_statistics.new_dxcc_worked += u64::from(target.candidate.new_dxcc);
            self.all_time_statistics.new_band_slots += u64::from(target.candidate.new_band);
            self.all_time_statistics.new_grids += u64::from(target.candidate.new_grid);
        }
        let mut a = self.observe_logged_qso(now, call, entity, band, grid);
        self.target = None;
        self.pending_verification = None;
        self.state = if self.settings.enabled {
            RunState::Listening
        } else {
            RunState::Idle
        };
        if let Some((band, dial)) = self.original_dial.take() {
            a.push(Action::ReturnToStandard {
                band,
                dial_mhz: dial,
            });
        }
        a
    }
    /// Reconcile the selected target with the host application's Queue and Now lists.
    ///
    /// A present target needs no action. A missing engaged or scarce ATNO target is
    /// reissued from the cached candidate; an ordinary missing target is abandoned.
    /// Adapters should call this after a queue insertion attempt and once per decode
    /// period while a target is selected.
    pub fn observe_queue(&mut self, now: u64, present: bool) -> Vec<Action> {
        if present {
            return Vec::new();
        }
        let Some(target) = self.target.clone() else {
            return Vec::new();
        };
        let scarce = self.is_scarce_priority_target(&target, now);
        let reinsert = target.addressed_me
            || (self.settings.scarce_mode
                && (self.scarce_target_mode || (self.target_is_atno(&target) && scarce)));
        if reinsert && now.saturating_sub(target.last_seen_at) <= self.settings.decode_ttl_secs {
            self.record(
                now,
                "queue_reinsert",
                Some(target.candidate.call.clone()),
                if target.addressed_me {
                    "target engaged"
                } else {
                    "scarce target persistence"
                },
            );
            let candidate = target.candidate;
            return vec![Action::Call {
                call: candidate.call,
                band: candidate.band.clone(),
                grid: candidate.grid,
                message: candidate.message,
                snr: candidate.snr,
                audio_hz: candidate.audio_hz,
                tx_audio_hz: self.select_tx_slot(&candidate.band, candidate.audio_hz),
            }];
        }
        self.record(
            now,
            "queue_removed",
            Some(target.candidate.call),
            "target missing from Queue and Now",
        );
        self.target = None;
        self.scarce_target_mode = false;
        self.state = if self.settings.enabled {
            RunState::Listening
        } else {
            RunState::Idle
        };
        Vec::new()
    }
    pub fn cycle(
        &mut self,
        now: u64,
        current_band: &str,
        current_dial_mhz: f64,
        tx_armed: bool,
        qso_active: bool,
    ) -> Vec<Action> {
        self.prune(now);
        if !self.settings.enabled {
            self.state = RunState::Idle;
            return Vec::new();
        }
        if self.state == RunState::Idle {
            self.start(now);
        }
        self.observe_band(now, current_band);
        self.feed_completed_periods(now, current_band);
        if !self.automation_armed {
            return Vec::new();
        }
        if quiet_hour(
            now,
            self.settings.quiet_hours_start,
            self.settings.quiet_hours_end,
        ) {
            return Vec::new();
        }
        if qso_active {
            self.defer_best_pounce(now, current_band);
            self.state = RunState::WaitingForQso;
            return Vec::new();
        }
        if let Some(action) = self.release_deferred_pounce(now, current_band, current_dial_mhz) {
            return vec![action];
        }
        let scarce_now = self
            .target
            .as_ref()
            .is_some_and(|target| self.is_scarce_priority_target(target, now));
        if let Some(t) = &mut self.target {
            t.total_cycles = t.total_cycles.saturating_add(1);
            if t.last_seen_at > t.last_cycle_at {
                t.missed_cycles = 0;
            } else {
                t.missed_cycles = t.missed_cycles.saturating_add(1);
            }
            t.last_cycle_at = now;
            let extended = match self.settings.atno_chase_policy {
                AtnoChasePolicy::Disabled => false,
                AtnoChasePolicy::AllTime => t.candidate.new_dxcc,
                AtnoChasePolicy::BandSlot => t.candidate.new_band,
                AtnoChasePolicy::Both => t.candidate.new_dxcc || t.candidate.new_band,
            };
            let max = if self.settings.scarce_mode && extended && scarce_now {
                self.settings.atno_hard_max_cycles
            } else if self.settings.scarce_mode
                && (extended || self.scarce_target_mode || scarce_now)
            {
                self.settings
                    .scarce_missed_cycles
                    .saturating_mul(u16::from(t.candidate.rarity_multiplier))
            } else {
                self.settings.missed_cycles
            };
            if self.settings.scarce_mode && (extended || self.scarce_target_mode || scarce_now) {
                self.scarce_target_mode = true;
            }
            let heard_recently = now.saturating_sub(t.last_seen_at) <= 60;
            let atno_limit = extended && t.total_cycles >= self.settings.atno_hard_max_cycles;
            let missed_limit = t.missed_cycles >= max;
            let should_abandon =
                atno_limit || (!t.addressed_me && (!heard_recently || missed_limit));
            if !should_abandon {
                self.state = RunState::WaitingForQso;
                return Vec::new();
            }
            let call = t.candidate.call.clone();
            if atno_limit && heard_recently {
                self.recently_worked.insert(
                    call_base(&call).to_string(),
                    Cooldown {
                        at: now,
                        duration_secs: self.settings.atno_cooldown_secs,
                    },
                );
            }
            self.statistics.abandoned += 1;
            self.all_time_statistics.abandoned += 1;
            self.record(now, "abandon", Some(call), "target missed cycle limit");
            self.target = None;
        }
        self.state = RunState::Listening;
        let mut candidates = self.candidates(now);
        if let Some(verification) = self.pending_verification.clone() {
            let verified = candidates.iter().any(|candidate| {
                same_call(&candidate.call, &verification.call)
                    && candidate
                        .band
                        .eq_ignore_ascii_case(&verification.target_band)
                    && candidate.last_seen_at > verification.started_at
            });
            if verified {
                self.pending_verification = None;
                candidates.retain(|candidate| {
                    same_call(&candidate.call, &verification.call)
                        && candidate
                            .band
                            .eq_ignore_ascii_case(&verification.target_band)
                });
            } else if now > verification.expires_at {
                self.pending_verification = None;
                self.original_dial = None;
                return vec![Action::ReturnToStandard {
                    band: verification.return_band,
                    dial_mhz: verification.return_dial_mhz,
                }];
            } else {
                return Vec::new();
            }
        }
        if let Some(action) = self.same_band_nonstandard_action(now, current_band, current_dial_mhz)
        {
            return vec![action];
        }
        self.statistics.evaluated += candidates.len() as u64;
        self.all_time_statistics.evaluated += candidates.len() as u64;
        let Some(best) = candidates.first().cloned() else {
            return self.cross_band_actions(now, current_band, current_dial_mhz);
        };
        if best.band != current_band.to_lowercase() {
            return self.cross_band_actions(now, current_band, current_dial_mhz);
        }
        if !tx_armed {
            return vec![Action::Notify {
                title: "DX Chaser ready".into(),
                body: format!(
                    "{} is the best local target; Enable TX to let DX Chaser call",
                    best.call
                ),
            }];
        }
        let standard = standard_dial(&best.band).unwrap_or(current_dial_mhz);
        if self.settings.auto_qsy_nonstandard
            && (best.dial_mhz - standard).abs() * 1_000_000.0
                > f64::from(self.settings.nonstandard_threshold_hz)
            && (best.dial_mhz - current_dial_mhz).abs() * 1_000_000.0 > 100.0
            && now.saturating_sub(self.last_nonstandard_qsy)
                >= self.settings.nonstandard_qsy_cooldown_secs
        {
            self.last_nonstandard_qsy = now;
            self.original_dial = Some((best.band.clone(), standard));
            self.pending_verification = Some(PendingVerification {
                call: best.call.clone(),
                target_band: best.band.clone(),
                return_band: best.band.clone(),
                return_dial_mhz: standard,
                started_at: now,
                expires_at: now.saturating_add(self.settings.local_verification_secs),
            });
            self.statistics.nonstandard_qsys += 1;
            self.all_time_statistics.nonstandard_qsys += 1;
            return vec![Action::ChangeDial {
                band: best.band.clone(),
                dial_mhz: best.dial_mhz,
                reason: format!("locally verify {} on non-standard frequency", best.call),
            }];
        }
        self.statistics.selected += 1;
        self.all_time_statistics.selected += 1;
        self.target = Some(Target {
            candidate: best.clone(),
            selected_at: now,
            last_seen_at: best.last_seen_at,
            missed_cycles: 0,
            total_cycles: 0,
            last_cycle_at: now,
            addressed_me: best.directed_to_me,
        });
        self.scarce_target_mode = false;
        self.state = RunState::WaitingForQso;
        self.record(
            now,
            "target",
            Some(best.call.clone()),
            &format!("selected score {}", best.score),
        );
        let tx_sms = (self.settings.sms_mode == SmsMode::EveryTx)
            .then(|| format_tx_sms(now, &best.call, &best.band, &self.operating_mode));
        let mut out = vec![Action::Call {
            call: best.call,
            band: best.band.clone(),
            grid: best.grid,
            message: best.message,
            snr: best.snr,
            audio_hz: best.audio_hz,
            tx_audio_hz: self.select_tx_slot(&best.band, best.audio_hz),
        }];
        if let Some(body) = tx_sms {
            out.push(Action::Sms { body });
        }
        out
    }
    pub fn candidates(&self, now: u64) -> Vec<Candidate> {
        let mut grouped: BTreeMap<(String, String), Vec<&Decode>> = BTreeMap::new();
        for d in &self.decodes {
            if self.eligible(d, now) {
                grouped
                    .entry((d.call.clone(), d.band.clone()))
                    .or_default()
                    .push(d);
            }
        }
        let mut out = Vec::new();
        for ((_call, _band), ds) in grouped {
            let d = ds.last().expect("non-empty");
            let decode_count = *self
                .decode_counts
                .get(&(d.call.clone(), d.band.clone()))
                .unwrap_or(&1);
            let spotters: HashSet<&str> = self
                .spots
                .iter()
                .filter(|s| {
                    same_call(&s.call, &d.call)
                        && s.band == d.band
                        && now.saturating_sub(s.at) <= EXTERNAL_DECISION_TTL_SECS
                })
                .map(|s| s.spotter.as_str())
                .collect();
            let spotter_count = spotters.len().min(20) as u16;
            let new_dxcc = ds.iter().any(|decode| decode.new_dxcc);
            let new_band = ds.iter().any(|decode| decode.new_band);
            let new_grid = ds.iter().any(|decode| decode.new_grid);
            let directed_to_me = ds.iter().any(|decode| decode.directed_to_me);
            let rarity = ds
                .iter()
                .filter_map(|decode| decode.rarity_rank)
                .min()
                .map_or(1, |rank| rarity_multiplier(Some(rank)));
            // TargetScorer uses an exclusive need ladder.  A station is a candidate
            // only because it satisfies one enabled need; activity bonuses cannot
            // turn an already-worked station into a target.
            let need_score = if new_dxcc {
                self.settings.score_new_dxcc * rarity as i32
            } else if new_band {
                self.settings.score_new_band
            } else if new_grid {
                self.settings.score_new_grid
            } else {
                0
            };
            if need_score == 0 {
                continue;
            }
            let score = need_score
                + (if d.is_cq { self.settings.score_cq } else { 0 })
                + ((d.snr + 15).max(0) * self.settings.score_snr_per_db)
                + (i32::from(decode_count.saturating_sub(1)) * self.settings.score_repeat)
                + (i32::from(spotter_count) * self.settings.score_spotter)
                + (if directed_to_me {
                    self.settings.score_calling_me
                } else {
                    0
                });
            out.push(Candidate {
                call: d.call.clone(),
                band: d.band.clone(),
                dial_mhz: d.dial_mhz,
                audio_hz: d.audio_hz,
                score,
                rarity_multiplier: rarity,
                decode_count,
                spotter_count,
                locally_decoded: true,
                new_dxcc,
                new_band,
                new_grid,
                is_cq: d.is_cq,
                directed_to_me,
                snr: d.snr,
                country: d.country.clone(),
                grid: d.grid.clone(),
                message: d.message.clone(),
                last_seen_at: d.at,
            });
        }
        out.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| b.snr.cmp(&a.snr))
                .then_with(|| a.call.cmp(&b.call))
        });
        out
    }
    pub fn band_activity(&self) -> Vec<BandActivity> {
        let mut m: BTreeMap<String, (u32, HashSet<String>, u32)> = BTreeMap::new();
        for d in &self.decodes {
            let e = m.entry(d.band.clone()).or_default();
            e.0 += 1;
            e.1.insert(d.call.clone());
        }
        for s in &self.spots {
            m.entry(s.band.clone()).or_default().2 += 1;
        }
        m.into_iter()
            .map(|(band, (decodes, calls, spots))| BandActivity {
                band,
                decodes,
                unique_calls: calls.len() as u32,
                spots,
                score: (decodes as i32 * 10) + (calls.len() as i32 * 25) + (spots as i32 * 5),
            })
            .collect()
    }
    pub fn best_free_slot(&self, band: &str) -> u32 {
        self.slot_analysis(band, None).suggested_hz
    }
    pub fn slot_analysis(&self, band: &str, range: Option<CallableRange>) -> SlotAnalysis {
        let decodes: Vec<_> = self
            .slot_observations
            .iter()
            .filter(|observation| observation.band.eq_ignore_ascii_case(band))
            .collect();
        let periods: HashSet<_> = decodes
            .iter()
            .map(|observation| observation.at / 15)
            .collect();
        let (min_hz, max_hz) = range
            .map(|value| (value.min_hz, value.max_hz))
            .unwrap_or((self.settings.slot_min_hz, self.settings.slot_max_hz));
        slot_analysis_from_decodes(
            decodes.into_iter().map(|observation| observation.audio_hz),
            periods.len().min(u16::MAX as usize) as u16,
            min_hz,
            max_hz,
            self.settings.slot_width_hz,
        )
    }
    pub fn detect_multistream(&self, band: &str, call: &str) -> MultistreamInfo {
        let call = normalize_call(call);
        let mut frequencies: Vec<_> = self
            .slot_observations
            .iter()
            .filter(|observation| {
                observation.band.eq_ignore_ascii_case(band)
                    && normalize_call(&observation.call) == call
            })
            .map(|observation| round_to_slot(observation.audio_hz, self.settings.slot_width_hz))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        frequencies.sort_unstable();
        MultistreamInfo {
            call,
            lowest_tx: frequencies.first().copied().unwrap_or(0),
            highest_tx: frequencies.last().copied().unwrap_or(0),
            is_multistream: frequencies.len() >= 2,
            tx_frequencies: frequencies,
        }
    }
    pub fn select_tx_slot(&self, band: &str, target_hz: u32) -> u32 {
        let preferred = i64::from(target_hz) + i64::from(self.settings.tx_offset_delta_hz);
        let preferred = preferred.clamp(300, 2_700) as u32;
        let occupied = self.decodes.iter().any(|decode| {
            decode.band.eq_ignore_ascii_case(band)
                && decode.audio_hz.abs_diff(preferred) < self.settings.slot_width_hz
        });
        if occupied {
            self.best_free_slot(band)
        } else {
            preferred
        }
    }
    pub fn snapshot(&self, now: u64, tx_armed: bool) -> Snapshot {
        let mut decode_history: Vec<_> = self.decode_history.values().cloned().collect();
        decode_history.sort_by(|a, b| {
            b.new_dxcc
                .cmp(&a.new_dxcc)
                .then_with(|| b.times_heard.cmp(&a.times_heard))
                .then_with(|| b.snr.cmp(&a.snr))
                .then_with(|| b.new_grid.cmp(&a.new_grid))
                .then_with(|| a.call.cmp(&b.call))
        });
        Snapshot {
            upstream_revision: UPSTREAM_REVISION.into(),
            state: self.state,
            automation_armed: self.automation_armed,
            last_error: self.last_error.clone(),
            settings: self.settings.clone(),
            target: self.target.clone(),
            candidates: self.candidates(now),
            bands: self.band_activity(),
            session_statistics: self.statistics.clone(),
            all_time_statistics: self.all_time_statistics.clone(),
            band_history: self.band_history.iter().cloned().collect(),
            decode_history,
            deferred_pounces: self.deferred_pounces.len(),
            target_cycles_remaining: self.target.as_ref().map(|target| {
                let used = if self.target_is_atno(target) {
                    target.total_cycles
                } else {
                    target.missed_cycles
                };
                self.target_cycle_budget(target).saturating_sub(used)
            }),
            local_verification_remaining_secs: self
                .pending_verification
                .as_ref()
                .map(|verification| verification.expires_at.saturating_sub(now)),
            sources: self.source_status(now),
            active_spot_pairs: self
                .spots
                .iter()
                .filter(|spot| now.saturating_sub(spot.at) <= self.settings.spot_ttl_secs)
                .map(|spot| {
                    (
                        spot.country
                            .as_deref()
                            .filter(|country| !country.is_empty())
                            .unwrap_or(&spot.call)
                            .to_ascii_uppercase(),
                        spot.band.to_ascii_lowercase(),
                    )
                })
                .collect::<HashSet<_>>()
                .len(),
            log: self.log.iter().cloned().collect(),
            tx_guard: if tx_armed {
                "armed by operator"
            } else {
                "Enable TX is off; DX Chaser cannot call"
            }
            .into(),
        }
    }
    pub fn persistent_state(&self) -> PersistentState {
        PersistentState {
            version: 3,
            legacy_statistics: Statistics::default(),
            all_time_statistics: self.all_time_statistics.clone(),
            band_history: self.band_history.iter().cloned().collect(),
            band_performance: self.band_performance.clone(),
        }
    }
    pub fn restore_persistent_state(&mut self, state: PersistentState) {
        if matches!(state.version, 1 | 2 | 3) {
            self.all_time_statistics = if state.version >= 3 {
                state.all_time_statistics
            } else {
                state.legacy_statistics
            };
            self.band_history = state.band_history.into_iter().collect();
            self.band_history.truncate(500);
            self.band_performance = state.band_performance;
        }
    }
    pub fn record_band_period(
        &mut self,
        band: &str,
        stats: DecodePeriodStats,
        expected_spots: u32,
        hour_utc: u8,
        now: u64,
    ) {
        self.band_performance
            .entry(band.to_ascii_lowercase())
            .or_default()
            .finish_period(stats, expected_spots, hour_utc, now);
    }
    pub fn best_bailout_band(&self, current_band: &str, hour_utc: u8, now: u64) -> Option<String> {
        self.band_performance
            .iter()
            .filter(|(band, _)| !band.eq_ignore_ascii_case(current_band))
            .max_by(|(_, left), (_, right)| {
                let score = |performance: &BandPerformance| {
                    (1.0 - performance.penalty_at(now))
                        * (1.0
                            + performance
                                .hourly_performance
                                .get(&(hour_utc % 24))
                                .copied()
                                .unwrap_or(0.0))
                };
                score(left).total_cmp(&score(right))
            })
            .map(|(band, _)| band.clone())
    }
    fn feed_completed_periods(&mut self, now: u64, band: &str) {
        let band = band.to_ascii_lowercase();
        let current_period = now / 15;
        let first_observed = self
            .slot_observations
            .iter()
            .filter(|observation| observation.band.eq_ignore_ascii_case(&band))
            .map(|observation| observation.at / 15)
            .min();
        let mut cursor = self
            .feedback_cursor
            .get(&band)
            .copied()
            .unwrap_or_else(|| first_observed.unwrap_or(current_period));
        while cursor < current_period {
            let observations: Vec<_> = self
                .slot_observations
                .iter()
                .filter(|observation| {
                    observation.band.eq_ignore_ascii_case(&band) && observation.at / 15 == cursor
                })
                .collect();
            let unique_calls: HashSet<_> = observations
                .iter()
                .map(|observation| observation.call.as_str())
                .collect();
            let snr_total: i64 = observations
                .iter()
                .map(|observation| i64::from(observation.snr))
                .sum();
            let stats = DecodePeriodStats {
                decode_count: observations.len() as u32,
                unique_callsigns: unique_calls.len() as u32,
                new_dxcc_count: observations
                    .iter()
                    .filter(|observation| observation.new_dxcc)
                    .count() as u32,
                new_grid_count: observations
                    .iter()
                    .filter(|observation| observation.new_grid)
                    .count() as u32,
                average_snr: if observations.is_empty() {
                    0.0
                } else {
                    snr_total as f64 / observations.len() as f64
                },
                max_snr: observations
                    .iter()
                    .map(|observation| observation.snr)
                    .max()
                    .map_or(0.0, f64::from),
            };
            let period_end = cursor.saturating_add(1).saturating_mul(15);
            let expected_spots = self
                .feedback_expected_spots
                .get(&band)
                .copied()
                .unwrap_or(0);
            self.record_band_period(
                &band,
                stats,
                expected_spots,
                ((period_end / 3_600) % 24) as u8,
                period_end,
            );
            cursor = cursor.saturating_add(1);
        }
        self.feedback_cursor.insert(band, current_period);
    }

    fn auto_band_score(&self, band: &str, now: u64) -> f64 {
        let spots: Vec<_> = self
            .spots
            .iter()
            .filter(|spot| {
                spot.band.eq_ignore_ascii_case(band)
                    && now.saturating_sub(spot.at) <= EXTERNAL_DECISION_TTL_SECS
            })
            .collect();
        let mut unique_entities = HashSet::new();
        let mut needed_entities = HashSet::new();
        let mut snr_total = 0_i64;
        let mut snr_count = 0_u32;
        let mut last_spot_at = None;
        for spot in &spots {
            let entity = spot
                .country
                .as_deref()
                .filter(|country| !country.trim().is_empty())
                .unwrap_or_else(|| call_base(&spot.call));
            unique_entities.insert(entity.to_ascii_uppercase());
            if spot.new_dxcc {
                needed_entities.insert(entity.to_ascii_uppercase());
            }
            if let Some(snr) = spot.snr {
                snr_total += i64::from(snr);
                snr_count = snr_count.saturating_add(1);
            }
            last_spot_at = Some(last_spot_at.map_or(spot.at, |last: u64| last.max(spot.at)));
        }
        let performance = self.band_performance.get(&band.to_ascii_lowercase());
        let hour = ((now / 3_600) % 24) as u8;
        hv_band_score(
            BandScoreInput {
                total_spots: spots.len() as u32,
                unique_dxcc: unique_entities.len() as u32,
                needed_dxcc: needed_entities.len() as u32,
                average_snr: if snr_count == 0 {
                    0.0
                } else {
                    snr_total as f64 / f64::from(snr_count)
                },
                last_spot_at,
            },
            band,
            hour,
            now,
            self.settings.hunter_mode,
            performance.map_or(0.0, |value| value.penalty_at(now)),
            performance
                .and_then(|value| value.hourly_performance.get(&hour))
                .copied()
                .unwrap_or(0.0),
            BandScoreWeights::default(),
        )
    }
    pub fn reset_statistics(&mut self, now: u64) {
        self.statistics = Statistics::default();
        if self.settings.enabled {
            self.statistics.started_at = Some(now);
        }
    }
    /// Count an SMS only after the platform adapter accepts it into a configured outbox.
    pub fn sms_queued(&mut self) {
        self.statistics.sms_queued = self.statistics.sms_queued.saturating_add(1);
        self.all_time_statistics.sms_queued = self.all_time_statistics.sms_queued.saturating_add(1);
    }
    pub fn action_failed(&mut self, now: u64, detail: impl Into<String>) {
        let detail = detail.into();
        self.last_error = Some(detail.clone());
        self.target = None;
        self.state = if self.settings.enabled {
            RunState::Listening
        } else {
            RunState::Idle
        };
        self.automation_armed = false;
        self.record(now, "error", None, &detail);
    }
    pub fn clear_error(&mut self) {
        self.last_error = None;
    }
    fn eligible(&self, d: &Decode, now: u64) -> bool {
        self.settings
            .enabled_bands
            .iter()
            .any(|b| b.eq_ignore_ascii_case(&d.band))
            && now.saturating_sub(d.at) <= self.settings.decode_ttl_secs
            && d.snr >= self.settings.min_snr
            && self
                .recently_worked
                .get(call_base(&d.call))
                .is_none_or(|cooldown| now.saturating_sub(cooldown.at) > cooldown.duration_secs)
            && (!self.settings.own_country_filter
                || self.my_country.is_empty()
                || !d.country.eq_ignore_ascii_case(&self.my_country))
            && (!self.settings.us_grid_only
                || us_grid_allowed(self.my_grid.as_str(), d.grid.as_deref()))
    }
    fn target_cycle_budget(&self, target: &Target) -> u16 {
        if self.target_is_atno(target) {
            self.settings.atno_hard_max_cycles
        } else if self.settings.scarce_mode && target.candidate.rarity_multiplier > 1 {
            self.settings
                .scarce_missed_cycles
                .saturating_mul(u16::from(target.candidate.rarity_multiplier))
        } else {
            self.settings.missed_cycles
        }
    }
    fn target_is_atno(&self, target: &Target) -> bool {
        match self.settings.atno_chase_policy {
            AtnoChasePolicy::Disabled => false,
            AtnoChasePolicy::AllTime => target.candidate.new_dxcc,
            AtnoChasePolicy::BandSlot => target.candidate.new_band,
            AtnoChasePolicy::Both => target.candidate.new_dxcc || target.candidate.new_band,
        }
    }
    fn is_scarce_priority_target(&self, target: &Target, now: u64) -> bool {
        let mut viable = 0_u16;
        for decode in &self.decodes {
            if decode.band != target.candidate.band
                || same_call(&decode.call, &target.candidate.call)
                || now.saturating_sub(decode.at) > self.settings.decode_ttl_secs
                || (!decode.new_dxcc && !decode.new_band)
            {
                continue;
            }
            viable = viable.saturating_add(1);
            if viable > self.settings.scarce_threshold {
                return false;
            }
        }
        true
    }
    fn source_status(&self, now: u64) -> Vec<SourceStatus> {
        let mut sources: BTreeMap<String, (u32, u64)> = BTreeMap::new();
        if let Some(last) = self.decodes.back() {
            sources.insert("Local decoder".into(), (self.decodes.len() as u32, last.at));
        }
        for spot in &self.spots {
            let entry = sources.entry(spot.source.clone()).or_default();
            entry.0 = entry.0.saturating_add(1);
            entry.1 = entry.1.max(spot.at);
        }
        sources
            .into_iter()
            .map(|(source, (items, last_seen_at))| {
                let ttl = if source == "Local decoder" {
                    self.settings.decode_ttl_secs
                } else {
                    self.settings.spot_ttl_secs
                };
                SourceStatus {
                    source,
                    items,
                    last_seen_at,
                    healthy: now.saturating_sub(last_seen_at) <= ttl,
                }
            })
            .collect()
    }

    fn same_band_nonstandard_action(
        &mut self,
        now: u64,
        current_band: &str,
        current_dial_mhz: f64,
    ) -> Option<Action> {
        if !self.settings.auto_qsy_nonstandard
            || (self.last_nonstandard_qsy != 0
                && now.saturating_sub(self.last_nonstandard_qsy)
                    < self.settings.nonstandard_qsy_cooldown_secs)
        {
            return None;
        }
        let standard = standard_dial(current_band).unwrap_or(current_dial_mhz);
        let spot = self
            .spots
            .iter()
            .rev()
            .find(|spot| {
                spot.band.eq_ignore_ascii_case(current_band)
                    && (spot.new_dxcc || spot.new_band)
                    && now.saturating_sub(spot.at) <= EXTERNAL_DECISION_TTL_SECS
                    && (external_target_dial(&spot.call, spot.freq_mhz) - standard).abs()
                        * 1_000_000.0
                        > f64::from(self.settings.nonstandard_threshold_hz)
            })?
            .clone();
        let target_dial = external_target_dial(&spot.call, spot.freq_mhz);
        if (target_dial - current_dial_mhz).abs() * 1_000_000.0
            <= f64::from(self.settings.nonstandard_threshold_hz)
        {
            return None;
        }
        self.last_nonstandard_qsy = now;
        self.original_dial = Some((current_band.to_lowercase(), standard));
        self.pending_verification = Some(PendingVerification {
            call: spot.call.clone(),
            target_band: current_band.to_lowercase(),
            return_band: current_band.to_lowercase(),
            return_dial_mhz: standard,
            started_at: now,
            expires_at: now.saturating_add(self.settings.local_verification_secs),
        });
        self.statistics.nonstandard_qsys += 1;
        self.all_time_statistics.nonstandard_qsys += 1;
        Some(Action::ChangeDial {
            band: current_band.to_lowercase(),
            dial_mhz: target_dial,
            reason: format!("locally verify {} on non-standard frequency", spot.call),
        })
    }

    fn cross_band_actions(
        &mut self,
        now: u64,
        current_band: &str,
        current_dial_mhz: f64,
    ) -> Vec<Action> {
        if !self.settings.auto_band {
            return Vec::new();
        }
        let choices: Vec<_> = self
            .candidates(now)
            .into_iter()
            .filter(|c| {
                c.band != current_band.to_lowercase() && self.auto_band_allowed(&c.band, now)
            })
            .collect();
        let mut nominations: Vec<_> = choices
            .into_iter()
            .map(|candidate| {
                (
                    candidate.call.clone(),
                    candidate.band.clone(),
                    candidate.dial_mhz,
                    candidate.spotter_count,
                    candidate.new_dxcc,
                    candidate.score,
                    false,
                )
            })
            .collect();
        nominations.extend(
            self.external_band_opportunities(now, current_band)
                .into_iter()
                .map(|(call, band, freq, spotters, atno, score)| {
                    (call, band, freq, spotters, atno, score, true)
                }),
        );
        let hour = ((now / 3_600) % 24) as u8;
        let bailout_band = self
            .band_performance
            .get(&current_band.to_ascii_lowercase())
            .filter(|performance| performance.should_bailout())
            .and_then(|_| self.best_bailout_band(current_band, hour, now));
        nominations.sort_by(|left, right| {
            let hunter_left = self.settings.hunter_mode && left.4;
            let hunter_right = self.settings.hunter_mode && right.4;
            hunter_right
                .cmp(&hunter_left)
                .then_with(|| {
                    let preferred = |band: &str| {
                        bailout_band
                            .as_ref()
                            .is_some_and(|preferred| preferred.eq_ignore_ascii_case(band))
                    };
                    preferred(&right.1).cmp(&preferred(&left.1))
                })
                .then_with(|| {
                    self.auto_band_score(&right.1, now)
                        .total_cmp(&self.auto_band_score(&left.1, now))
                })
                .then_with(|| right.5.cmp(&left.5))
        });
        let selected = nominations.first().cloned();
        let Some((call, band, freq_mhz, spotter_count, new_dxcc, _target_score, external)) =
            selected
        else {
            return Vec::new();
        };
        let confidence = if new_dxcc {
            1
        } else {
            self.settings.min_external_spotters
        };
        if spotter_count < confidence {
            return Vec::new();
        }
        let hunter_pounce = self.settings.hunter_mode && new_dxcc;
        if hunter_pounce {
            if self.last_pounce_at != 0
                && now.saturating_sub(self.last_pounce_at) < self.settings.pounce_cooldown_secs
            {
                return Vec::new();
            }
        } else {
            if now.saturating_sub(self.current_band_since) < self.settings.min_dwell_secs {
                return Vec::new();
            }
            let current_spots = self
                .spots
                .iter()
                .filter(|spot| {
                    spot.band.eq_ignore_ascii_case(current_band)
                        && now.saturating_sub(spot.at) <= EXTERNAL_DECISION_TTL_SECS
                })
                .count() as u32;
            let current_score = self.auto_band_score(current_band, now);
            let selected_band_score = self.auto_band_score(&band, now);
            let quiet = current_spots < self.settings.quiet_spot_threshold;
            let clears_hysteresis = selected_band_score
                > current_score * (1.0 + self.settings.hysteresis.clamp(0.10, 0.50));
            if !quiet && !clears_hysteresis {
                return Vec::new();
            }
        }
        if self.last_cross_band_alert_at != 0
            && now.saturating_sub(self.last_cross_band_alert_at)
                < self.settings.cross_band_alert_cooldown_secs
        {
            return Vec::new();
        }
        self.last_cross_band_alert_at = now;
        if hunter_pounce {
            self.last_pounce_at = now;
        }
        let target_dial = if external {
            external_target_dial(&call, freq_mhz)
        } else {
            freq_mhz
        };
        self.original_dial = Some((current_band.to_lowercase(), current_dial_mhz));
        self.pending_verification = Some(PendingVerification {
            call: call.clone(),
            target_band: band.clone(),
            return_band: current_band.to_lowercase(),
            return_dial_mhz: current_dial_mhz,
            started_at: now,
            expires_at: now.saturating_add(self.settings.local_verification_secs),
        });
        self.statistics.band_changes += 1;
        self.all_time_statistics.band_changes += 1;
        vec![Action::ChangeBand {
            band: band.clone(),
            dial_mhz: target_dial,
            reason: format!("verify {call} locally before calling"),
        }]
    }

    fn observe_band(&mut self, now: u64, current_band: &str) {
        let current_band = current_band.to_lowercase();
        let Some(previous) = self.current_band.clone() else {
            self.current_band = Some(current_band.clone());
            self.current_band_since = now;
            self.begin_band_feedback(now, &current_band);
            return;
        };
        if previous == current_band {
            return;
        }
        self.feed_completed_periods(now, &previous);
        if self
            .pending_verification
            .as_ref()
            .is_some_and(|pending| !pending.target_band.eq_ignore_ascii_case(&current_band))
        {
            self.pending_verification = None;
            self.original_dial = None;
        }
        let samples: Vec<_> = self
            .decodes
            .iter()
            .filter(|decode| decode.band == previous && decode.at >= self.current_band_since)
            .collect();
        let unique_calls: HashSet<_> = samples.iter().map(|decode| &decode.call).collect();
        let snr_total: i64 = samples.iter().map(|decode| i64::from(decode.snr)).sum();
        self.band_history.push_front(BandHistoryEntry {
            band: previous.clone(),
            started_at: self.current_band_since,
            ended_at: now,
            decode_count: samples.len() as u32,
            unique_calls: unique_calls.len() as u32,
            new_dxcc_count: samples.iter().filter(|decode| decode.new_dxcc).count() as u32,
            new_grid_count: samples.iter().filter(|decode| decode.new_grid).count() as u32,
            average_snr: if samples.is_empty() {
                0.0
            } else {
                snr_total as f64 / samples.len() as f64
            },
        });
        self.band_history.truncate(500);
        self.decodes
            .retain(|decode| !decode.band.eq_ignore_ascii_case(&previous));
        self.decode_counts
            .retain(|(_, band), _| !band.eq_ignore_ascii_case(&previous));
        self.decode_history.clear();
        self.slot_observations
            .retain(|observation| !observation.band.eq_ignore_ascii_case(&previous));
        self.current_band = Some(current_band);
        self.current_band_since = now;
        let current = self.current_band.clone().unwrap_or_default();
        self.begin_band_feedback(now, &current);
    }

    fn begin_band_feedback(&mut self, now: u64, band: &str) {
        let band = band.to_ascii_lowercase();
        let expected = self
            .spots
            .iter()
            .filter(|spot| {
                spot.band.eq_ignore_ascii_case(&band)
                    && now.saturating_sub(spot.at) <= EXTERNAL_DECISION_TTL_SECS
            })
            .count() as u32;
        self.feedback_expected_spots.insert(band.clone(), expected);
        self.feedback_cursor.insert(band.clone(), now / 15);
        let performance = self.band_performance.entry(band).or_default();
        performance.actual_decodes = 0;
        performance.expected_spots = expected;
        performance.performance_ratio = if expected == 0 { 1.0 } else { 0.0 };
        performance.consecutive_empty_periods = 0;
    }

    fn defer_best_pounce(&mut self, now: u64, current_band: &str) {
        if !self.settings.auto_band || !self.settings.hunter_mode {
            return;
        }
        let Some((call, band, freq_mhz, spotters, new_dxcc, _)) =
            self.external_band_opportunity(now, current_band)
        else {
            return;
        };
        if !new_dxcc
            || spotters == 0
            || !self.auto_band_allowed(&band, now)
            || self
                .deferred_pounces
                .iter()
                .any(|pounce| same_call(&pounce.call, &call))
        {
            return;
        }
        self.deferred_pounces.push_back(DeferredPounce {
            call,
            band,
            freq_mhz,
            deferred_at: now,
        });
        while self.deferred_pounces.len() > self.settings.max_deferred_pounces.max(1) {
            self.deferred_pounces.pop_front();
        }
    }

    fn release_deferred_pounce(
        &mut self,
        now: u64,
        current_band: &str,
        current_dial_mhz: f64,
    ) -> Option<Action> {
        if !self.settings.auto_band || !self.settings.hunter_mode {
            self.deferred_pounces.clear();
            return None;
        }
        while let Some(pounce) = self.deferred_pounces.pop_front() {
            if now.saturating_sub(pounce.deferred_at) > self.settings.deferred_pounce_expire_secs
                || pounce.band.eq_ignore_ascii_case(current_band)
                || !self.auto_band_allowed(&pounce.band, now)
                || (self.last_pounce_at != 0
                    && now.saturating_sub(self.last_pounce_at) < self.settings.pounce_cooldown_secs)
            {
                continue;
            }
            let still_needed = self.spots.iter().any(|spot| {
                same_call(&spot.call, &pounce.call)
                    && spot.band.eq_ignore_ascii_case(&pounce.band)
                    && spot.new_dxcc
                    && now.saturating_sub(spot.at) <= EXTERNAL_DECISION_TTL_SECS
            });
            if !still_needed {
                continue;
            }
            self.deferred_pounces.clear();
            self.last_pounce_at = now;
            self.last_cross_band_alert_at = now;
            self.original_dial = Some((current_band.to_lowercase(), current_dial_mhz));
            self.pending_verification = Some(PendingVerification {
                call: pounce.call.clone(),
                target_band: pounce.band.clone(),
                return_band: current_band.to_lowercase(),
                return_dial_mhz: current_dial_mhz,
                started_at: now,
                expires_at: now.saturating_add(self.settings.local_verification_secs),
            });
            self.statistics.band_changes += 1;
            self.all_time_statistics.band_changes += 1;
            return Some(Action::ChangeBand {
                band: pounce.band.clone(),
                dial_mhz: external_target_dial(&pounce.call, pounce.freq_mhz),
                reason: format!(
                    "deferred Hunter pounce: locally verify {} before calling",
                    pounce.call
                ),
            });
        }
        None
    }

    fn auto_band_allowed(&self, band: &str, now: u64) -> bool {
        if band.eq_ignore_ascii_case("60m")
            || !self
                .settings
                .enabled_bands
                .iter()
                .any(|enabled| enabled.eq_ignore_ascii_case(band))
        {
            return false;
        }
        if !self.settings.time_based_exclusions
            || (!band.eq_ignore_ascii_case("160m") && !band.eq_ignore_ascii_case("80m"))
        {
            return true;
        }
        let utc_hour = ((now / 3_600) % 24) as i16;
        let local_hour = (utc_hour + i16::from(self.settings.utc_offset_hours)).rem_euclid(24);
        !(6..18).contains(&local_hour)
    }

    /// External feeds can nominate a band, never a call. Need facts come from the Nexus
    /// logbook and confidence is independent spotters; a fresh local decode is still required
    /// by `candidates()` before `Action::Call` can exist.
    fn external_band_opportunity(
        &self,
        now: u64,
        current_band: &str,
    ) -> Option<(String, String, f64, u16, bool, i32)> {
        self.external_band_opportunities(now, current_band)
            .into_iter()
            .max_by_key(|choice| choice.5)
    }

    fn external_band_opportunities(
        &self,
        now: u64,
        current_band: &str,
    ) -> Vec<(String, String, f64, u16, bool, i32)> {
        let mut groups: BTreeMap<(String, String), Vec<&Spot>> = BTreeMap::new();
        for spot in &self.spots {
            if now.saturating_sub(spot.at) <= EXTERNAL_DECISION_TTL_SECS
                && !spot.band.eq_ignore_ascii_case(current_band)
                && (spot.new_dxcc || spot.new_band)
                && self
                    .settings
                    .enabled_bands
                    .iter()
                    .any(|band| band.eq_ignore_ascii_case(&spot.band))
            {
                groups
                    .entry((spot.call.clone(), spot.band.clone()))
                    .or_default()
                    .push(spot);
            }
        }
        groups
            .into_values()
            .filter_map(|spots| {
                let newest = *spots.last()?;
                let voices: HashSet<&str> =
                    spots.iter().map(|spot| spot.spotter.as_str()).collect();
                let new_dxcc = spots.iter().any(|spot| spot.new_dxcc);
                let rank = spots.iter().filter_map(|spot| spot.rarity_rank).min();
                let score = if new_dxcc {
                    self.settings.score_new_dxcc * i32::from(rarity_multiplier(rank))
                } else {
                    self.settings.score_new_band
                } + voices.len() as i32 * self.settings.score_spotter;
                Some((
                    score,
                    newest.call.clone(),
                    newest.band.clone(),
                    newest.freq_mhz,
                    voices.len() as u16,
                    new_dxcc,
                ))
            })
            .map(|(score, call, band, frequency, voices, new_dxcc)| {
                (call, band, frequency, voices, new_dxcc, score)
            })
            .collect()
    }
    fn prune(&mut self, now: u64) {
        let ttl = self.settings.decode_ttl_secs;
        while self
            .decodes
            .front()
            .is_some_and(|d| now.saturating_sub(d.at) > ttl)
        {
            if let Some(expired) = self.decodes.pop_front() {
                self.decode_counts.remove(&(expired.call, expired.band));
            }
        }
        while self
            .spots
            .front()
            .is_some_and(|s| now.saturating_sub(s.at) > self.settings.spot_ttl_secs)
        {
            self.spots.pop_front();
        }
        self.recently_worked
            .retain(|_, cooldown| now.saturating_sub(cooldown.at) <= cooldown.duration_secs);
        let retention = u64::from(self.settings.decode_history_retention_cycles.max(2)) * 15;
        self.decode_history.retain(|_, entry| {
            entry.was_worked || now.saturating_sub(entry.last_heard_at) <= retention
        });
        self.slot_observations
            .retain(|observation| now.saturating_sub(observation.at) <= ttl);
    }
    fn record(&mut self, at: u64, kind: &str, call: Option<String>, detail: &str) {
        self.log.push_front(LogEntry {
            at,
            kind: kind.into(),
            call,
            detail: detail.into(),
        });
        self.log.truncate(250);
    }
}

pub fn estimate_callable_range(info: &MultistreamInfo) -> CallableRange {
    if info.is_multistream {
        CallableRange {
            min_hz: info.highest_tx.saturating_add(1_000).max(200),
            max_hz: info.highest_tx.saturating_add(2_000).min(2_800),
            estimated: true,
        }
    } else if !info.tx_frequencies.is_empty() {
        CallableRange {
            min_hz: info.lowest_tx.saturating_add(1_000).max(200),
            max_hz: info.lowest_tx.saturating_add(1_500).min(2_800),
            estimated: true,
        }
    } else {
        CallableRange {
            min_hz: 1_500,
            max_hz: 2_500,
            estimated: false,
        }
    }
}

/// Direct port of HvBandScorer's deterministic formula. Grayline is currently
/// zero upstream, so the parameter is intentionally not approximated here.
pub fn hv_band_score(
    activity: BandScoreInput,
    band: &str,
    hour_utc: u8,
    now: u64,
    hunter_mode: bool,
    penalty: f64,
    historical_decodes_per_period: f64,
    weights: BandScoreWeights,
) -> f64 {
    let spot_score = (f64::from(activity.total_spots) * 2.0).min(100.0);
    let needed_multiplier = if hunter_mode { 200.0 } else { 10.0 };
    let unique_score =
        f64::from(activity.unique_dxcc) * 5.0 + f64::from(activity.needed_dxcc) * needed_multiplier;
    let signal_score = activity.average_snr.clamp(-10.0, 30.0) + 10.0;
    let time_score = time_of_day_factor(band, hour_utc);
    let mut score = if hunter_mode {
        if activity.needed_dxcc > 0 {
            f64::from(activity.needed_dxcc) * 100.0
                + unique_score * 5.0
                + spot_score * 0.5
                + signal_score
        } else {
            spot_score * 0.1 + signal_score * 0.2
        }
    } else {
        spot_score * weights.spot_count
            + unique_score * weights.unique_dxcc
            + f64::from(activity.needed_dxcc) * weights.needed_dxcc
            + signal_score * weights.signal_quality
    };
    score += time_score * weights.time_of_day;
    // HvBandScorer::graylineBonus currently returns 0.0.
    score += 0.0 * weights.grayline;
    if let Some(last_spot_at) = activity.last_spot_at {
        let minutes = now.saturating_sub(last_spot_at) / 60;
        score *= (1.0 - weights.recency_decay).powi(minutes.min(i32::MAX as u64) as i32);
    }
    score *= 1.0 - penalty.clamp(0.0, 1.0) * 0.7;
    if historical_decodes_per_period > 0.0 {
        score += (historical_decodes_per_period / 10.0).min(1.0) * 20.0;
    }
    score
}

fn time_of_day_factor(band: &str, hour_utc: u8) -> f64 {
    let hour = hour_utc % 24;
    if matches!(
        band.to_ascii_lowercase().as_str(),
        "160m" | "80m" | "60m" | "40m"
    ) {
        if hour >= 22 || hour < 6 {
            2.0
        } else if hour >= 18 || hour < 8 {
            1.5
        } else {
            0.5
        }
    } else if matches!(
        band.to_ascii_lowercase().as_str(),
        "20m" | "17m" | "15m" | "12m" | "10m"
    ) {
        if (10..18).contains(&hour) {
            2.0
        } else if (8..20).contains(&hour) {
            1.5
        } else {
            0.5
        }
    } else {
        1.0
    }
}

fn round_to_slot(freq: u32, width: u32) -> u32 {
    ((freq.saturating_add(width / 2)) / width) * width
}

fn slot_analysis_from_decodes(
    frequencies: impl IntoIterator<Item = u32>,
    periods_collected: u16,
    min_hz: u32,
    max_hz: u32,
    width: u32,
) -> SlotAnalysis {
    let width = width.max(1);
    let mut usage: HashMap<u32, u32> = HashMap::new();
    for frequency in frequencies {
        *usage.entry(round_to_slot(frequency, width)).or_default() += 1;
    }
    let default = if min_hz <= 1_500 && 1_500 <= max_hz {
        1_500
    } else {
        (min_hz + max_hz) / 2
    };
    let total_slots = max_hz.saturating_sub(min_hz) / width;
    let occupied_slots = usage
        .iter()
        .filter(|(slot, count)| **count > 0 && **slot >= min_hz && **slot <= max_hz)
        .count() as u32;
    if periods_collected < 2 || min_hz > max_hz {
        return SlotAnalysis {
            suggested_hz: default,
            alternate_hz: min_hz.saturating_add(width),
            confidence: 0.0,
            total_slots,
            occupied_slots,
            periods_collected,
        };
    }
    let slots: Vec<_> = (min_hz..=max_hz)
        .step_by(width as usize)
        .map(|frequency| round_to_slot(frequency, width))
        .collect();
    let clear_with_neighbours = slots.iter().copied().find(|slot| {
        !usage.contains_key(slot)
            && !usage.contains_key(&slot.saturating_sub(width))
            && !usage.contains_key(&slot.saturating_add(width))
    });
    let clear = clear_with_neighbours
        .or_else(|| slots.iter().copied().find(|slot| !usage.contains_key(slot)));
    let (suggested_hz, confidence) = if let Some(slot) = clear {
        (slot, 0.9)
    } else {
        let least = slots
            .iter()
            .copied()
            .min_by_key(|slot| {
                [
                    slot.saturating_sub(width),
                    *slot,
                    slot.saturating_add(width),
                ]
                .into_iter()
                .map(|neighbor| {
                    let count = usage.get(&neighbor).copied().unwrap_or(0);
                    count + if count > 1 { 5 } else { 0 }
                })
                .sum::<u32>()
            })
            .unwrap_or(default);
        (least, 0.6)
    };
    let distance = if range_is_default(min_hz, max_hz) {
        3
    } else {
        2
    };
    let mut alternate_hz = suggested_hz.saturating_add(width * distance);
    if alternate_hz > max_hz {
        alternate_hz = suggested_hz.saturating_sub(width * distance);
    }
    if alternate_hz < min_hz {
        alternate_hz = if distance == 3 {
            min_hz.saturating_add(width)
        } else {
            min_hz
        };
    }
    SlotAnalysis {
        suggested_hz,
        alternate_hz: round_to_slot(alternate_hz, width),
        confidence,
        total_slots,
        occupied_slots,
        periods_collected,
    }
}

fn range_is_default(min_hz: u32, max_hz: u32) -> bool {
    min_hz == 500 && max_hz == 2_500
}

pub fn rarity_multiplier(rank: Option<u16>) -> u8 {
    match rank {
        Some(1..=10) => 5,
        Some(11..=25) => 4,
        Some(26..=50) => 3,
        Some(51..=100) => 2,
        _ => 1,
    }
}

/// Club Log Most Wanted table embedded by MSHV Auto DX Chaser (January 2025).
/// Entity strings intentionally remain byte-for-byte compatible with that table.
pub fn clublog_rank(entity: &str) -> Option<u16> {
    const ENTITIES: &[&str] = &[
        "DPR of Korea",
        "Bouvet",
        "Heard Is.",
        "Crozet Is.",
        "Kerguelen Is.",
        "Scarborough Reef",
        "Amsterdam & St. Paul Is.",
        "Annobon Is.",
        "Macao",
        "Baker & Howland Is.",
        "Bhutan",
        "Eritrea",
        "Agalega & St. Brandon",
        "Yemen",
        "Palmyra & Jarvis Is.",
        "Cocos (Keeling) Is.",
        "Clipperton Is.",
        "Lakshadweep Is.",
        "Peter 1 Is.",
        "Pr. Edward & Marion Is.",
        "Nauru",
        "Tuvalu",
        "Rotuma Is.",
        "Norfolk Is.",
        "Bangladesh",
        "Niger",
        "Conway Reef",
        "Fiji",
        "Monaco",
        "Mayotte",
        "Comoros",
        "Juan de Nova, Europa",
        "Reunion Is.",
        "Glorioso Is.",
        "San Andres & Providencia",
        "Navassa Is.",
        "Guinea",
        "Sao Tome & Principe",
        "Nigeria",
        "Oman",
        "Timor - Leste",
        "Papua New Guinea",
        "Philippines",
        "Nepal",
        "Samoa",
        "St. Barthelemy",
        "Turkmenistan",
        "Tunisia",
        "New Caledonia",
        "Ethiopia",
        "Gabon",
        "Ghana",
        "Guam",
        "Guinea-Bissau",
        "Haiti",
        "Guyana",
        "Honduras",
        "India",
        "Indonesia",
        "Iran",
        "Iraq",
        "Jamaica",
        "Japan",
        "Jordan",
        "Kazakhstan",
        "Kenya",
        "Kiribati",
        "Kuwait",
        "Laos",
        "Latvia",
        "Lebanon",
        "Lesotho",
        "Liberia",
        "Libya",
        "Liechtenstein",
        "Lithuania",
        "Luxembourg",
        "Madagascar",
        "Malawi",
        "W. Malaysia",
        "Maldives",
        "Mali",
        "Malta",
        "Marshall Is.",
        "Mauritania",
        "Mauritius",
        "Mexico",
        "Micronesia",
        "Moldova",
        "Mongolia",
        "Montenegro",
        "Montserrat",
        "Morocco",
        "Mozambique",
        "Myanmar",
        "Namibia",
        "New Zealand",
        "Nicaragua",
        "Pakistan",
        "Palau",
    ];
    ENTITIES
        .iter()
        .position(|name| name.eq_ignore_ascii_case(entity.trim()))
        .map(|index| index as u16 + 1)
}
pub fn standard_dial(band: &str) -> Option<f64> {
    STANDARD_FT8_MHZ
        .iter()
        .find(|(b, _)| b.eq_ignore_ascii_case(band))
        .map(|(_, f)| *f)
}
pub fn normalize_call(call: &str) -> String {
    call.trim().to_uppercase()
}
fn external_target_dial(call: &str, spotted_mhz: f64) -> f64 {
    if is_rare_dx_callsign(call) {
        spotted_mhz
    } else {
        (spotted_mhz - 0.001_5).max(0.0)
    }
}
pub fn is_rare_dx_callsign(call: &str) -> bool {
    let call = normalize_call(call);
    ["KP5", "KP1", "3Y", "ZS8", "VK0", "VP8", "ZL9", "T31"]
        .iter()
        .any(|prefix| call.starts_with(prefix) || call.contains(&format!("/{prefix}")))
        || [
            "BS7H", "BV9P", "P5", "YK", "E3", "7O", "EZ", "1A", "SV/A", "FT5W", "FT5X", "FT5Z",
        ]
        .iter()
        .any(|prefix| call.starts_with(prefix))
}

/// Upstream SpotAggregator's exact US spotter-prefix boundary.
pub fn is_us_callsign(call: &str) -> bool {
    let call = normalize_call(call);
    let prefix: String = call.chars().take(2).collect();
    call.starts_with('K')
        || call.starts_with('W')
        || call.starts_with('N')
        || (prefix.as_str() >= "AA" && prefix.as_str() <= "AL")
}

pub fn logged_qso_matches_attempt(
    call: &str,
    band: &str,
    mode: &str,
    when_unix: u64,
    expected_call: &str,
    expected_band: &str,
    started_at: u64,
) -> bool {
    same_call(call, expected_call)
        && band.eq_ignore_ascii_case(expected_band)
        && matches!(mode.to_ascii_uppercase().as_str(), "FT8" | "FT4")
        && when_unix >= started_at.saturating_sub(2)
}

fn utc_hhmm(unix_secs: u64) -> String {
    let hour = (unix_secs / 3_600) % 24;
    let minute = (unix_secs / 60) % 60;
    format!("{hour:02}:{minute:02}")
}

pub fn format_tx_sms(now: u64, call: &str, band: &str, mode: &str) -> String {
    format!(
        "MSHV TX: Calling {} on {} {}\n{}z",
        normalize_call(call),
        band,
        mode.trim().to_uppercase(),
        utc_hhmm(now)
    )
}

pub fn format_qso_sms(
    now: u64,
    call: &str,
    entity: &str,
    band: &str,
    grid: Option<&str>,
) -> String {
    let mut message = format!("MSHV QSO: {}", normalize_call(call));
    if !entity.trim().is_empty() {
        message.push_str(&format!(" ({})", entity.trim()));
    }
    message.push('\n');
    message.push_str(band.trim());
    if let Some(grid) = grid.filter(|grid| !grid.trim().is_empty()) {
        message.push(' ');
        message.push_str(&grid.trim().to_uppercase());
    }
    message.push_str(&format!("\n{}z", utc_hhmm(now)));
    message
}

fn call_base(call: &str) -> &str {
    call.split('/')
        .max_by_key(|part| part.len())
        .unwrap_or(call)
}
fn same_call(a: &str, b: &str) -> bool {
    call_base(a).eq_ignore_ascii_case(call_base(b))
}
fn us_grid_allowed(_mine: &str, theirs: Option<&str>) -> bool {
    let Some(t) = theirs else { return false };
    is_us_grid(t)
}

/// Exact four-character Maidenhead territory test used by upstream TargetScorer.
/// Longer locators are accepted based on their first four characters.
pub fn is_us_grid(grid: &str) -> bool {
    let bytes = grid.as_bytes();
    if bytes.len() < 4 {
        return false;
    }
    let field = [bytes[0].to_ascii_uppercase(), bytes[1].to_ascii_uppercase()];
    let lon = bytes[2];
    let lat = bytes[3];
    if !(b'A'..=b'R').contains(&field[0])
        || !(b'A'..=b'R').contains(&field[1])
        || !lon.is_ascii_digit()
        || !lat.is_ascii_digit()
    {
        return false;
    }
    let field = std::str::from_utf8(&field).expect("ASCII Maidenhead field");

    if matches!(field, "EN" | "FN") {
        return lat < b'5';
    }
    if matches!(field, "CM" | "CN" | "DM" | "DN" | "EM" | "FM") {
        return true;
    }
    if matches!(field, "AO" | "AP" | "AQ" | "BO" | "BP" | "BQ" | "CO" | "CP") {
        return true;
    }
    if field == "BK" && (b'2'..=b'4').contains(&lat) && lon >= b'9' {
        return true;
    }
    if field == "BL" && lon <= b'1' && lat <= b'1' {
        return true;
    }
    if field == "FK" && (b'6'..=b'8').contains(&lon) && lat >= b'7' {
        return true;
    }
    if field == "QK" && (b'2'..=b'4').contains(&lon) && (b'2'..=b'5').contains(&lat) {
        return true;
    }
    field == "AH" && matches!(lon, b'3' | b'4') && (b'5'..=b'7').contains(&lat)
}

fn quiet_hour(now: u64, start: Option<u8>, end: Option<u8>) -> bool {
    let (Some(start), Some(end)) = (start, end) else {
        return false;
    };
    let hour = ((now / 3_600) % 24) as u8;
    if start == end {
        return false;
    }
    if start < end {
        (start..end).contains(&hour)
    } else {
        hour >= start || hour < end
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(call: &str, at: u64) -> Decode {
        Decode {
            at,
            call: call.into(),
            band: "20m".into(),
            dial_mhz: 14.074,
            audio_hz: 1200,
            snr: -10,
            message: format!("CQ {call} EN52"),
            is_cq: true,
            directed_to_me: false,
            country: "France".into(),
            grid: Some("EN52".into()),
            new_dxcc: false,
            new_band: true,
            new_grid: false,
            rarity_rank: None,
        }
    }
    #[test]
    fn upstream_score_vector() {
        let mut c = Chaser::default();
        let mut s = Settings::default();
        s.enabled = true;
        c.configure(s);
        c.set_operator_arm(true, 0);
        c.set_station("United States", "EN52");
        let mut x = d("F4ABC", 100);
        x.new_dxcc = true;
        x.new_band = true;
        x.new_grid = true;
        x.rarity_rank = Some(9);
        c.ingest_decode(x);
        c.ingest_decode(d("F4ABC", 101));
        c.ingest_spot(Spot {
            at: 101,
            call: "F4ABC".into(),
            band: "20m".into(),
            freq_mhz: 14.074,
            source: "RBN".into(),
            spotter: "K1A".into(),
            mode: Some("FT8".into()),
            snr: None,
            country: None,
            grid: None,
            new_dxcc: true,
            new_band: false,
            rarity_rank: Some(9),
        });
        let got = c.candidates(102);
        assert_eq!(got[0].score, 50_325);
        assert_eq!(got[0].rarity_multiplier, 5);
    }

    #[test]
    fn upstream_min_snr_boundary_accepts_minus_20_and_rejects_minus_21() {
        let mut c = Chaser::new(Settings {
            enabled: true,
            us_grid_only: false,
            ..Settings::default()
        });
        let mut accepted = d("F4ABC", 100);
        accepted.snr = -20;
        let mut rejected = d("DL1ABC", 100);
        rejected.snr = -21;
        c.ingest_decode(accepted);
        c.ingest_decode(rejected);
        let calls: Vec<_> = c
            .candidates(100)
            .into_iter()
            .map(|candidate| candidate.call)
            .collect();
        assert_eq!(calls, ["F4ABC"]);
    }

    #[test]
    fn us_spotter_filter_matches_upstream_prefixes_and_rare_exemption() {
        assert!(is_us_callsign("K1ABC"));
        assert!(is_us_callsign("W3LPL-2"));
        assert!(is_us_callsign("N0CALL"));
        assert!(is_us_callsign("AA1A"));
        assert!(is_us_callsign("AL7ABC"));
        assert!(!is_us_callsign("AM1A"));
        assert!(!is_us_callsign("VE3ABC"));

        let mut c = Chaser::default();
        let external = |call: &str, spotter: &str| Spot {
            at: 100,
            call: call.into(),
            band: "20m".into(),
            freq_mhz: 14.074,
            source: "PSK Reporter".into(),
            spotter: spotter.into(),
            mode: Some("FT8".into()),
            snr: Some(-10),
            country: None,
            grid: None,
            new_dxcc: true,
            new_band: true,
            rarity_rank: None,
        };
        c.ingest_spot(external("F4ABC", "DL1ABC"));
        assert!(c.spots.is_empty(), "ordinary non-US evidence is filtered");
        c.ingest_spot(external("F4ABC", "K1ABC"));
        c.ingest_spot(external("3Y0J", "DL1ABC"));
        assert_eq!(c.spots.len(), 2, "US and rare-DX-exempt evidence remains");

        c.ingest_decode(d("F4LOCAL", 100));
        assert_eq!(
            c.decodes.len(),
            1,
            "local decodes bypass the spotter filter"
        );
    }

    #[test]
    fn local_and_external_ttl_boundaries_are_distinct() {
        let mut local = Chaser::new(Settings {
            us_grid_only: false,
            ..Settings::default()
        });
        local.ingest_decode(d("F4LOCAL", 0));
        assert!(!local.candidates(120).is_empty());
        assert!(local.candidates(121).is_empty());

        let mut external = Chaser::new(Settings {
            decode_ttl_secs: 1_000,
            us_grid_only: false,
            ..Settings::default()
        });
        external.ingest_decode(d("F4SPOT", 0));
        let mut evidence = Spot {
            at: 0,
            call: "F4SPOT".into(),
            band: "20m".into(),
            freq_mhz: 14.074,
            source: "RBN".into(),
            spotter: "K1ABC".into(),
            mode: Some("FT8".into()),
            snr: None,
            country: None,
            grid: None,
            new_dxcc: false,
            new_band: true,
            rarity_rank: None,
        };
        external.ingest_spot(evidence.clone());
        assert_eq!(external.candidates(120)[0].spotter_count, 1);
        assert_eq!(external.candidates(121)[0].spotter_count, 0);
        assert_eq!(external.snapshot(300, false).active_spot_pairs, 1);
        assert_eq!(external.snapshot(301, false).active_spot_pairs, 0);
        evidence.at = 301;
        external.ingest_spot(evidence);
        assert_eq!(
            external.spots.len(),
            1,
            "expired external evidence is pruned"
        );
    }

    #[test]
    fn external_spotter_confidence_and_cache_are_bounded() {
        let mut chaser = Chaser::new(Settings {
            decode_ttl_secs: 1_000,
            us_grid_only: false,
            ..Settings::default()
        });
        chaser.ingest_decode(d("F4ABC", 100));
        for index in 0..3_001 {
            chaser.ingest_spot(Spot {
                at: 100,
                call: if index < 21 {
                    "F4ABC".into()
                } else {
                    format!("F{index}")
                },
                band: "20m".into(),
                freq_mhz: 14.074,
                source: "RBN".into(),
                spotter: format!("K{index}"),
                mode: Some("FT8".into()),
                snr: None,
                country: None,
                grid: None,
                new_dxcc: true,
                new_band: true,
                rarity_rank: None,
            });
        }
        assert_eq!(chaser.spots.len(), 3_000);
        assert_eq!(chaser.candidates(100)[0].spotter_count, 20);
    }

    #[test]
    fn matching_logged_qso_cannot_recall_cached_target_without_new_decode() {
        let mut chaser = Chaser::new(Settings {
            enabled: true,
            us_grid_only: false,
            ..Settings::default()
        });
        chaser.set_operator_arm(true, 1);
        chaser.ingest_decode(d("F4DONE", 100));
        assert!(chaser
            .cycle(100, "20m", 14.074, true, false)
            .iter()
            .any(|action| matches!(action, Action::Call { .. })));
        chaser.qso_complete(101, "F4DONE");
        assert!(chaser.target.is_none());
        assert!(chaser.recently_worked.contains_key("F4DONE"));
        assert!(chaser
            .cycle(102, "20m", 14.074, true, false)
            .iter()
            .all(|action| !matches!(action, Action::Call { .. })));
    }
    #[test]
    fn candidates_require_a_need_and_use_exclusive_need_priority() {
        let mut c = Chaser::default();
        let mut settings = Settings::default();
        settings.enabled = true;
        c.configure(settings);
        let mut no_need = d("F4ABC", 100);
        no_need.new_band = false;
        no_need.directed_to_me = true;
        no_need.is_cq = true;
        no_need.snr = 20;
        c.ingest_decode(no_need);
        assert!(
            c.candidates(100).is_empty(),
            "bonuses cannot create a target"
        );

        let mut all_needs = d("A52AA", 101);
        all_needs.new_dxcc = true;
        all_needs.new_band = true;
        all_needs.new_grid = true;
        all_needs.rarity_rank = Some(11);
        c.ingest_decode(all_needs);
        let candidate = &c.candidates(101)[0];
        assert_eq!(candidate.score, 40_250);
    }

    #[test]
    fn default_bands_exactly_match_upstream_auto_band_set() {
        assert_eq!(
            Settings::default().enabled_bands,
            ["160m", "80m", "40m", "30m", "20m", "17m", "15m", "12m", "10m"]
        );
    }

    #[test]
    fn source_use_settings_default_on_survive_serde_and_gate_only_their_source() {
        let defaults: Settings = serde_json::from_str("{}").unwrap();
        assert!(defaults.use_rbn);
        assert!(defaults.use_psk_reporter);
        assert!(defaults.use_dx_cluster);

        let mut settings = defaults;
        settings.use_rbn = false;
        settings.use_psk_reporter = false;
        settings.use_dx_cluster = false;
        let round_trip: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert!(!round_trip.use_rbn);
        assert!(!round_trip.use_psk_reporter);
        assert!(!round_trip.use_dx_cluster);

        let mut chaser = Chaser::new(round_trip);
        for source in ["RBN", "PSK Reporter", "DX cluster"] {
            chaser.ingest_spot(Spot {
                at: 1,
                call: "A52AA".into(),
                band: "20m".into(),
                freq_mhz: 14.074,
                source: source.into(),
                spotter: "K1A".into(),
                mode: Some("FT8".into()),
                snr: None,
                country: None,
                grid: None,
                new_dxcc: true,
                new_band: true,
                rarity_rank: Some(11),
            });
        }
        assert!(chaser.spots.is_empty());
    }

    #[test]
    fn us_grid_validation_matches_upstream_boundaries_and_territories() {
        for grid in [
            "CM00", "CN99", "DM12", "DN88", "EM10", "FM19", "EN04", "FN44", "AO00", "AP99", "AQ12",
            "BO55", "BP51", "BQ40", "CO28", "CP30", "BK92", "BK93", "BK94", "BL00", "BL01", "BL10",
            "BL11", "FK67", "FK69", "FK78", "FK89", "QK22", "QK45", "AH35", "AH47", "fn31pr",
        ] {
            assert!(is_us_grid(grid), "expected US grid: {grid}");
        }
        for grid in [
            "", "F", "FN", "FN3", "SS00", "1N00", "FA0X", "EN05", "FN95", "BK19", "BK28", "BK29",
            "BL22", "FK57", "QK12", "QK26", "AH25", "AH48", "JN18",
        ] {
            assert!(!is_us_grid(grid), "expected non-US grid: {grid}");
        }
    }
    #[test]
    fn never_calls_external_only() {
        let mut c = Chaser::default();
        let mut s = Settings::default();
        s.enabled = true;
        c.configure(s);
        c.set_operator_arm(true, 0);
        c.ingest_spot(Spot {
            at: 1,
            call: "3Y0J".into(),
            band: "20m".into(),
            freq_mhz: 14.074,
            source: "cluster".into(),
            spotter: "K1A".into(),
            mode: Some("FT8".into()),
            snr: None,
            country: None,
            grid: None,
            new_dxcc: true,
            new_band: false,
            rarity_rank: Some(2),
        });
        assert!(c
            .cycle(2, "20m", 14.074, true, false)
            .iter()
            .all(|a| !matches!(a, Action::Call { .. })));
    }
    #[test]
    fn corroborated_external_need_changes_band_for_local_verification_only() {
        let mut c = Chaser::default();
        let mut settings = Settings::default();
        settings.enabled = true;
        settings.hunter_mode = true;
        settings.min_dwell_secs = 0;
        c.configure(settings);
        c.set_operator_arm(true, 0);
        assert!(c.cycle(100, "20m", 14.074, true, false).is_empty());
        for spotter in ["K1A", "W9B"] {
            c.ingest_spot(Spot {
                at: 200,
                call: "A52AA".into(),
                band: "15m".into(),
                freq_mhz: 21.074,
                source: "DX cluster".into(),
                spotter: spotter.into(),
                mode: Some("FT8".into()),
                snr: None,
                country: Some("Bhutan".into()),
                grid: None,
                new_dxcc: false,
                new_band: true,
                rarity_rank: Some(11),
            });
        }
        let actions = c.cycle(201, "20m", 14.074, true, false);
        assert!(matches!(actions.first(), Some(Action::ChangeBand { band, .. }) if band == "15m"));
        assert!(actions
            .iter()
            .all(|action| !matches!(action, Action::Call { .. })));
    }
    #[test]
    fn external_qsy_decision_expires_after_120_seconds_even_with_long_retention() {
        let settings = Settings {
            enabled: true,
            hunter_mode: true,
            time_based_exclusions: false,
            decode_ttl_secs: 3_600,
            spot_ttl_secs: 300,
            ..Settings::default()
        };
        let make = || Spot {
            at: 100,
            call: "3Y0J".into(),
            band: "15m".into(),
            freq_mhz: 21.081,
            source: "DX cluster".into(),
            spotter: "W1AW".into(),
            mode: Some("FT8".into()),
            snr: None,
            country: Some("Bouvet".into()),
            grid: None,
            new_dxcc: true,
            new_band: true,
            rarity_rank: Some(1),
        };

        let mut boundary = Chaser::new(settings.clone());
        boundary.set_operator_arm(true, 0);
        boundary.cycle(1, "20m", 14.074, true, false);
        boundary.ingest_spot(make());
        assert!(matches!(
            boundary.cycle(220, "20m", 14.074, true, false).first(),
            Some(Action::ChangeBand { band, .. }) if band == "15m"
        ));

        let mut expired = Chaser::new(settings);
        expired.set_operator_arm(true, 0);
        expired.cycle(1, "20m", 14.074, true, false);
        expired.ingest_spot(make());
        assert!(expired
            .cycle(221, "20m", 14.074, true, false)
            .iter()
            .all(|action| !matches!(action, Action::ChangeBand { .. })));
        assert_eq!(expired.snapshot(221, true).active_spot_pairs, 1);
    }
    #[test]
    fn tx_latch_is_mandatory() {
        let mut c = Chaser::default();
        let mut s = Settings::default();
        s.enabled = true;
        c.configure(s);
        c.set_operator_arm(true, 0);
        c.ingest_decode(d("F4ABC", 1));
        let a = c.cycle(2, "20m", 14.074, false, false);
        assert!(matches!(a.first(), Some(Action::Notify { .. })));
        assert!(c.target.is_none());
    }
    #[test]
    fn engage_lock_survives_missed_cycles() {
        let mut c = Chaser::default();
        let mut s = Settings::default();
        s.enabled = true;
        c.configure(s);
        c.set_operator_arm(true, 0);
        let mut x = d("F4ABC", 1);
        x.directed_to_me = true;
        c.ingest_decode(x);
        assert!(matches!(
            c.cycle(2, "20m", 14.074, true, false).first(),
            Some(Action::Call { .. })
        ));
        for n in 3..20 {
            assert!(c.cycle(n, "20m", 14.074, true, false).is_empty());
        }
        assert!(c.target.is_some());
    }
    #[test]
    fn slot_finder_avoids_occupied() {
        let mut c = Chaser::default();
        let mut s = Settings::default();
        s.enabled = true;
        c.configure(s);
        c.set_operator_arm(true, 0);
        let mut x = d("F4ABC", 1);
        x.audio_hz = 1500;
        c.ingest_decode(x.clone());
        x.at = 16;
        c.ingest_decode(x);
        assert_ne!(c.best_free_slot("20m"), 1500);
    }

    #[test]
    fn slot_finder_requires_two_periods_and_uses_rounded_neighbour_clearance() {
        let mut c = Chaser::default();
        let mut first = d("K1AAA", 1);
        first.audio_hz = 500;
        c.ingest_decode(first);
        assert_eq!(c.slot_analysis("20m", None).confidence, 0.0);
        let mut second = d("K1BBB", 16);
        second.audio_hz = 560;
        c.ingest_decode(second);
        let analysis = c.slot_analysis("20m", None);
        assert_eq!(analysis.periods_collected, 2);
        assert_eq!(analysis.suggested_hz, 660);
        assert_eq!(analysis.confidence, 0.9);
    }

    #[test]
    fn multistream_range_matches_upstream_offsets_and_clamps() {
        let mut c = Chaser::default();
        for (at, frequency) in [(1, 500), (16, 800)] {
            let mut decode = d("3Y0J", at);
            decode.audio_hz = frequency;
            c.ingest_decode(decode);
        }
        let info = c.detect_multistream("20m", "3y0j");
        assert!(info.is_multistream);
        assert_eq!(info.tx_frequencies, [480, 780]);
        assert_eq!(
            estimate_callable_range(&info),
            CallableRange {
                min_hz: 1_780,
                max_hz: 2_780,
                estimated: true,
            }
        );
    }

    #[test]
    fn band_scorer_matches_upstream_normal_hunter_penalty_and_history_vectors() {
        let input = BandScoreInput {
            total_spots: 10,
            unique_dxcc: 4,
            needed_dxcc: 2,
            average_snr: -5.0,
            last_spot_at: Some(1_000),
        };
        let weights = BandScoreWeights::default();
        let normal = hv_band_score(input, "20m", 12, 1_000, false, 0.0, 0.0, weights);
        assert!((normal - 119.5).abs() < f64::EPSILON);
        let hunter = hv_band_score(input, "20m", 12, 1_000, true, 0.0, 0.0, weights);
        assert!((hunter - 2_317.0).abs() < f64::EPSILON);
        let adjusted = hv_band_score(input, "20m", 12, 1_000, false, 0.5, 5.0, weights);
        assert!((adjusted - 87.675).abs() < 0.000_001);
        let stale = hv_band_score(input, "20m", 12, 1_120, false, 0.0, 0.0, weights);
        assert!((stale - 96.795).abs() < 0.000_001);
    }

    #[test]
    fn decode_feedback_matches_upstream_ema_penalty_and_bailout() {
        let mut performance = BandPerformance::default();
        performance.finish_period(DecodePeriodStats::default(), 10, 12, 100);
        assert_eq!(performance.current_penalty, 1.0);
        assert_eq!(performance.penalty_expiry, 700);
        assert!(!performance.should_bailout());
        performance.finish_period(DecodePeriodStats::default(), 10, 12, 115);
        assert!(performance.should_bailout());
        assert_eq!(performance.hourly_performance[&12], 0.0);
        assert_eq!(performance.penalty_at(700), 1.0);
        assert_eq!(performance.penalty_at(701), 0.0);

        let stats = DecodePeriodStats {
            decode_count: 4,
            unique_callsigns: 3,
            average_snr: -10.0,
            max_snr: -4.0,
            ..DecodePeriodStats::default()
        };
        performance.finish_period(stats, 10, 12, 130);
        assert!((performance.hourly_performance[&12] - 0.4).abs() < f64::EPSILON);

        let mut chaser = Chaser::default();
        chaser.record_band_period("20m", DecodePeriodStats::default(), 10, 12, 100);
        chaser.record_band_period("15m", DecodePeriodStats::default(), 0, 12, 100);
        let state = chaser.persistent_state();
        assert_eq!(state.version, 3);
        let encoded = serde_json::to_string(&state).unwrap();
        let mut restored = Chaser::default();
        restored.restore_persistent_state(serde_json::from_str(&encoded).unwrap());
        assert_eq!(
            restored.best_bailout_band("20m", 12, 100).as_deref(),
            Some("15m")
        );
    }

    #[test]
    fn cycle_feeds_completed_decode_periods_even_while_disarmed() {
        let mut settings = Settings::default();
        settings.enabled = true;
        let mut chaser = Chaser::new(settings);
        for spotter in ["W1A", "W1B"] {
            chaser.ingest_spot(Spot {
                at: 1,
                call: "F4ABC".into(),
                band: "20m".into(),
                freq_mhz: 14.074,
                source: "cluster".into(),
                spotter: spotter.into(),
                mode: Some("FT8".into()),
                snr: Some(-8),
                country: Some("France".into()),
                grid: None,
                new_dxcc: true,
                new_band: true,
                rarity_rank: None,
            });
        }
        let mut decode = d("F4ABC", 1);
        decode.snr = -7;
        decode.new_dxcc = true;
        decode.new_grid = true;
        chaser.ingest_decode(decode);
        assert!(chaser.cycle(1, "20m", 14.074, false, false).is_empty());
        assert!(chaser.cycle(16, "20m", 14.074, false, false).is_empty());
        let performance = &chaser.band_performance["20m"];
        assert_eq!(performance.total_periods, 1);
        assert_eq!(performance.actual_decodes, 1);
        assert_eq!(performance.expected_spots, 2);
        assert_eq!(performance.recent_periods[0].unique_callsigns, 1);
        assert_eq!(performance.recent_periods[0].new_dxcc_count, 1);
        assert_eq!(performance.recent_periods[0].new_grid_count, 1);
        assert_eq!(performance.recent_periods[0].average_snr, -7.0);
    }

    #[test]
    fn ordinary_ranking_uses_persisted_penalty_history_and_bailout_without_bypassing_gates() {
        let mut settings = Settings::default();
        settings.enabled = true;
        settings.time_based_exclusions = false;
        settings.min_dwell_secs = 30;
        settings.cross_band_alert_cooldown_secs = 0;
        settings.quiet_spot_threshold = 1;
        settings.decode_ttl_secs = 300;
        settings.us_spotters_only = false;
        let mut chaser = Chaser::new(settings);
        chaser.set_operator_arm(true, 0);
        for index in 0..10 {
            chaser.ingest_spot(Spot {
                at: 1,
                call: format!("K1A{index}"),
                band: "20m".into(),
                freq_mhz: 14.074,
                source: "cluster".into(),
                spotter: format!("W1{index}"),
                mode: Some("FT8".into()),
                snr: Some(0),
                country: Some(format!("Home {index}")),
                grid: None,
                new_dxcc: false,
                new_band: false,
                rarity_rank: None,
            });
        }
        for (band, call, historical) in [("15m", "F4ABC", 8.0), ("10m", "JA1ABC", 1.0)] {
            for index in 0..5 {
                chaser.ingest_spot(Spot {
                    at: 1,
                    call: format!("{call}{index}"),
                    band: band.into(),
                    freq_mhz: standard_dial(band).unwrap(),
                    source: "cluster".into(),
                    spotter: format!("DX-{band}-{index}"),
                    mode: Some("FT8".into()),
                    snr: Some(-5),
                    country: Some(format!("{call}-{index}")),
                    grid: None,
                    new_dxcc: false,
                    new_band: true,
                    rarity_rank: None,
                });
            }
            chaser
                .band_performance
                .entry(band.into())
                .or_default()
                .hourly_performance
                .insert(0, historical);
        }
        assert!(chaser.cycle(1, "20m", 14.074, true, false).is_empty());
        chaser.record_band_period("20m", DecodePeriodStats::default(), 10, 0, 15);
        chaser.record_band_period("20m", DecodePeriodStats::default(), 10, 0, 30);
        assert!(chaser.band_performance["20m"].should_bailout());
        assert_eq!(
            chaser.best_bailout_band("20m", 0, 31).as_deref(),
            Some("15m")
        );
        let actions = chaser.cycle(31, "20m", 14.074, true, false);
        assert!(matches!(
            actions.first(),
            Some(Action::ChangeBand { band, .. }) if band == "15m"
        ));
        assert!(actions
            .iter()
            .all(|action| !matches!(action, Action::Call { .. })));

        let persisted = serde_json::to_string(&chaser.persistent_state()).unwrap();
        let mut restored = Chaser::default();
        restored.restore_persistent_state(serde_json::from_str(&persisted).unwrap());
        assert_eq!(
            restored.best_bailout_band("20m", 0, 31).as_deref(),
            Some("15m")
        );
    }
    #[test]
    fn rarity_bands_match_upstream() {
        assert_eq!(rarity_multiplier(Some(1)), 5);
        assert_eq!(rarity_multiplier(Some(11)), 4);
        assert_eq!(rarity_multiplier(Some(26)), 3);
        assert_eq!(rarity_multiplier(Some(51)), 2);
        assert_eq!(rarity_multiplier(Some(101)), 1);
        for call in [
            "KP5AA",
            "K1ABC/KP1",
            "3Y0J",
            "ZS8Z",
            "VK0EK",
            "VP8PJ",
            "ZL9HR",
            "T31EU",
            "BS7H",
            "BV9P",
            "P5ABC",
            "YK9A",
            "E30FB",
            "7O6T",
            "EZ8BO",
            "1A0KM",
            "SV/A",
            "FT5WQ",
            "FT5XU",
            "FT5ZM",
        ] {
            assert!(
                is_rare_dx_callsign(call),
                "missing upstream rare prefix {call}"
            );
        }
        assert!(!is_rare_dx_callsign("F4ABC"));
    }

    #[test]
    fn sms_outbox_rate_limits_retries_and_drops_after_bound() {
        let mut queue = SmsOutbox::default();
        assert_eq!(queue.min_interval_secs, 60);
        assert_eq!(queue.max_attempts, 3);
        queue.enqueue("one".into(), 10);
        queue.enqueue("two".into(), 10);
        queue.enqueue("two".into(), 10);
        assert_eq!(queue.len(), 3, "upstream queues every notification event");
        assert_eq!(queue.due(10).map(|job| job.body.as_str()), Some("one"));
        assert!(queue.failed(10).is_none());
        assert!(queue.due(39).is_none());
        assert!(queue.due(40).is_some());
        queue.delivered(40);
        assert!(
            queue.due(40).is_none(),
            "rate window blocks the next message"
        );
        assert!(queue.due(41).is_none());
        assert_eq!(queue.due(100).map(|job| job.body.as_str()), Some("two"));
        for now in [100, 130, 190] {
            queue.failed(now);
        }
        assert_eq!(
            queue.len(),
            1,
            "the failed event is dropped but its distinct duplicate remains"
        );
    }

    #[test]
    fn sms_interval_is_migrated_and_clamped_to_upstream_range() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.sms_min_interval_secs, 60);
        assert_eq!(
            Settings {
                sms_min_interval_secs: 1,
                ..Settings::default()
            }
            .normalized()
            .sms_min_interval_secs,
            10
        );
        assert_eq!(
            Settings {
                sms_min_interval_secs: 9_999,
                ..Settings::default()
            }
            .normalized()
            .sms_min_interval_secs,
            3_600
        );
    }

    #[test]
    fn sms_messages_match_upstream_facts_and_utc_shape() {
        assert_eq!(
            format_tx_sms(13 * 3_600 + 7 * 60, "kh8/w1aw", "20m", "ft4"),
            "MSHV TX: Calling KH8/W1AW on 20m FT4\n13:07z"
        );
        assert_eq!(
            format_qso_sms(23 * 3_600 + 59 * 60, "ja1abc", "Japan", "40m", Some("pm95")),
            "MSHV QSO: JA1ABC (Japan)\n40m PM95\n23:59z"
        );
    }

    #[test]
    fn session_reset_preserves_persisted_all_time_statistics() {
        let mut chaser = Chaser::new(Settings {
            enabled: true,
            ..Settings::default()
        });
        chaser.start(10);
        chaser.statistics.completed = 2;
        chaser.all_time_statistics.completed = 12;
        chaser.all_time_statistics.new_dxcc_worked = 4;
        chaser.reset_statistics(20);
        let snapshot = chaser.snapshot(20, false);
        assert_eq!(snapshot.session_statistics.completed, 0);
        assert_eq!(snapshot.session_statistics.started_at, Some(20));
        assert_eq!(snapshot.all_time_statistics.completed, 12);
        assert_eq!(snapshot.all_time_statistics.new_dxcc_worked, 4);

        let saved = chaser.persistent_state();
        assert_eq!(saved.version, 3);
        let mut restored = Chaser::default();
        restored.restore_persistent_state(saved);
        let snapshot = restored.snapshot(30, false);
        assert_eq!(snapshot.session_statistics.completed, 0);
        assert_eq!(snapshot.all_time_statistics.completed, 12);
    }

    #[test]
    fn unrelated_logged_qso_updates_counts_without_clearing_active_target() {
        let mut chaser = Chaser::new(Settings {
            enabled: true,
            us_grid_only: false,
            sms_mode: SmsMode::QsoCompleteOnly,
            ..Settings::default()
        });
        chaser.set_operator_arm(true, 1);
        chaser.ingest_decode(d("F4TARGET", 100));
        let actions = chaser.cycle(100, "20m", 14.074, true, false);
        assert!(actions
            .iter()
            .any(|action| matches!(action, Action::Call { .. })));
        assert_eq!(
            chaser
                .target
                .as_ref()
                .map(|target| target.candidate.call.as_str()),
            Some("F4TARGET")
        );

        let sms = chaser.observe_logged_qso(101, "DL1OTHER", "Germany", "20m", Some("JO31"));
        assert_eq!(chaser.statistics.completed, 1);
        assert_eq!(chaser.all_time_statistics.completed, 1);
        assert_eq!(
            chaser
                .target
                .as_ref()
                .map(|target| target.candidate.call.as_str()),
            Some("F4TARGET")
        );
        assert!(chaser.recently_worked.contains_key("DL1OTHER"));
        assert!(
            matches!(sms.first(), Some(Action::Sms { body }) if body.contains("DL1OTHER (Germany)"))
        );
    }

    #[test]
    fn version_two_history_migrates_legacy_statistics_to_all_time() {
        let json = r#"{
          "version": 2,
          "statistics": {"completed": 9, "newDxccWorked": 3},
          "bandHistory": [],
          "bandPerformance": {}
        }"#;
        let state: PersistentState = serde_json::from_str(json).unwrap();
        let mut restored = Chaser::default();
        restored.restore_persistent_state(state);
        let snapshot = restored.snapshot(30, false);
        assert_eq!(snapshot.all_time_statistics.completed, 9);
        assert_eq!(snapshot.all_time_statistics.new_dxcc_worked, 3);
        assert_eq!(snapshot.session_statistics.completed, 0);
    }

    #[test]
    fn ambiguous_sms_delivery_is_discardable_without_duplicate_retry() {
        let mut queue = SmsOutbox::default();
        queue.enqueue("one attempt only".into(), 10);
        assert!(queue.due(10).is_some());
        assert_eq!(
            queue.discard().map(|job| job.body),
            Some("one attempt only".into())
        );
        assert!(queue.due(100).is_none());
    }

    #[test]
    fn settings_backward_compatibility_uses_upstream_band_defaults() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.auto_band);
        assert!(!settings.hunter_mode);
        assert_eq!(settings.min_dwell_secs, 120);
        assert_eq!(settings.hysteresis, 0.25);
        assert_eq!(settings.quiet_spot_threshold, 3);
        assert_eq!(settings.pounce_cooldown_secs, 60);
        assert_eq!(settings.deferred_pounce_expire_secs, 120);
        assert_eq!(settings.max_deferred_pounces, 10);
        assert_eq!(settings.decode_history_retention_cycles, 8);
        assert_eq!(settings.min_snr, -20);
        assert!(settings.us_grid_only);
        assert!(settings.us_spotters_only);
        assert_eq!(settings.sms_min_interval_secs, 60);
        assert_eq!(settings.decode_ttl_secs, 120);
        assert_eq!(settings.spot_ttl_secs, 300);
    }

    #[test]
    fn settings_apply_upstream_clamps_and_safe_ranges() {
        let mut settings = Settings::default();
        settings.min_dwell_secs = 1;
        settings.hysteresis = 0.01;
        settings.quiet_spot_threshold = 0;
        settings.max_deferred_pounces = 99;
        settings.decode_history_retention_cycles = 0;
        settings.slot_min_hz = 2500;
        settings.slot_max_hz = 500;
        settings.slot_width_hz = 0;
        settings.quiet_hours_start = Some(24);
        let settings = settings.normalized();
        assert_eq!(settings.min_dwell_secs, 30);
        assert_eq!(settings.hysteresis, 0.10);
        assert_eq!(settings.quiet_spot_threshold, 1);
        assert_eq!(settings.max_deferred_pounces, 10);
        assert_eq!(settings.decode_history_retention_cycles, 2);
        assert_eq!((settings.slot_min_hz, settings.slot_max_hz), (500, 2500));
        assert_eq!(settings.slot_width_hz, 1);
        assert_eq!(settings.quiet_hours_start, None);
    }

    #[test]
    fn deserialized_zero_and_oversize_settings_are_safely_normalized() {
        let low: Settings = serde_json::from_value(serde_json::json!({
            "missedCycles": 0, "scarceMissedCycles": 0, "atnoHardMaxCycles": 0,
            "cooldownSecs": 0, "atnoCooldownSecs": 0, "decodeTtlSecs": 0,
            "spotTtlSecs": 0, "crossBandAlertCooldownSecs": 0,
            "localVerificationSecs": 0, "nonstandardQsyCooldownSecs": 0,
            "nonstandardThresholdHz": 0, "minExternalSpotters": 0,
            "slotWidthHz": 0, "minDwellSecs": 0, "quietSpotThreshold": 0,
            "pounceCooldownSecs": 0, "deferredPounceExpireSecs": 0,
            "maxDeferredPounces": 0, "decodeHistoryRetentionCycles": 0,
            "smsMinIntervalSecs": 0
        }))
        .unwrap();
        let low = low.normalized();
        assert_eq!(
            (
                low.missed_cycles,
                low.scarce_missed_cycles,
                low.atno_hard_max_cycles
            ),
            (1, 1, 1)
        );
        assert_eq!(
            (
                low.cooldown_secs,
                low.atno_cooldown_secs,
                low.decode_ttl_secs,
                low.spot_ttl_secs
            ),
            (1, 1, 1, 1)
        );
        assert_eq!(
            (
                low.cross_band_alert_cooldown_secs,
                low.local_verification_secs,
                low.nonstandard_qsy_cooldown_secs
            ),
            (1, 1, 1)
        );
        assert_eq!(
            (
                low.nonstandard_threshold_hz,
                low.min_external_spotters,
                low.slot_width_hz
            ),
            (1, 1, 1)
        );
        assert_eq!(
            (
                low.min_dwell_secs,
                low.quiet_spot_threshold,
                low.pounce_cooldown_secs,
                low.deferred_pounce_expire_secs
            ),
            (30, 1, 1, 1)
        );
        assert_eq!(
            (
                low.max_deferred_pounces,
                low.decode_history_retention_cycles,
                low.sms_min_interval_secs
            ),
            (1, 2, 10)
        );

        let high: Settings = serde_json::from_value(serde_json::json!({
            "missedCycles": 65535, "scarceMissedCycles": 65535,
            "atnoHardMaxCycles": 65535, "scarceThreshold": 65535,
            "cooldownSecs": 9999999, "atnoCooldownSecs": 9999999,
            "decodeTtlSecs": 9999999, "spotTtlSecs": 9999999,
            "crossBandAlertCooldownSecs": 9999999, "localVerificationSecs": 9999999,
            "nonstandardQsyCooldownSecs": 9999999, "nonstandardThresholdHz": 4294967295u64,
            "minExternalSpotters": 65535, "slotMinHz": 4294967295u64,
            "slotMaxHz": 4294967295u64, "slotWidthHz": 4294967295u64,
            "txOffsetDeltaHz": 2147483647, "scoreNewDxcc": 2147483647,
            "scoreNewBand": 2147483647, "scoreNewGrid": 2147483647,
            "scoreCq": 2147483647, "scoreSnrPerDb": 2147483647,
            "scoreRepeat": 2147483647, "scoreSpotter": 2147483647,
            "scoreCallingMe": 2147483647, "minSnr": 2147483647,
            "minDwellSecs": 9999999, "hysteresis": 99.0,
            "quietSpotThreshold": 4294967295u64, "pounceCooldownSecs": 9999999,
            "deferredPounceExpireSecs": 9999999, "maxDeferredPounces": 999999,
            "utcOffsetHours": 127, "decodeHistoryRetentionCycles": 65535,
            "smsMinIntervalSecs": 9999999
        }))
        .unwrap();
        let high = high.normalized();
        assert_eq!(
            (
                high.missed_cycles,
                high.scarce_missed_cycles,
                high.atno_hard_max_cycles,
                high.scarce_threshold
            ),
            (100, 100, 1000, 20)
        );
        assert_eq!(
            (
                high.cooldown_secs,
                high.atno_cooldown_secs,
                high.decode_ttl_secs,
                high.spot_ttl_secs
            ),
            (86400, 604800, 3600, 3600)
        );
        assert_eq!(
            (
                high.slot_min_hz,
                high.slot_max_hz,
                high.slot_width_hz,
                high.tx_offset_delta_hz
            ),
            (3000, 3000, 1000, 3000)
        );
        assert_eq!(
            (
                high.score_new_dxcc,
                high.score_new_band,
                high.score_new_grid,
                high.score_cq
            ),
            (100000, 100000, 100000, 100000)
        );
        assert_eq!(
            (
                high.score_snr_per_db,
                high.score_repeat,
                high.score_spotter,
                high.score_calling_me
            ),
            (100000, 100000, 100000, 100000)
        );
        assert_eq!(
            (
                high.min_snr,
                high.min_dwell_secs,
                high.hysteresis,
                high.max_deferred_pounces
            ),
            (0, 86400, 0.5, 10)
        );
        assert_eq!(
            (
                high.utc_offset_hours,
                high.decode_history_retention_cycles,
                high.sms_min_interval_secs
            ),
            (14, 1000, 3600)
        );
    }

    #[test]
    fn ordinary_band_switch_obeys_dwell_boundary_and_strict_hysteresis() {
        let mut settings = Settings::default();
        settings.enabled = true;
        settings.time_based_exclusions = false;
        settings.min_dwell_secs = 120;
        settings.decode_ttl_secs = 300;
        settings.score_spotter = 0;
        let mut c = Chaser::new(settings);
        c.set_operator_arm(true, 0);
        for (band, call, spotter, needed) in [
            ("20m", "K1A", "W1A", false),
            ("20m", "K1B", "W1B", false),
            ("20m", "K1C", "W1C", false),
            ("15m", "F4ABC", "W1D", true),
            ("15m", "F4ABC", "W1E", true),
        ] {
            c.ingest_spot(Spot {
                at: 1,
                call: call.into(),
                band: band.into(),
                freq_mhz: standard_dial(band).unwrap(),
                source: "cluster".into(),
                spotter: spotter.into(),
                mode: Some("FT8".into()),
                snr: None,
                country: None,
                grid: None,
                new_dxcc: false,
                new_band: needed,
                rarity_rank: None,
            });
        }
        assert!(c.cycle(1, "20m", 14.074, true, false).is_empty());
        for (index, at) in [1, 16, 31, 46, 61, 76, 91, 106].into_iter().enumerate() {
            let mut decode = d(&format!("K1D{index}"), at);
            decode.new_band = false;
            c.ingest_decode(decode);
        }
        assert!(c.cycle(120, "20m", 14.074, true, false).is_empty());
        assert!(
            c.cycle(121, "20m", 14.074, true, false).is_empty(),
            "the upstream band score must clear strict hysteresis"
        );
        for index in 0..10 {
            c.ingest_spot(Spot {
                at: 122,
                call: format!("F4X{index}"),
                band: "15m".into(),
                freq_mhz: 21.074,
                source: "cluster".into(),
                spotter: format!("W2{index}"),
                mode: Some("FT8".into()),
                snr: Some(-5),
                country: Some(format!("Entity {index}")),
                grid: None,
                new_dxcc: false,
                new_band: true,
                rarity_rank: None,
            });
        }
        assert!(matches!(
            c.cycle(122, "20m", 14.074, true, false).first(),
            Some(Action::ChangeBand { band, .. }) if band == "15m"
        ));
    }

    #[test]
    fn deferred_hunter_pounce_is_fifo_deduplicated_and_locally_safe() {
        let mut settings = Settings::default();
        settings.enabled = true;
        settings.hunter_mode = true;
        settings.time_based_exclusions = false;
        settings.pounce_cooldown_secs = 0;
        let mut c = Chaser::new(settings);
        c.set_operator_arm(true, 0);
        for at in [10, 11] {
            c.ingest_spot(Spot {
                at,
                call: "A52AA".into(),
                band: "15m".into(),
                freq_mhz: 21.074,
                source: "cluster".into(),
                spotter: format!("W{at}"),
                mode: Some("FT8".into()),
                snr: None,
                country: Some("Bhutan".into()),
                grid: None,
                new_dxcc: true,
                new_band: true,
                rarity_rank: Some(11),
            });
            assert!(c.cycle(at, "20m", 14.074, true, true).is_empty());
        }
        assert_eq!(c.deferred_pounces.len(), 1);
        let released = c.cycle(12, "20m", 14.074, true, false);
        assert!(matches!(
            released.first(),
            Some(Action::ChangeBand { band, .. }) if band == "15m"
        ));
        assert!(released
            .iter()
            .all(|action| !matches!(action, Action::Call { .. })));
    }

    #[test]
    fn band_change_records_feedback_and_clears_decode_target_history() {
        let mut c = Chaser::default();
        let mut settings = Settings::default();
        settings.enabled = true;
        c.configure(settings);
        c.set_operator_arm(true, 0);
        c.cycle(10, "20m", 14.074, false, false);
        c.ingest_decode(d("F4ABC", 11));
        assert_eq!(c.decode_history.len(), 1);
        c.cycle(20, "15m", 21.074, false, false);
        assert!(c.decode_history.is_empty());
        assert_eq!(c.band_history.len(), 1);
        assert_eq!(c.band_history[0].decode_count, 1);
        let state = c.persistent_state();
        let mut restored = Chaser::default();
        restored.restore_persistent_state(state);
        assert_eq!(restored.band_history.len(), 1);
    }

    #[test]
    fn automation_arm_is_distinct_and_stop_is_sticky() {
        let mut settings = Settings::default();
        settings.enabled = true;
        let mut c = Chaser::new(settings);
        c.ingest_decode(d("F4ABC", 1));
        assert!(c.cycle(2, "20m", 14.074, true, false).is_empty());
        assert!(!c.automation_armed());
        c.set_operator_arm(true, 3);
        assert!(matches!(
            c.cycle(4, "20m", 14.074, true, false).first(),
            Some(Action::Call { .. })
        ));
        c.stop(5, "operator stop");
        assert!(!c.automation_armed());
        assert!(c.cycle(6, "20m", 14.074, true, false).is_empty());
    }

    #[test]
    fn action_failure_disarms_clears_target_and_surfaces_error() {
        let mut settings = Settings::default();
        settings.enabled = true;
        let mut c = Chaser::new(settings);
        c.set_operator_arm(true, 1);
        c.ingest_decode(d("F4ABC", 1));
        c.cycle(2, "20m", 14.074, true, false);
        assert!(c.target.is_some());
        c.action_failed(3, "TX gate raced closed");
        assert!(!c.automation_armed());
        assert!(c.target.is_none());
        assert_eq!(
            c.snapshot(3, false).last_error.as_deref(),
            Some("TX gate raced closed")
        );
    }

    #[test]
    fn only_matching_fresh_canonical_qso_completes_attempt() {
        assert!(!logged_qso_matches_attempt(
            "W1AW", "20m", "FT8", 100, "F4ABC", "20m", 100
        ));
        assert!(!logged_qso_matches_attempt(
            "F4ABC", "15m", "FT8", 100, "F4ABC", "20m", 100
        ));
        assert!(!logged_qso_matches_attempt(
            "F4ABC", "20m", "SSB", 100, "F4ABC", "20m", 100
        ));
        assert!(!logged_qso_matches_attempt(
            "F4ABC", "20m", "FT8", 90, "F4ABC", "20m", 100
        ));
        assert!(logged_qso_matches_attempt(
            "F4ABC/P", "20m", "FT8", 101, "F4ABC", "20m", 100
        ));
    }

    #[test]
    fn time_schedule_excludes_low_bands_only_during_local_day() {
        let c = Chaser::default();
        assert!(!c.auto_band_allowed("80m", 12 * 3_600));
        assert!(!c.auto_band_allowed("160m", 17 * 3_600));
        assert!(c.auto_band_allowed("80m", 18 * 3_600));
        assert!(c.auto_band_allowed("20m", 12 * 3_600));
        assert!(!c.auto_band_allowed("60m", 0));
    }

    #[test]
    fn deferred_pounce_expires_strictly_after_upstream_boundary() {
        let mut settings = Settings::default();
        settings.enabled = true;
        settings.hunter_mode = true;
        settings.time_based_exclusions = false;
        settings.pounce_cooldown_secs = 0;
        let mut c = Chaser::new(settings);
        c.set_operator_arm(true, 0);
        c.ingest_spot(Spot {
            at: 10,
            call: "A52AA".into(),
            band: "15m".into(),
            freq_mhz: 21.074,
            source: "cluster".into(),
            spotter: "W1A".into(),
            mode: Some("FT8".into()),
            snr: None,
            country: Some("Bhutan".into()),
            grid: None,
            new_dxcc: true,
            new_band: true,
            rarity_rank: Some(11),
        });
        c.cycle(10, "20m", 14.074, true, true);
        assert_eq!(c.deferred_pounces.len(), 1);
        assert!(c.cycle(131, "20m", 14.074, true, false).is_empty());
        assert!(c.deferred_pounces.is_empty());
    }

    #[test]
    fn atno_policy_controls_total_cycle_cap() {
        for (policy, caps_new_dxcc) in [
            (AtnoChasePolicy::Disabled, false),
            (AtnoChasePolicy::BandSlot, false),
            (AtnoChasePolicy::AllTime, true),
            (AtnoChasePolicy::Both, true),
        ] {
            let mut settings = Settings::default();
            settings.enabled = true;
            settings.atno_chase_policy = policy;
            let mut c = Chaser::new(settings);
            c.set_operator_arm(true, 0);
            let mut decode = d("A52AA", 1);
            decode.new_dxcc = true;
            decode.new_band = false;
            c.ingest_decode(decode);
            c.cycle(2, "20m", 14.074, true, false);
            for now in 3..=22 {
                let mut heard = d("A52AA", now);
                heard.new_dxcc = true;
                c.ingest_decode(heard);
                c.cycle(now, "20m", 14.074, true, false);
            }
            assert_eq!(
                c.statistics.abandoned > 0,
                caps_new_dxcc,
                "policy {policy:?}"
            );
        }
    }

    #[test]
    fn active_selection_and_cache_expiry_match_upstream_boundaries() {
        let mut settings = Settings::default();
        settings.enabled = true;
        settings.own_country_filter = false;
        let mut at_boundary = Chaser::new(settings.clone());
        at_boundary.set_operator_arm(true, 0);
        at_boundary.ingest_decode(d("F4ABC", 1));
        assert!(matches!(
            at_boundary.cycle(121, "20m", 14.074, true, false).first(),
            Some(Action::Call { .. })
        ));

        let mut expired = Chaser::new(settings);
        expired.set_operator_arm(true, 0);
        expired.ingest_decode(d("F4ABC", 1));
        assert!(expired
            .cycle(122, "20m", 14.074, true, false)
            .iter()
            .all(|action| !matches!(action, Action::Call { .. })));
        assert!(expired.decodes.is_empty(), "cache expires at age 121 s");
    }

    #[test]
    fn ordinary_target_abandons_on_second_miss_and_can_reselect_cached_decode() {
        let mut settings = Settings::default();
        settings.enabled = true;
        settings.own_country_filter = false;
        settings.scarce_mode = false;
        let mut c = Chaser::new(settings);
        c.set_operator_arm(true, 0);
        c.ingest_decode(d("F4ABC", 1));
        c.cycle(2, "20m", 14.074, true, false);
        c.cycle(3, "20m", 14.074, true, false);
        let replay = c.cycle(4, "20m", 14.074, true, false);
        assert_eq!(c.statistics.abandoned, 1);
        assert!(matches!(replay.first(), Some(Action::Call { .. })));
    }

    #[test]
    fn continuously_heard_atno_stops_on_total_cycle_twenty() {
        let mut settings = Settings::default();
        settings.enabled = true;
        settings.own_country_filter = false;
        let mut c = Chaser::new(settings);
        c.set_operator_arm(true, 0);
        let mut first = d("F4ABC", 1);
        first.new_dxcc = true;
        c.ingest_decode(first);
        c.cycle(2, "20m", 14.074, true, false);
        for cycle in 1..20 {
            let now = 2 + cycle as u64;
            let mut heard = d("F4ABC", now);
            heard.new_dxcc = true;
            c.ingest_decode(heard);
            c.cycle(now, "20m", 14.074, true, false);
            assert!(c.target.is_some(), "ATNO ended before cycle {cycle}");
        }
        let mut heard = d("F4ABC", 22);
        heard.new_dxcc = true;
        c.ingest_decode(heard);
        c.cycle(22, "20m", 14.074, true, false);
        assert!(c.target.is_none());
        let mut boundary = d("F4ABC", 622);
        boundary.new_dxcc = true;
        c.ingest_decode(boundary);
        assert!(
            c.candidates(622).is_empty(),
            "cooldown includes exact 600 s"
        );
        let mut released = d("F4ABC", 623);
        released.new_dxcc = true;
        c.ingest_decode(released);
        assert!(!c.candidates(623).is_empty(), "cooldown clears at 601 s");
    }

    #[test]
    fn worked_cooldown_uses_base_calls_and_strict_boundary() {
        let mut settings = Settings::default();
        settings.enabled = true;
        settings.own_country_filter = false;
        let mut c = Chaser::new(settings);
        c.qso_complete(10, "F4ABC/P");
        c.ingest_decode(d("F4ABC", 310));
        assert!(c.candidates(310).is_empty());
        c.ingest_decode(d("F4ABC/MM", 311));
        assert!(!c.candidates(311).is_empty());
    }

    #[test]
    fn cross_band_nomination_locks_to_call_and_preserves_rare_frequency() {
        let mut settings = Settings::default();
        settings.enabled = true;
        settings.hunter_mode = true;
        settings.time_based_exclusions = false;
        let mut c = Chaser::new(settings);
        c.set_operator_arm(true, 0);
        c.ingest_spot(Spot {
            at: 100,
            call: "3Y0J".into(),
            band: "15m".into(),
            freq_mhz: 21.081,
            source: "cluster".into(),
            spotter: "W1A".into(),
            mode: Some("FT8".into()),
            snr: None,
            country: None,
            grid: None,
            new_dxcc: true,
            new_band: true,
            rarity_rank: Some(1),
        });
        let action = c.cycle(100, "20m", 14.074, true, false);
        assert!(matches!(
            action.first(),
            Some(Action::ChangeBand { band, dial_mhz, .. }) if band == "15m" && (*dial_mhz - 21.081).abs() < 0.000_001
        ));
        let mut unrelated = d("F4ABC", 101);
        unrelated.band = "15m".into();
        unrelated.dial_mhz = 21.081;
        c.ingest_decode(unrelated);
        assert!(c.cycle(101, "15m", 21.081, true, false).is_empty());
        let mut nominated = d("3Y0J/P", 102);
        nominated.band = "15m".into();
        nominated.dial_mhz = 21.081;
        nominated.new_dxcc = true;
        c.ingest_decode(nominated);
        assert!(matches!(
            c.cycle(102, "15m", 21.081, true, false).first(),
            Some(Action::Call { call, .. }) if call == "3Y0J/P"
        ));
    }

    #[test]
    fn same_band_external_nonstandard_qsy_waits_through_sixty_then_returns() {
        let mut settings = Settings::default();
        settings.enabled = true;
        settings.time_based_exclusions = false;
        let mut c = Chaser::new(settings);
        c.set_operator_arm(true, 0);
        c.ingest_spot(Spot {
            at: 10,
            call: "F4ABC".into(),
            band: "20m".into(),
            freq_mhz: 14.079_501,
            source: "cluster".into(),
            spotter: "W1A".into(),
            mode: Some("FT8".into()),
            snr: None,
            country: None,
            grid: None,
            new_dxcc: false,
            new_band: true,
            rarity_rank: None,
        });
        assert!(matches!(
            c.cycle(10, "20m", 14.074, true, false).first(),
            Some(Action::ChangeDial { dial_mhz, .. }) if (*dial_mhz - 14.078_001).abs() < 0.000_001
        ));
        assert!(c.cycle(70, "20m", 14.078_001, true, false).is_empty());
        assert!(matches!(
            c.cycle(71, "20m", 14.078_001, true, false).first(),
            Some(Action::ReturnToStandard { band, dial_mhz }) if band == "20m" && (*dial_mhz - 14.074).abs() < 0.000_001
        ));
    }

    #[test]
    fn decode_cache_refreshes_one_entry_and_preserves_repeat_count() {
        let mut settings = Settings::default();
        settings.enabled = true;
        settings.own_country_filter = false;
        let mut c = Chaser::new(settings);
        c.ingest_decode(d("F4ABC", 1));
        let mut refreshed = d("F4ABC", 100);
        refreshed.snr = -5;
        c.ingest_decode(refreshed);
        assert_eq!(c.decodes.len(), 1);
        let candidate = c.candidates(160).pop().expect("active at exactly 60 s");
        assert_eq!(candidate.decode_count, 2);
        assert_eq!(candidate.snr, -5);
        c.prune(221);
        assert!(c.decodes.is_empty());
        assert!(c.decode_counts.is_empty());
    }

    #[test]
    fn queue_presence_replays_remove_and_reinsert_transitions() {
        let mut ordinary_settings = Settings::default();
        ordinary_settings.enabled = true;
        ordinary_settings.own_country_filter = false;
        ordinary_settings.scarce_mode = false;
        let mut ordinary = Chaser::new(ordinary_settings);
        ordinary.set_operator_arm(true, 0);
        ordinary.ingest_decode(d("F4ABC", 1));
        ordinary.cycle(2, "20m", 14.074, true, false);
        assert!(ordinary.observe_queue(3, true).is_empty());
        assert!(ordinary.observe_queue(3, false).is_empty());
        assert!(ordinary.target.is_none());
        assert_eq!(ordinary.state, RunState::Listening);

        let mut engaged_settings = Settings::default();
        engaged_settings.enabled = true;
        engaged_settings.own_country_filter = false;
        let mut engaged = Chaser::new(engaged_settings);
        engaged.set_operator_arm(true, 0);
        let mut reply = d("F4ABC", 1);
        reply.directed_to_me = true;
        engaged.ingest_decode(reply);
        engaged.cycle(2, "20m", 14.074, true, false);
        assert!(matches!(
            engaged.observe_queue(3, false).first(),
            Some(Action::Call { call, .. }) if call == "F4ABC"
        ));
        assert!(engaged.target.is_some());

        let mut scarce_settings = Settings::default();
        scarce_settings.enabled = true;
        scarce_settings.own_country_filter = false;
        let mut scarce = Chaser::new(scarce_settings);
        scarce.set_operator_arm(true, 0);
        let mut atno = d("A52AA", 1);
        atno.new_dxcc = true;
        scarce.ingest_decode(atno);
        scarce.cycle(2, "20m", 14.074, true, false);
        assert!(matches!(
            scarce.observe_queue(3, false).first(),
            Some(Action::Call { call, .. }) if call == "A52AA"
        ));
    }

    #[test]
    fn engaged_atno_still_obeys_total_cycle_hard_cap() {
        let mut settings = Settings::default();
        settings.enabled = true;
        settings.own_country_filter = false;
        let mut c = Chaser::new(settings);
        c.set_operator_arm(true, 0);
        let mut first = d("A52AA", 1);
        first.new_dxcc = true;
        first.directed_to_me = true;
        c.ingest_decode(first);
        c.cycle(2, "20m", 14.074, true, false);
        for now in 3..=22 {
            let mut heard = d("A52AA", now);
            heard.new_dxcc = true;
            heard.directed_to_me = true;
            c.ingest_decode(heard);
            c.cycle(now, "20m", 14.074, true, false);
        }
        assert_eq!(c.statistics.abandoned, 1);
        assert!(c.target.is_none());
        assert!(c.candidates(22).is_empty(), "hard-cap cooldown is active");
    }

    #[test]
    fn same_band_nonstandard_alert_precedes_local_candidate() {
        let mut settings = Settings::default();
        settings.enabled = true;
        settings.own_country_filter = false;
        let mut c = Chaser::new(settings);
        c.set_operator_arm(true, 0);
        c.ingest_decode(d("F4ABC", 10));
        c.ingest_spot(Spot {
            at: 10,
            call: "A52AA".into(),
            band: "20m".into(),
            freq_mhz: 14.090,
            source: "cluster".into(),
            spotter: "W1A".into(),
            mode: Some("FT8".into()),
            snr: None,
            country: None,
            grid: None,
            new_dxcc: true,
            new_band: true,
            rarity_rank: Some(11),
        });
        assert!(matches!(
            c.cycle(10, "20m", 14.074, true, false).first(),
            Some(Action::ChangeDial { .. })
        ));
        assert!(c.target.is_none());
    }

    #[test]
    fn slot_selection_clamps_then_avoids_multistream_occupancy() {
        let mut settings = Settings::default();
        settings.enabled = true;
        let mut c = Chaser::new(settings);
        for (call, at, hz) in [("F1AAA", 1, 2700), ("F1BBB", 16, 1500), ("F1CCC", 17, 1445)] {
            let mut decode = d(call, at);
            decode.audio_hz = hz;
            c.ingest_decode(decode);
        }
        let selected = c.select_tx_slot("20m", 2300);
        assert!((480..=2520).contains(&selected));
        assert!(selected.abs_diff(1500) >= 60);
        assert!(selected.abs_diff(1445) >= 60);
    }
}
