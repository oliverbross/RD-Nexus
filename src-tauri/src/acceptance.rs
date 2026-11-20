// SPDX-License-Identifier: GPL-3.0-only
//! Review-candidate admission. Resolved before application state, never a profile
//! alias. No subprocesses, sockets, keychain or hardware factories are used here.
//! The fixture owner must separately verify endpoint ownership before launch.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Component, Path, PathBuf},
    sync::OnceLock,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub id: String,
    pub web_origin: String,
    pub radio_address: String,
    pub rotor_address: String,
    pub digi_audio: bool,
    #[serde(default)]
    pub interrupt_after_contact_commit_once: bool,
}

pub struct Isolation {
    pub root: PathBuf,
    pub manifest: Manifest,
    _lock: fs::File,
    root_handle: fs::File,
}

static ISOLATION: OnceLock<Isolation> = OnceLock::new();
pub fn get() -> Option<&'static Isolation> {
    ISOLATION.get()
}
pub fn active() -> bool {
    get().is_some()
}

fn checked_tree(path: &Path, budget: &mut usize) -> Result<(), String> {
    *budget = budget.checked_sub(1).ok_or("acceptance tree too large")?;
    let meta = fs::symlink_metadata(path).map_err(|_| "acceptance path missing")?;
    if meta.file_type().is_symlink() || (!meta.is_dir() && !meta.is_file()) {
        return Err("acceptance symlink or special file refused".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.is_file() && meta.nlink() != 1 {
            return Err("acceptance hardlink refused".into());
        }
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path).map_err(|_| "acceptance directory unavailable")? {
            checked_tree(
                &entry.map_err(|_| "acceptance entry unavailable")?.path(),
                budget,
            )?;
        }
    }
    Ok(())
}

impl Isolation {
    pub fn open(root: &Path) -> Result<Self, String> {
        if !root.is_absolute()
            || root
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err("acceptance root must be an absolute canonical path".into());
        }
        if fs::canonicalize(root).map_err(|_| "acceptance root missing")? != root {
            return Err("acceptance root aliases another path".into());
        }
        let mut ancestor = PathBuf::new();
        for part in root.components() {
            ancestor.push(part);
            if fs::symlink_metadata(&ancestor)
                .map_err(|_| "acceptance ancestor missing")?
                .file_type()
                .is_symlink()
            {
                return Err("acceptance ancestor symlink refused".into());
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if fs::metadata(root)
                .map_err(|_| "acceptance root missing")?
                .permissions()
                .mode()
                & 0o077
                != 0
            {
                return Err("acceptance root must already be private (0700)".into());
            }
        }
        checked_tree(root, &mut 10000)?;
        let bytes = fs::read(root.join("fixture.json")).map_err(|_| "fixture.json required")?;
        if bytes.len() > 4096 {
            return Err("fixture manifest too large".into());
        }
        let manifest: Manifest =
            serde_json::from_slice(&bytes).map_err(|_| "invalid fixture manifest")?;
        if manifest.version != 1
            || manifest.id.len() != 32
            || !manifest.id.bytes().all(|b| b.is_ascii_hexdigit())
            || root.file_name().and_then(|v| v.to_str())
                != Some(&format!("shackcq-acceptance-{}", manifest.id))
        {
            return Err("fixture identity/root mismatch".into());
        }
        let endpoint = |s: &str| -> Result<std::net::SocketAddr, String> {
            let a: std::net::SocketAddr = s.parse().map_err(|_| "invalid fixture endpoint")?;
            if a.ip() != std::net::Ipv4Addr::LOCALHOST || a.port() < 1024 {
                return Err("fixture endpoint refused".into());
            }
            Ok(a)
        };
        let radio = endpoint(&manifest.radio_address)?;
        let rotor = endpoint(&manifest.rotor_address)?;
        let web = manifest
            .web_origin
            .strip_prefix("http://")
            .ok_or("fixture origin must be HTTP loopback")?;
        let web = endpoint(web)?;
        if radio == rotor || web == radio || web == rotor {
            return Err("fixture endpoint collision".into());
        }
        for dir in [
            "config",
            "data",
            "cache",
            "session",
            "pictures",
            "documents",
        ] {
            if !root.join(dir).is_dir() {
                return Err(format!("acceptance {dir} directory required"));
            }
        }
        if manifest.digi_audio && !root.join("session/input.f32le").is_file() {
            return Err("fixture audio source required".into());
        }
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("session/instance.lock"))
            .map_err(|_| "acceptance lock unavailable")?;
        lock.try_lock()
            .map_err(|_| "acceptance instance already running")?;
        let root_handle = fs::File::open(root).map_err(|_| "acceptance root unavailable")?;
        Ok(Self {
            root: root.into(),
            manifest,
            _lock: lock,
            root_handle,
        })
    }

    pub fn revalidate(&self) -> Result<(), String> {
        if fs::canonicalize(&self.root).map_err(|_| "acceptance root missing")? != self.root {
            return Err("acceptance root replaced".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let held = self
                .root_handle
                .metadata()
                .map_err(|_| "acceptance root unavailable")?;
            let current =
                fs::symlink_metadata(&self.root).map_err(|_| "acceptance root missing")?;
            if (held.dev(), held.ino()) != (current.dev(), current.ino())
                || current.mode() & 0o077 != 0
            {
                return Err("acceptance root identity or permissions changed".into());
            }
        }
        checked_tree(&self.root, &mut 10000)
    }

    pub fn path(&self, name: &str) -> PathBuf {
        // No production fallback is representable after admission. Fail closed
        // even for timer-driven writes that did not come through Tauri IPC.
        self.revalidate()
            .expect("acceptance filesystem boundary changed");
        self.root.join(name)
    }

    /// Consume the acceptance-only interruption at the exact point where the
    /// native log append is durable but the prepared contact journal has not
    /// yet been promoted. The marker makes the fault one-shot across restart.
    pub fn take_contact_commit_interrupt(&self) -> Result<bool, String> {
        if !self.manifest.interrupt_after_contact_commit_once {
            return Ok(false);
        }
        self.revalidate()?;
        let marker = self.root.join("session/contact-commit-interrupted");
        let mut file = match fs::OpenOptions::new().create_new(true).write(true).open(&marker) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
            Err(_) => return Err("acceptance interruption marker unavailable".into()),
        };
        use std::io::Write;
        file.write_all(b"local-contact-durable-before-outbox-promotion\n")
            .and_then(|()| file.sync_all())
            .map_err(|_| "acceptance interruption marker unavailable")?;
        fs::File::open(self.root.join("session"))
            .and_then(|directory| directory.sync_all())
            .map_err(|_| "acceptance interruption marker unavailable")?;
        Ok(true)
    }

    /// Do not deserialize a remembered Settings document at all. Only the narrow
    /// manifest supplies I/O; application-controlled journals remain persistent.
    pub fn settings(&self) -> tempo_app::settings::Settings {
        let mut s = tempo_app::settings::Settings::default();
        // Use a standard-packable FT8 callsign so isolated acceptance can
        // exercise the genuine encoder and decoder path.
        s.mycall = "N0TST".into();
        s.mygrid = "AA00".into();
        s.rig_model = 2;
        s.rig_model_name = "Task-owned fake rigctld".into();
        s.rig_conn = "network".into();
        s.rig_addr = self.manifest.radio_address.clone();
        s.rigctld_port = self
            .manifest
            .radio_address
            .parse::<std::net::SocketAddr>()
            .unwrap()
            .port();
        s.serial_port.clear();
        s.ptt_method = "vox".into();
        s.icom_native_cat = false;
        s.cat_broker = false;
        s.simultaneous_radios = false;
        s.rotator_model = 0;
        s.rotator_port.clear();
        s.rotator_host = self.manifest.rotor_address.clone();
        s.audio_in = "ACCEPTANCE-NO-DEVICE".into();
        s.audio_out = "ACCEPTANCE-NO-DEVICE".into();
        s.monitor_enabled = false;
        s.wsjtx_udp = false;
        s.pskreporter = false;
        s.clock_check = false;
        // Web Audio is physical output too, even though no CPAL backend exists.
        s.band_edge_tones = false;
        s.alert_my_call = false;
        s.alert_new = false;
        s.sound_tx_state = false;
        s.sound_decode_tick = false;
        s.save_qso_wav = false;
        s.radios.clear();
        s.ensure_radio_profiles();
        s.sync_active_from_flat();
        s
    }
}

/// Admit a review candidate only with its explicit fixture argument. macOS can
/// relaunch an approved application without the original command-line arguments;
/// that launch must stop here, before any ordinary settings, credentials or I/O.
/// Stable product launches retain the complete Nexus startup and IPC surface.
pub fn initialize_if_requested() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let Some(root_arg) = root_argument(&args)? else {
        return Ok(());
    };
    let isolation = Isolation::open(Path::new(root_arg))?;
    let root = &isolation.root;
    std::env::set_var("SHACKCQ_DIGI_TX_ENABLED", "false");
    std::env::set_var(
        "SHACKCQ_ACCEPTANCE_RIGCTLD",
        &isolation.manifest.radio_address,
    );
    std::env::set_var(
        "SHACKCQ_DIGI_FIXTURE_AUDIO",
        if isolation.manifest.digi_audio {
            "1"
        } else {
            "0"
        },
    );
    std::env::set_var(
        "SHACKCQ_DIGI_FIXTURE_AUDIO_F32LE",
        root.join("session/input.f32le"),
    );
    std::env::set_var(
        "SHACKCQ_DIGI_FIXTURE_SINK_F32LE",
        root.join("session/output.f32le"),
    );
    eprintln!(
        "ACCEPTANCE {} root={} web={} radio={} rotor={}",
        isolation.manifest.id,
        root.display(),
        isolation.manifest.web_origin,
        isolation.manifest.radio_address,
        isolation.manifest.rotor_address
    );
    ISOLATION
        .set(isolation)
        .map_err(|_| "acceptance initialized twice".into())
}

fn root_argument(args: &[String]) -> Result<Option<&str>, String> {
    root_argument_for_prerelease(args, env!("CARGO_PKG_VERSION_PRE"))
}

fn root_argument_for_prerelease<'a>(
    args: &'a [String],
    prerelease: &str,
) -> Result<Option<&'a str>, String> {
    let acceptance_args: Vec<_> = args
        .iter()
        .filter(|arg| arg.starts_with("--acceptance-root"))
        .collect();
    if acceptance_args.is_empty() {
        if !prerelease.is_empty() {
            return Err(
                "review candidate requires --acceptance-root=<canonical task root>; ordinary startup is unavailable in this prerelease build".into(),
            );
        }
        return Ok(None);
    }
    if args.len() != 1 || acceptance_args.len() != 1 {
        return Err(
            "acceptance mode requires exactly --acceptance-root=<canonical task root>".into(),
        );
    }
    acceptance_args[0]
        .strip_prefix("--acceptance-root=")
        .filter(|s| !s.is_empty())
        .map(Some)
        .ok_or_else(|| "acceptance root argument required".into())
}

pub fn credential_service(original: &str) -> String {
    credential_service_for(original, get().map(|i| i.manifest.id.as_str()))
}

fn credential_service_for(original: &str, fixture_id: Option<&str>) -> String {
    fixture_id.map_or_else(
        || original.into(),
        |id| format!("{original}.acceptance.{id}"),
    )
}

pub fn origin_allowed(origin: &str) -> bool {
    get().is_none_or(|i| origin == i.manifest.web_origin)
}

pub fn ipc_allowed(command: &str) -> bool {
    matches!(
        command,
        "get_snapshot"
            | "get_settings"
            | "get_log"
            | "get_log_delta"
            | "get_log_stats"
            | "get_awards"
            | "get_meters"
            | "get_spectrum_row"
            | "get_scope_row"
            | "display_metrics"
            | "get_band_plan"
            | "get_licensed_band_plan"
            // Retained Nexus receive/decoder controls. These cannot arm the
            // universal TX latch; commands that enqueue or key transmission
            // remain denied below and SHACKCQ_DIGI_TX_ENABLED stays false.
            | "cw_decode"
            | "cw_clear"
            | "cw_skim"
            | "preview_cw"
            | "rtty_arm"
            | "rtty_auto_arm"
            | "get_rtty_state"
            | "rtty_clear"
            | "rtty_afc_reset"
            | "rtty_net"
            | "rtty_set_auto"
            | "rtty_auto_abort"
            | "rtty_stop"
            | "psk_arm"
            | "psk_auto_arm"
            | "get_psk_state"
            | "psk_clear"
            | "psk_afc_reset"
            | "psk_net"
            | "psk_set_mode"
            | "psk_stop"
            | "aprs_arm"
            | "aprs_auto_arm"
            | "get_aprs_heard"
            | "get_aprs_health"
            | "get_aprs_is_status"
            | "get_aprs_stations"
            | "js8_enter"
            | "get_js8_state"
            | "js8_set_speed"
            | "js8_set_rx_speeds"
            | "js8_cancel"
            | "js8_drop_queue"
            | "sstv_arm"
            | "sstv_manual_rx"
            | "sstv_auto_arm"
            | "sstv_auto_disarm"
            | "get_sstv_state"
            | "sstv_stop"
            // In acceptance these route only to the manifest-bound dummy
            // rigctld. They change receive mode/frequency, never PTT or Tune.
            | "set_operating_mode"
            | "set_frequency"
            | "pick_band"
            | "tune_channel"
            | "aprs_tune"
            | "sstv_tune"
            | "get_sat_transponder"
            | "point_rotator"
            | "read_rotator"
            | "sat_track_status"
            | "stop_rotator"
            | "stop_sat_track"
            | "ui_state_load"
            | "ui_state_save"
            | "halt_tx"
            | "set_tier"
            | "log_current_qso"
            | "log_qso"
            | "call_station"
            | "confirm_pending_log"
            | "discard_pending_log"
            | "shackcq_connector_capabilities"
            | "shackcq_product_mode"
            | "shackcq_set_product_mode"
            | "shackcq_pairing_status"
            | "shackcq_pair"
            | "shackcq_forget_pairing"
            | "shackcq_agent_status"
            | "shackcq_start_agent"
            | "shackcq_stop_agent"
            | "shackcq_fetch_configuration"
            | "shackcq_sync_selected_logbook"
            | "shackcq_contact_outbox_status"
            | "shackcq_flush_contact_outbox"
            | "shackcq_retry_rejected_contacts"
            | "shackcq_export_unassigned_contacts"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PathBuf {
        let id = format!(
            "{:032x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("shackcq-acceptance-{id}"));
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        for dir in [
            "config",
            "data",
            "cache",
            "session",
            "pictures",
            "documents",
        ] {
            fs::create_dir(root.join(dir)).unwrap();
        }
        fs::write(
            root.join("fixture.json"),
            serde_json::to_vec(&Manifest {
                version: 1,
                id,
                web_origin: "http://127.0.0.1:39878".into(),
                radio_address: "127.0.0.1:45432".into(),
                rotor_address: "127.0.0.1:45433".into(),
                digi_audio: false,
                interrupt_after_contact_commit_once: false,
            })
            .unwrap(),
        )
        .unwrap();
        root
    }
    #[test]
    fn contact_commit_interruption_is_acceptance_only_and_one_shot() {
        let root = fixture();
        let mut manifest: Manifest =
            serde_json::from_slice(&fs::read(root.join("fixture.json")).unwrap()).unwrap();
        manifest.interrupt_after_contact_commit_once = true;
        fs::write(
            root.join("fixture.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let isolation = Isolation::open(&root).unwrap();
        assert!(isolation.take_contact_commit_interrupt().unwrap());
        assert!(!isolation.take_contact_commit_interrupt().unwrap());
        assert_eq!(
            fs::read_to_string(root.join("session/contact-commit-interrupted")).unwrap(),
            "local-contact-durable-before-outbox-promotion\n"
        );
        drop(isolation);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn sentinel_settings_and_log_never_enter_isolated_initialization() {
        let root = fixture();
        fs::write(
            root.join("config/settings.json"),
            r#"{"serial_port":"SENTINEL-REAL-RADIO","cloudlog_key":"SENTINEL-NOT-A-CREDENTIAL"}"#,
        )
        .unwrap();
        fs::write(root.join("config/log.adi"), "SENTINEL-DEFAULT-CONTACT").unwrap();
        let i = Isolation::open(&root).unwrap();
        let s = i.settings();
        assert!(s.serial_port.is_empty());
        assert!(s.cloudlog_key.is_empty());
        assert_eq!(s.rig_addr, "127.0.0.1:45432");
        assert!(!s.monitor_enabled);
        assert!(
            !s.clock_check,
            "acceptance must not start automatic public SNTP"
        );
        assert!(!s.band_edge_tones && !s.alert_my_call && !s.alert_new);
        assert!(!s.sound_tx_state && !s.sound_decode_tick);
        assert!(!root.join("data/log.adi").exists());
        assert!(Isolation::open(&root).is_err());
        drop(i);
        assert!(Isolation::open(&root).is_ok());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn missing_malformed_and_escaping_inputs_fail_closed() {
        let root = fixture();
        fs::remove_dir(root.join("data")).unwrap();
        assert!(Isolation::open(&root).is_err());
        fs::create_dir(root.join("data")).unwrap();
        fs::write(root.join("fixture.json"), "{}").unwrap();
        assert!(Isolation::open(&root).is_err());
        assert!(Isolation::open(Path::new("relative")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn symlink_and_late_path_substitution_are_rejected() {
        let root = fixture();
        let i = Isolation::open(&root).unwrap();
        std::os::unix::fs::symlink(root.join("config"), root.join("data/escape")).unwrap();
        assert!(i.revalidate().is_err());
        drop(i);
        assert!(Isolation::open(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn ipc_has_positive_and_negative_controls() {
        assert!(ipc_allowed("shackcq_pair"));
        assert!(ipc_allowed("get_log"));
        assert!(ipc_allowed("call_station"));
        // The packaged RotorStrip must reach the same retained controller as
        // the Web bridge. Acceptance rewrites that controller to the
        // manifest's task-owned loopback fixture before these calls can run.
        assert!(ipc_allowed("get_sat_transponder"));
        assert!(ipc_allowed("point_rotator"));
        assert!(ipc_allowed("read_rotator"));
        assert!(ipc_allowed("sat_track_status"));
        assert!(ipc_allowed("stop_rotator"));
        assert!(ipc_allowed("stop_sat_track"));
        // These names must match the actual #[tauri::command] functions.  The
        // old get_ui_state/save_ui_state aliases never existed, so isolated
        // first-run state silently fell back to ephemeral WebView storage.
        assert!(ipc_allowed("ui_state_load"));
        assert!(ipc_allowed("ui_state_save"));
        // Stop is the one TX-related command that must stay available: mode
        // transitions and Quit use it to make the fixture safer.  Commands
        // that can arm or generate TX remain denied below.
        assert!(ipc_allowed("halt_tx"));
        for command in [
            "cw_decode",
            "rtty_auto_arm",
            "get_rtty_state",
            "rtty_set_auto",
            "psk_auto_arm",
            "get_psk_state",
            "aprs_auto_arm",
            "get_aprs_health",
            "js8_enter",
            "js8_set_rx_speeds",
            "sstv_auto_arm",
            "get_sstv_state",
            "sstv_manual_rx",
            "set_operating_mode",
            "set_frequency",
        ] {
            assert!(ipc_allowed(command), "{command}");
        }
        for command in [
            "get_ui_state",
            "save_ui_state",
            "save_settings",
            "set_source",
            "choose_radio",
            "set_launch_at_login",
            "upload_lotw_report",
            "get_propagation",
            "plugin:opener|open_url",
            "tune",
            "set_tx_enabled",
            "set_tune",
            "atu_tune",
            "send_cw",
            "rtty_send",
            "rtty_set_latched",
            "rtty_type",
            "rtty_auto_cq",
            "rtty_auto_answer",
            "psk_send",
            "psk_set_latched",
            "psk_type",
            "aprs_send_beacon",
            "aprs_send_message",
            "js8_send",
            "js8_send_command",
            "js8_call_cq",
            "js8_arm",
            "js8_cq_repeat",
            "sstv_send",
            "start_cq",
            "call_cq",
            "set_mode",
            "point_rotator_at_call",
            "set_sat_transponder",
            "start_sat_track",
        ] {
            assert!(!ipc_allowed(command));
        }
    }

    #[test]
    fn isolated_engine_loads_only_its_own_log_not_synthetic_default_profile() {
        let root = fixture();
        let ordinary = root.with_file_name(format!(
            "synthetic-default-{}",
            root.file_name().unwrap().to_string_lossy()
        ));
        fs::create_dir(&ordinary).unwrap();
        let default_settings = ordinary.join("settings.json");
        let default_log = ordinary.join("log.adi");
        fs::write(&default_settings, r#"{"serial_port":"SENTINEL-PHYSICAL","audio_in":"System default","cloudlog_key":"SENTINEL-SECRET"}"#).unwrap();
        fs::write(
            &default_log,
            "<CALL:6>N0FAKE<QSO_DATE:8>20260926<TIME_ON:6>000000<BAND:3>20m<MODE:3>FT8<EOR>",
        )
        .unwrap();
        let before = fs::read(&default_log).unwrap();
        let isolation = Isolation::open(&root).unwrap();
        let mut engine = tempo_app::engine::Engine::with_settings(isolation.settings());
        engine.set_log_path(isolation.path("data").join("log.adi"));
        assert!(engine.log_records().is_empty());
        assert!(engine.settings().cloudlog_key.is_empty());
        assert_eq!(fs::read(&default_log).unwrap(), before);
        drop(engine);
        drop(isolation);
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(ordinary).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn root_identity_replacement_and_external_hardlinks_are_refused() {
        let root = fixture();
        let isolation = Isolation::open(&root).unwrap();
        let moved = root.with_extension("held");
        fs::rename(&root, &moved).unwrap();
        fs::create_dir(&root).unwrap();
        assert!(isolation.revalidate().is_err());
        fs::remove_dir(&root).unwrap();
        fs::rename(&moved, &root).unwrap();
        drop(isolation);
        fs::hard_link(root.join("fixture.json"), root.join("data/linked.json")).unwrap();
        assert!(Isolation::open(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires an explicit task-owned validation root; admission only, no application launch"]
    fn staged_acceptance_manifest() {
        let root = std::env::var("SHACKCQ_FIXTURE_VALIDATION_ROOT")
            .expect("explicit fixture validation root required");
        let isolation = Isolation::open(Path::new(&root)).unwrap();
        isolation.revalidate().unwrap();
        let settings = isolation.settings();
        assert!(settings.serial_port.is_empty());
        assert!(!settings.monitor_enabled && !settings.clock_check);
        assert!(!settings.band_edge_tones && !settings.alert_my_call && !settings.alert_new);
        assert!(!isolation.manifest.digi_audio);
        println!(
            "ADMISSION_ONLY id={} root={} web={} radio={} rotor={}",
            isolation.manifest.id,
            isolation.root.display(),
            isolation.manifest.web_origin,
            isolation.manifest.radio_address,
            isolation.manifest.rotor_address
        );
    }

    #[test]
    fn review_candidate_rejects_an_approval_relaunch_without_arguments() {
        for prerelease in ["review.6", "review.7", "rc.1"] {
            assert!(root_argument_for_prerelease(&[], prerelease).is_err());
            assert!(root_argument_for_prerelease(&["--profile=real".into()], prerelease).is_err());
        }
        assert_eq!(
            root_argument(&[]).is_err(),
            !env!("CARGO_PKG_VERSION_PRE").is_empty(),
            "the compiled candidate must use its own build-time admission policy"
        );
    }

    #[test]
    fn acceptance_is_explicit_while_normal_product_arguments_remain_available() {
        assert_eq!(root_argument_for_prerelease(&[], "").unwrap(), None);
        assert_eq!(
            root_argument_for_prerelease(&["--profile=real".into()], "").unwrap(),
            None
        );
        for args in [
            vec!["--acceptance-root".into()],
            vec!["--acceptance-root=".into()],
            vec!["--acceptance-root=/fixture".into(), "--profile=real".into()],
        ] {
            assert!(root_argument(&args).is_err());
        }
        assert_eq!(
            root_argument(&["--acceptance-root=/fixture".into()]).unwrap(),
            Some("/fixture")
        );
    }

    #[test]
    fn explicit_review_launch_resolves_only_fixture_settings() {
        let root = fixture();
        let args = vec![format!("--acceptance-root={}", root.display())];
        let admitted = root_argument_for_prerelease(&args, "review.6")
            .unwrap()
            .expect("review startup cannot admit an ordinary profile");
        let isolation = Isolation::open(Path::new(admitted)).unwrap();
        let settings = isolation.settings();
        assert_eq!(settings.mycall, "N0TST");
        assert!(settings.serial_port.is_empty() && settings.cloudlog_key.is_empty());
        assert_eq!(isolation.path("config"), root.join("config"));
        assert_eq!(isolation.path("data"), root.join("data"));
        drop(isolation);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn credential_namespaces_never_fall_back_to_an_ordinary_service() {
        let id = "0123456789abcdef0123456789abcdef";
        for service in [
            "app.shackcq.desktop.nexusreview.remote",
            "shackcq-connector",
            "lotw",
        ] {
            assert_eq!(credential_service_for(service, None), service);
            assert_eq!(
                credential_service_for(service, Some(id)),
                format!("{service}.acceptance.{id}")
            );
            assert_ne!(
                credential_service_for(service, Some(id)),
                credential_service_for(service, Some("different-fixture"))
            );
        }
    }

    #[test]
    fn isolated_operator_can_commit_a_contact_without_enabling_external_outputs() {
        let root = fixture();
        let isolation = Isolation::open(&root).unwrap();
        let settings = isolation.settings();
        assert!(ipc_allowed("log_qso"));
        assert!(ipc_allowed("log_current_qso"));
        assert!(!settings.save_qso_wav);
        assert!(settings.cloudlog_key.is_empty());
        assert!(!ipc_allowed("set_tx_enabled"));
        drop(isolation);
        fs::remove_dir_all(root).unwrap();
    }
}
