//! Native Groups.io integration.
//!
//! The API key is deliberately absent from [`Store`]. It lives in the platform credential
//! vault (`keyring`: Keychain / Credential Manager / Secret Service); this profile-scoped file
//! contains only preferences, offline data and delivery receipts. Every network operation is
//! operator-triggered foreground work. In particular, an ambiguous `postdraft` result is never
//! retried automatically.

use reqwest::blocking::{multipart, Client, Response};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, MutexGuard,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const API_BASE: &str = "https://groups.io/api/v1";
const API_REVISION: &str = "2026-07-31";
const STORE_VERSION: u32 = 1;
const KEYCHAIN_SERVICE: &str = "tempo";
const KEYCHAIN_USER: &str = "groupsio-api-key";
const PAGE_SIZE: usize = 50;
const MAX_GROUPS_PER_REFRESH: usize = 50;
const MAX_TOPICS_PER_GROUP: usize = 50;
const MAX_ARCHIVE_PAGES: usize = 200;
const MAX_API_RESPONSE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_OFFICIAL_ARCHIVE_BYTES: u64 = 100 * 1024 * 1024;
static STORE_LOCK: Mutex<()> = Mutex::new(());
// Serializes every read-modify-write command across its full transaction, including provider
// work. STORE_LOCK still protects individual atomic file reads/replacements and status readers.
static STORE_TRANSACTION_LOCK: Mutex<()> = Mutex::new(());
static NETWORK_LOCK: Mutex<()> = Mutex::new(());
static ARCHIVE_CANCEL: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GroupsIoSettings {
    pub enabled: bool,
    pub default_lookback_days: u32,
    pub default_maximum_messages: usize,
    pub default_archive_depth: usize,
    pub attachment_auto_download: bool,
    pub maximum_attachment_bytes: u64,
    pub offline_retention_days: u32,
    pub refresh_on_open: bool,
    pub foreground_refresh_minutes: u32,
    pub include_muted_groups: bool,
    pub draft_retention_days: u32,
    pub show_new_message_preview: bool,
    pub muted_group_ids: Vec<String>,
    pub group_overrides: BTreeMap<String, GroupPolicy>,
}

impl Default for GroupsIoSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            default_lookback_days: 30,
            default_maximum_messages: 500,
            default_archive_depth: 2_000,
            attachment_auto_download: false,
            maximum_attachment_bytes: 8 * 1024 * 1024,
            offline_retention_days: 180,
            refresh_on_open: true,
            foreground_refresh_minutes: 15,
            include_muted_groups: false,
            draft_retention_days: 365,
            show_new_message_preview: false,
            muted_group_ids: Vec::new(),
            group_overrides: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GroupPolicy {
    pub enabled: bool,
    pub inherit_global: bool,
    pub lookback_days: u32,
    pub maximum_messages: usize,
    pub attachments: bool,
    pub retention_days: u32,
}

impl Default for GroupPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            inherit_global: true,
            lookback_days: 30,
            maximum_messages: 500,
            attachments: false,
            retention_days: 180,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub title: String,
    pub active: bool,
    pub can_post: bool,
    pub can_reply: bool,
    pub download_archives: bool,
    pub last_sync_unix: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Topic {
    pub id: String,
    pub group_id: String,
    pub subject: String,
    pub message_count: usize,
    pub last_message_unix: u64,
    pub closed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Message {
    pub id: String,
    pub group_id: String,
    pub topic_id: String,
    pub message_number: String,
    pub api_message_id: String,
    pub subject: String,
    pub sender: String,
    pub body: String,
    pub created_unix: u64,
    pub attachment_count: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Draft {
    pub id: String,
    pub group_id: String,
    pub topic_id: Option<String>,
    pub reply_message_number: Option<String>,
    pub reply_api_message_id: Option<String>,
    pub remote_draft_id: Option<String>,
    pub final_post_attempt_id: Option<String>,
    pub kind: String,
    pub subject: String,
    pub body: String,
    pub reply_destination: String,
    pub state: String,
    pub note: String,
    pub updated_unix: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Attachment {
    pub id: String,
    pub draft_id: String,
    pub filename: String,
    pub media_type: String,
    pub byte_size: u64,
    pub sha256: String,
    pub remote_id: Option<String>,
    pub upload_state: String,
    pub created_unix: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IncomingAttachment {
    pub id: String,
    pub group_id: String,
    pub message_number: String,
    pub filename: String,
    pub media_type: String,
    pub byte_size: u64,
    pub cached: bool,
    pub created_unix: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerDraft {
    pub id: String,
    pub group_id: String,
    pub kind: String,
    pub message_id: String,
    pub subject: String,
    pub body: String,
    pub attachment_count: usize,
    pub updated_unix: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAttachment {
    pub id: String,
    pub filename: String,
    pub media_type: String,
    pub byte_size: u64,
    pub url: Option<String>,
    pub preview_url: Option<String>,
    pub cached: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePayload {
    pub filename: String,
    pub media_type: String,
    pub byte_size: u64,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ArchiveState {
    pub state: String,
    pub group_id: Option<String>,
    pub downloaded: usize,
    pub total: Option<usize>,
    pub range_days: Option<u32>,
    pub page_token: Option<String>,
    pub message: String,
}

impl Default for ArchiveState {
    fn default() -> Self {
        Self {
            state: "not_started".into(),
            group_id: None,
            downloaded: 0,
            total: None,
            range_days: Some(365),
            page_token: None,
            message: "No archive sync has been started.".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Store {
    version: u32,
    settings: GroupsIoSettings,
    groups: Vec<Group>,
    topics: Vec<Topic>,
    messages: Vec<Message>,
    drafts: Vec<Draft>,
    outbox: Vec<Draft>,
    attachments: Vec<Attachment>,
    incoming_attachments: Vec<IncomingAttachment>,
    topic_tokens: BTreeMap<String, String>,
    message_tokens: BTreeMap<String, String>,
    archive: ArchiveState,
    last_sync_unix: u64,
    new_message_ids: Vec<String>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            settings: GroupsIoSettings::default(),
            groups: Vec::new(),
            topics: Vec::new(),
            messages: Vec::new(),
            drafts: Vec::new(),
            outbox: Vec::new(),
            attachments: Vec::new(),
            incoming_attachments: Vec::new(),
            topic_tokens: BTreeMap::new(),
            message_tokens: BTreeMap::new(),
            archive: ArchiveState::default(),
            last_sync_unix: 0,
            new_message_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    groups: usize,
    topics: usize,
    messages: usize,
    drafts: usize,
    outbox: usize,
    files: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    enabled: bool,
    connected: bool,
    api_revision: &'static str,
    endpoint: &'static str,
    provider_state: &'static str,
    delivery_policy: &'static str,
    counts: Counts,
    storage_bytes: u64,
    settings: GroupsIoSettings,
    last_sync_unix: u64,
    new_message_ids: Vec<String>,
    topic_tokens: BTreeMap<String, String>,
    message_tokens: BTreeMap<String, String>,
    archive: ArchiveState,
    groups: Vec<Group>,
    topics: Vec<Topic>,
    messages: Vec<Message>,
    drafts: Vec<Draft>,
    outbox: Vec<Draft>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    items: Vec<Message>,
    next_page_token: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerDraftPage {
    items: Vec<ServerDraft>,
    next_page_token: Option<String>,
}

fn store_path() -> PathBuf {
    crate::config_dir().join("groupsio.json")
}

fn files_dir() -> PathBuf {
    crate::config_dir().join("groupsio-files")
}

fn cached_file_path(namespace: &str, identity: &str) -> PathBuf {
    // Provider and persisted identifiers are untrusted. Only a fixed-width digest is ever used
    // as a filesystem component, so even a poisoned store cannot escape the cache directory.
    files_dir()
        .join(namespace)
        .join(sha256(identity.as_bytes()))
}

fn outgoing_attachment_path(id: &str) -> PathBuf {
    cached_file_path("outgoing", id)
}

fn incoming_attachment_path(group_id: &str, message_number: &str, id: &str) -> PathBuf {
    cached_file_path("incoming", &format!("{group_id}\0{message_number}\0{id}"))
}

fn safe_filename(value: &str) -> String {
    let name = Path::new(value)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment.bin");
    let safe: String = name
        .chars()
        .filter(|ch| !ch.is_control() && !matches!(ch, '/' | '\\' | ':' | '\0'))
        .take(255)
        .collect();
    if safe.trim().is_empty() {
        "attachment.bin".into()
    } else {
        safe
    }
}

fn sha256(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Groups.io attachment path has no parent".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("couldn't create Groups.io attachment storage: {e}"))?;
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| format!("couldn't stage the Groups.io attachment: {e}"))?;
    file.write_all(bytes)
        .map_err(|e| format!("couldn't write the Groups.io attachment: {e}"))?;
    file.as_file()
        .sync_all()
        .map_err(|e| format!("couldn't sync the Groups.io attachment: {e}"))?;
    persist_temp_durably(file, path, "Groups.io attachment")
}

fn keychain() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
        .map_err(|e| format!("couldn't open the system credential vault: {e}"))
}

fn api_key() -> Result<String, String> {
    keychain()?.get_password().map_err(|e| match e {
        keyring::Error::NoEntry => "Groups.io is not connected".into(),
        other => {
            format!("couldn't read the Groups.io key from the system credential vault: {other}")
        }
    })
}

fn has_key() -> bool {
    keychain()
        .and_then(|entry| entry.get_password().map_err(|e| e.to_string()))
        .is_ok()
}

fn read_store_from(path: &Path) -> Result<Store, String> {
    match fs::read(path) {
        Ok(bytes) => {
            let mut store: Store = serde_json::from_slice(&bytes)
                .map_err(|_| "Groups.io local data is unreadable".to_string())?;
            if store.version > STORE_VERSION {
                return Err("Groups.io local data was written by a newer RD-Nexus version".into());
            }
            store.version = STORE_VERSION;
            Ok(store)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
        Err(e) => Err(format!("couldn't read Groups.io local data: {e}")),
    }
}

fn read_store() -> Result<Store, String> {
    read_store_with_recovery_at(&store_path())
}

fn read_store_with_recovery_at(path: &Path) -> Result<Store, String> {
    // A free transaction lock means there is no in-process mutation that could legitimately
    // have a `posting` draft in flight. Hold it while recovering so a new mutation cannot race
    // the recovery write. When a command already owns the transaction, return its live state
    // unchanged; after a process restart the lock is free and the durable intent is recovered.
    let recovery_transaction = STORE_TRANSACTION_LOCK.try_lock().ok();
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if recovery_transaction.is_some() {
        load_recovered_store_from(path)
    } else {
        read_store_from(path)
    }
}

fn store_transaction() -> MutexGuard<'static, ()> {
    STORE_TRANSACTION_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

fn try_store_transaction() -> Result<MutexGuard<'static, ()>, String> {
    STORE_TRANSACTION_LOCK
        .try_lock()
        .map_err(|_| "another Groups.io state operation is already running".to_string())
}

#[cfg(windows)]
const WINDOWS_DURABLE_REPLACE_FLAGS: u32 = 0x0000_0001 | 0x0000_0008;
#[cfg(windows)]
const WINDOWS_FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;
#[cfg(windows)]
const WINDOWS_FILE_ATTRIBUTE_TEMPORARY: u32 = 0x0000_0100;
#[cfg(windows)]
const WINDOWS_INVALID_FILE_ATTRIBUTES: u32 = u32::MAX;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFileAttributesW(file_name: *const u16) -> u32;
    fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: u32) -> i32;
    fn SetFileAttributesW(file_name: *const u16, attributes: u32) -> i32;
}

#[cfg(windows)]
fn windows_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
fn windows_file_attributes(path: &Path) -> Result<u32, String> {
    let path = windows_path(path);
    let attributes = unsafe { GetFileAttributesW(path.as_ptr()) };
    if attributes == WINDOWS_INVALID_FILE_ATTRIBUTES {
        Err(format!(
            "couldn't read Windows file attributes: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(attributes)
    }
}

#[cfg(windows)]
fn replace_path_durably(temp: &Path, target: &Path) -> Result<(), String> {
    let existing = windows_path(temp);
    let replacement = windows_path(target);
    // NamedTempFile uses FILE_ATTRIBUTE_TEMPORARY on Windows. Normalize it before replacement so
    // the committed store is not left with temporary-file cache semantics.
    if unsafe { SetFileAttributesW(existing.as_ptr(), WINDOWS_FILE_ATTRIBUTE_NORMAL) } == 0 {
        return Err(format!(
            "couldn't normalize Windows temporary-file attributes: {}",
            std::io::Error::last_os_error()
        ));
    }
    // MOVEFILE_REPLACE_EXISTING preserves overwrite semantics; MOVEFILE_WRITE_THROUGH makes the
    // replacement durable before this function reports success.
    let result = unsafe {
        MoveFileExW(
            existing.as_ptr(),
            replacement.as_ptr(),
            WINDOWS_DURABLE_REPLACE_FLAGS,
        )
    };
    if result == 0 {
        Err(format!(
            "Windows durable atomic replacement failed: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn replace_path_durably_with_sync(
    temp: &Path,
    target: &Path,
    sync_parent: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<(), String> {
    fs::rename(temp, target).map_err(|error| format!("atomic replacement failed: {error}"))?;
    let parent = target
        .parent()
        .ok_or_else(|| "atomic replacement target has no parent".to_string())?;
    sync_parent(parent).map_err(|error| {
        format!(
            "persistence uncertain after atomic replacement; provider submission is blocked: {error}"
        )
    })
}

#[cfg(unix)]
fn replace_path_durably(temp: &Path, target: &Path) -> Result<(), String> {
    replace_path_durably_with_sync(temp, target, |parent| {
        fs::File::open(parent).and_then(|directory| directory.sync_all())
    })
}

#[cfg(not(any(unix, windows)))]
fn replace_path_durably(temp: &Path, target: &Path) -> Result<(), String> {
    fs::rename(temp, target).map_err(|error| format!("atomic replacement failed: {error}"))
}

fn persist_temp_durably(
    file: tempfile::NamedTempFile,
    path: &Path,
    label: &str,
) -> Result<(), String> {
    let temp = file.into_temp_path();
    replace_path_durably(temp.as_ref(), path)
        .map_err(|error| format!("couldn't durably commit {label}: {error}"))
}

fn mutate_store_at<T>(
    path: &Path,
    mutation: impl FnOnce(&mut Store) -> Result<T, String>,
) -> Result<(Store, T), String> {
    let _transaction = store_transaction();
    let _store = STORE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let mut store = load_recovered_store_from(path)?;
    let result = mutation(&mut store)?;
    write_store_to(path, &store)?;
    Ok((store, result))
}

fn mutate_store<T>(
    mutation: impl FnOnce(&mut Store) -> Result<T, String>,
) -> Result<(Store, T), String> {
    mutate_store_at(&store_path(), mutation)
}

fn recover_interrupted_posts(store: &mut Store) -> bool {
    let mut changed = false;
    for draft in &mut store.drafts {
        if draft.state == "posting" {
            draft.state = "delivery_unknown".into();
            draft.note = "Final post was interrupted; reconcile recent messages and server drafts before choosing delivered or retry.".into();
            draft.updated_unix = now();
            changed = true;
        }
    }
    changed
}

fn load_recovered_store_from(path: &Path) -> Result<Store, String> {
    let mut store = read_store_from(path)?;
    if recover_interrupted_posts(&mut store) {
        write_store_to(path, &store)?;
    }
    Ok(store)
}

fn write_store_to(path: &Path, store: &Store) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Groups.io data path has no parent".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("couldn't create the Groups.io data folder: {e}"))?;
    let bytes = serde_json::to_vec_pretty(store)
        .map_err(|e| format!("couldn't encode Groups.io local data: {e}"))?;
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| format!("couldn't stage Groups.io local data: {e}"))?;
    file.write_all(&bytes)
        .map_err(|e| format!("couldn't write Groups.io local data: {e}"))?;
    file.as_file()
        .sync_all()
        .map_err(|e| format!("couldn't sync Groups.io local data: {e}"))?;
    persist_temp_durably(file, path, "Groups.io local data")
}

fn write_store(store: &Store) -> Result<(), String> {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    write_store_to(&store_path(), store)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn status_for(store: Store) -> Status {
    let connected = has_key();
    let storage_bytes = fs::metadata(store_path()).map(|m| m.len()).unwrap_or(0)
        + store
            .attachments
            .iter()
            .map(|item| item.byte_size)
            .sum::<u64>()
        + store
            .incoming_attachments
            .iter()
            .map(|item| item.byte_size)
            .sum::<u64>();
    Status {
        enabled: store.settings.enabled,
        connected,
        api_revision: API_REVISION,
        endpoint: API_BASE,
        provider_state: if connected {
            "READY"
        } else {
            "CREDENTIAL_REQUIRED"
        },
        delivery_policy: "EXPLICIT_FOREGROUND_ONLY",
        counts: Counts {
            groups: store.groups.len(),
            topics: store.topics.len(),
            messages: store.messages.len(),
            drafts: store.drafts.len(),
            outbox: store.outbox.len(),
            files: store.attachments.len() + store.incoming_attachments.len(),
        },
        storage_bytes,
        settings: store.settings,
        last_sync_unix: store.last_sync_unix,
        new_message_ids: store.new_message_ids,
        topic_tokens: store.topic_tokens,
        message_tokens: store.message_tokens,
        archive: store.archive,
        groups: store.groups,
        topics: store.topics,
        messages: store.messages,
        drafts: store.drafts,
        outbox: store.outbox,
    }
}

fn client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("RD-Nexus/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "could not create the Groups.io HTTP client".into())
}

fn bearer(key: &str) -> String {
    format!("Bearer {}", key.trim())
}

fn checked(response: Response) -> Result<Response, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let label = match status.as_u16() {
        401 | 403 => "Groups.io refused the API key or group permission",
        404 => "the Groups.io API endpoint is unavailable",
        409 | 422 => "Groups.io refused the message or draft",
        413 => "the Groups.io attachment is too large",
        429 => "Groups.io rate-limited this request",
        500..=599 => "Groups.io reported a server error",
        _ => "Groups.io refused the request",
    };
    Err(format!("{label} (HTTP {})", status.as_u16()))
}

fn response_json(response: Response) -> Result<Value, String> {
    let bytes = read_limited_response(response, MAX_API_RESPONSE_BYTES, "Groups.io API response")?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(json!({}));
    }
    serde_json::from_slice(&bytes).map_err(|_| "Groups.io returned an unreadable response".into())
}

fn read_limited<R: Read>(
    reader: R,
    declared_length: Option<u64>,
    limit: u64,
    label: &str,
) -> Result<Vec<u8>, String> {
    if declared_length.is_some_and(|length| length > limit) {
        return Err(format!("{label} exceeds the {limit} byte safety limit"));
    }
    let mut bytes = Vec::with_capacity(
        declared_length
            .unwrap_or_default()
            .min(limit)
            .try_into()
            .unwrap_or(0),
    );
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| format!("{label} is unreadable"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("{label} exceeds the {limit} byte safety limit"));
    }
    Ok(bytes)
}

fn read_limited_response(response: Response, limit: u64, label: &str) -> Result<Vec<u8>, String> {
    let declared_length = response.content_length();
    read_limited(response, declared_length, limit, label)
}

fn attachment_get(client: &Client, url: &str) -> reqwest::blocking::RequestBuilder {
    // Deliberately no Authorization header: attachment URLs may be hosted away from groups.io.
    client.get(url).header(ACCEPT, "application/octet-stream")
}

fn get(client: &Client, key: &str, path: &str, query: &[(&str, String)]) -> Result<Value, String> {
    response_json(checked(
        client
            .get(format!("{API_BASE}{path}"))
            .header(AUTHORIZATION, bearer(key))
            .header(ACCEPT, "application/json")
            .query(query)
            .send()
            .map_err(|e| format!("Groups.io request failed: {e}"))?,
    )?)
}

fn post(client: &Client, key: &str, path: &str, form: &[(&str, String)]) -> Result<Value, String> {
    response_json(checked(
        client
            .post(format!("{API_BASE}{path}"))
            .header(AUTHORIZATION, bearer(key))
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .form(form)
            .send()
            .map_err(|e| format!("Groups.io request failed: {e}"))?,
    )?)
}

fn post_multipart(
    client: &Client,
    key: &str,
    path: &str,
    form: multipart::Form,
) -> Result<Value, String> {
    response_json(checked(
        client
            .post(format!("{API_BASE}{path}"))
            .header(AUTHORIZATION, bearer(key))
            .header(ACCEPT, "application/json")
            .multipart(form)
            .send()
            .map_err(|e| format!("Groups.io request failed: {e}"))?,
    )?)
}

#[derive(Debug)]
enum FinalPostError {
    Deterministic(String),
    Ambiguous(String),
}

fn record_final_post_failure(
    store: &mut Store,
    index: usize,
    mut draft: Draft,
    failure: FinalPostError,
) -> String {
    let (state, note, error) = match failure {
        FinalPostError::Deterministic(error) => (
            "failed_retryable",
            format!("Groups.io proved the message was not accepted: {error}"),
            error,
        ),
        FinalPostError::Ambiguous(error) => (
            "delivery_unknown",
            format!("Delivery must be reconciled before retry: {error}"),
            "Groups.io delivery is unknown; inspect recent messages and server drafts before retrying"
                .into(),
        ),
    };
    if state == "failed_retryable" {
        draft.final_post_attempt_id = None;
    }
    draft.state = state.into();
    draft.note = note;
    draft.updated_unix = now();
    store.drafts[index] = draft;
    error
}

fn post_final(client: &Client, key: &str, draft_id: String) -> Result<Value, FinalPostError> {
    let response = client
        .post(format!("{API_BASE}/postdraft"))
        .header(AUTHORIZATION, bearer(key))
        .header(ACCEPT, "application/json")
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .form(&[("draft_id", draft_id)])
        .send()
        .map_err(|e| FinalPostError::Ambiguous(format!("Groups.io request failed: {e}")))?;
    if !response.status().is_success() {
        let status = response.status();
        let detail = checked(response).unwrap_err();
        return Err(classify_final_post_status(status.as_u16(), detail));
    }
    response_json(response).map_err(FinalPostError::Ambiguous)
}

fn classify_final_post_status(status: u16, detail: String) -> FinalPostError {
    // Only documented validation/auth/permission rejections prove /postdraft was not accepted.
    // Missing/conflict/timeout/rate-limit and every undocumented status are fail-closed because
    // they may describe a draft that was consumed by a successful post before the response.
    if matches!(status, 400 | 401 | 403 | 422) {
        FinalPostError::Deterministic(detail)
    } else {
        FinalPostError::Ambiguous(detail)
    }
}

fn begin_final_post(store: &mut Store, index: usize, mut draft: Draft, remote_id: &str) -> Draft {
    if draft.final_post_attempt_id.is_none() {
        let identity = format!("{}\0{}\0{}", draft.id, remote_id, now());
        draft.final_post_attempt_id = Some(format!("post-{}", &sha256(identity.as_bytes())[..24]));
    }
    draft.state = "posting".into();
    draft.note = format!(
        "Final post intent {} committed before provider submission.",
        draft.final_post_attempt_id.as_deref().unwrap_or("unknown")
    );
    draft.updated_unix = now();
    store.drafts[index] = draft.clone();
    draft
}

fn persist_final_post_intent(
    store: &mut Store,
    index: usize,
    draft: Draft,
    remote_id: &str,
    persist: impl FnOnce(&Store) -> Result<(), String>,
) -> Result<Draft, String> {
    let draft = begin_final_post(store, index, draft, remote_id);
    persist(store)?;
    Ok(draft)
}

fn complete_final_post(store: &mut Store, index: usize, mut draft: Draft, response: &Value) {
    draft.state = if text(response, &["extra"])
        .to_lowercase()
        .contains("pending")
    {
        "pending_moderation".into()
    } else {
        "posted".into()
    };
    draft.note = "Submitted explicitly from RD-Nexus.".into();
    draft.updated_unix = now();
    store.drafts.remove(index);
    store.outbox.push(draft);
}

fn requires_delivery_reconciliation(draft: &Draft) -> bool {
    matches!(draft.state.as_str(), "posting" | "delivery_unknown")
}

fn ensure_draft_mutable(store: &Store, draft_id: &str) -> Result<(), String> {
    let draft = store
        .drafts
        .iter()
        .find(|draft| draft.id == draft_id)
        .ok_or_else(|| "Save the Groups.io draft before changing it".to_string())?;
    if requires_delivery_reconciliation(draft) {
        Err("Reconcile the uncertain Groups.io delivery before changing this draft".into())
    } else {
        Ok(())
    }
}

fn merge_saved_draft(store: &mut Store, draft: Draft) -> Result<(), String> {
    if let Some(saved) = store.drafts.iter_mut().find(|saved| saved.id == draft.id) {
        if requires_delivery_reconciliation(saved) {
            return Err(
                "Reconcile the uncertain Groups.io delivery before editing this draft".into(),
            );
        }
        if saved.remote_draft_id.is_none() {
            saved.group_id = draft.group_id;
            saved.topic_id = draft.topic_id;
            saved.reply_message_number = draft.reply_message_number;
            saved.reply_api_message_id = draft.reply_api_message_id;
            saved.kind = draft.kind;
        }
        saved.subject = draft.subject;
        saved.body = draft.body;
        saved.reply_destination = draft.reply_destination;
        saved.updated_unix = draft.updated_unix;
    } else {
        store.drafts.push(draft);
    }
    Ok(())
}

fn message_within_lookback(message: &Message, cutoff: u64) -> bool {
    message.created_unix == 0 || message.created_unix >= cutoff
}

fn normalize_new_message_ids(ids: &mut Vec<String>) {
    let mut seen = HashSet::new();
    ids.retain(|id| seen.insert(id.clone()));
    if ids.len() > 20 {
        ids.drain(..ids.len() - 20);
    }
}

fn uncertain_draft_ids(store: &Store) -> HashSet<String> {
    store
        .drafts
        .iter()
        .filter(|draft| requires_delivery_reconciliation(draft))
        .map(|draft| draft.id.clone())
        .collect()
}

fn safe_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .lines()
        .map(|line| format!("<p>{line}</p>"))
        .collect::<Vec<_>>()
        .join("")
}

fn object(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}
fn field<'a>(value: &'a Value, names: &[&str]) -> Option<&'a Value> {
    let obj = object(value)?;
    names.iter().find_map(|name| {
        obj.get(*name).or_else(|| {
            obj.iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value)
        })
    })
}
fn text(value: &Value, names: &[&str]) -> String {
    field(value, names)
        .and_then(|v| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        })
        .unwrap_or_default()
}
fn flag(value: &Value, names: &[&str]) -> bool {
    field(value, names).is_some_and(|v| {
        v.as_bool()
            .unwrap_or_else(|| matches!(v.as_str(), Some("true" | "yes" | "1")))
    })
}
fn number(value: &Value, names: &[&str]) -> usize {
    field(value, names)
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
        .unwrap_or(0) as usize
}
fn rows(value: &Value) -> Vec<Value> {
    if let Some(a) = value.as_array() {
        return a.clone();
    }
    for name in [
        "data",
        "items",
        "groups",
        "topics",
        "messages",
        "subs",
        "subscriptions",
    ] {
        if let Some(a) = field(value, &[name]).and_then(Value::as_array) {
            return a.clone();
        }
    }
    Vec::new()
}
fn next_token(value: &Value) -> Option<String> {
    let token = text(value, &["next_page_token", "nextPageToken", "page_token"]);
    (!token.trim().is_empty()).then_some(token)
}
fn parse_unix(value: &Value) -> u64 {
    let Some(value) = field(value, &["created", "date", "updated"]) else {
        return 0;
    };
    if let Some(number) = value.as_u64() {
        return number;
    }
    let Some(raw) = value.as_str() else {
        return 0;
    };
    raw.parse::<u64>()
        .ok()
        .or_else(|| {
            chrono::DateTime::parse_from_rfc3339(raw)
                .ok()
                .and_then(|date| u64::try_from(date.timestamp()).ok())
        })
        .unwrap_or(0)
}
fn plain_text(input: &str) -> String {
    let mut safe = input.to_string();
    for tag in ["script", "style", "iframe", "form"] {
        loop {
            let lower = safe.to_ascii_lowercase();
            let Some(start) = lower.find(&format!("<{tag}")) else {
                break;
            };
            let Some(relative_end) = lower[start..].find(&format!("</{tag}>")) else {
                safe.truncate(start);
                break;
            };
            let end = start + relative_end + tag.len() + 3;
            safe.replace_range(start..end, " ");
        }
    }
    let mut out = String::with_capacity(safe.len());
    let mut in_tag = false;
    for ch in safe.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn group_from(value: &Value) -> Option<Group> {
    let id = text(value, &["group_id", "id"]);
    if id.is_empty() {
        return None;
    }
    let name = text(value, &["group_name", "name", "email"]);
    Some(Group {
        id,
        title: text(value, &["title", "display_name"]),
        name,
        active: true,
        can_post: flag(value, &["can_post", "canPost"]),
        can_reply: flag(value, &["can_reply", "canReply"]),
        download_archives: flag(value, &["download_archives", "downloadArchives"]),
        last_sync_unix: 0,
    })
}

fn topic_from(group_id: &str, value: &Value) -> Option<Topic> {
    let id = text(value, &["id", "topic_id"]);
    if id.is_empty() {
        return None;
    }
    Some(Topic {
        id,
        group_id: group_id.into(),
        subject: text(value, &["subject", "title"]),
        message_count: number(value, &["num_messages", "message_count"]),
        last_message_unix: parse_unix(value),
        closed: flag(value, &["closed", "locked"]),
    })
}

fn message_from(group_id: &str, topic_id: &str, value: &Value) -> Option<Message> {
    let message_number = text(value, &["msg_num", "message_number", "number"]);
    let api_message_id = text(value, &["id", "message_id"]);
    if message_number.is_empty() && api_message_id.is_empty() {
        return None;
    }
    Some(Message {
        id: format!(
            "{group_id}:{}",
            if message_number.is_empty() {
                &api_message_id
            } else {
                &message_number
            }
        ),
        group_id: group_id.into(),
        topic_id: topic_id.into(),
        message_number,
        api_message_id,
        subject: text(value, &["subject"]),
        sender: text(value, &["sender_name", "sender", "from_name", "email"]),
        body: plain_text(&text(value, &["body", "message", "content"])),
        created_unix: parse_unix(value),
        attachment_count: number(value, &["num_attachments", "attachment_count"]),
    })
}

fn server_draft_from(value: &Value) -> Option<ServerDraft> {
    let id = text(value, &["id", "draft_id"]);
    if id.is_empty() {
        return None;
    }
    Some(ServerDraft {
        id,
        group_id: text(value, &["group_id"]),
        kind: text(value, &["draft_type", "type"]),
        message_id: text(value, &["message_id"]),
        subject: text(value, &["subject"]),
        body: plain_text(&text(value, &["body"])),
        attachment_count: number(value, &["num_attachments", "attachment_count"]),
        updated_unix: parse_unix(value),
    })
}

fn remote_attachment_from(value: &Value, index: usize) -> Option<RemoteAttachment> {
    let filename = safe_filename(&text(value, &["filename", "name"]));
    let id = {
        let provider = text(value, &["id", "attachment_id"]);
        if provider.is_empty() {
            format!("attachment-{index}")
        } else {
            provider
        }
    };
    Some(RemoteAttachment {
        id,
        filename,
        media_type: {
            let media = text(value, &["content_type", "media_type", "type"]);
            if media.is_empty() {
                "application/octet-stream".into()
            } else {
                media
            }
        },
        byte_size: number(value, &["size", "byte_size"]) as u64,
        url: field(value, &["download_url", "url"])
            .and_then(Value::as_str)
            .filter(|url| url.starts_with("https://"))
            .map(str::to_string),
        preview_url: field(value, &["preview_url", "thumbnail_url"])
            .and_then(Value::as_str)
            .filter(|url| url.starts_with("https://"))
            .map(str::to_string),
        cached: false,
    })
}

fn upsert<T, F>(items: &mut Vec<T>, item: T, same: F)
where
    F: Fn(&T, &T) -> bool,
{
    if let Some(slot) = items.iter_mut().find(|current| same(current, &item)) {
        *slot = item;
    } else {
        items.push(item);
    }
}

fn validate_settings(mut value: GroupsIoSettings) -> GroupsIoSettings {
    value.default_lookback_days = value.default_lookback_days.clamp(1, 3650);
    value.default_maximum_messages = value.default_maximum_messages.clamp(50, 20_000);
    value.default_archive_depth = value.default_archive_depth.clamp(100, 100_000);
    value.maximum_attachment_bytes = value
        .maximum_attachment_bytes
        .clamp(64 * 1024, 100 * 1024 * 1024);
    value.offline_retention_days = value.offline_retention_days.clamp(7, 3650);
    value.foreground_refresh_minutes = value.foreground_refresh_minutes.clamp(5, 1440);
    value.draft_retention_days = value.draft_retention_days.clamp(7, 3650);
    value.muted_group_ids.sort();
    value.muted_group_ids.dedup();
    value
}

fn sync_memberships_with(store: &mut Store, client: &Client, key: &str) -> Result<(), String> {
    let mut active = HashSet::new();
    let mut token: Option<String> = None;
    let mut seen = HashSet::new();
    for _ in 0..20 {
        let mut params = vec![("limit", "100".into())];
        if let Some(value) = token.clone() {
            params.push(("page_token", value));
        }
        let root = get(client, key, "/getsubs", &params)?;
        for group in rows(&root).iter().filter_map(group_from) {
            active.insert(group.id.clone());
            upsert(&mut store.groups, group, |a, b| a.id == b.id);
        }
        token = next_token(&root);
        let Some(next) = token.as_ref() else {
            break;
        };
        if !seen.insert(next.clone()) {
            return Err("Groups.io returned a repeated membership page token".into());
        }
    }
    for group in &mut store.groups {
        group.active = active.contains(&group.id);
    }
    store.last_sync_unix = now();
    Ok(())
}

fn effective_policy(store: &Store, group_id: &str) -> GroupPolicy {
    let mut policy = store
        .settings
        .group_overrides
        .get(group_id)
        .cloned()
        .unwrap_or_default();
    if policy.inherit_global {
        policy.lookback_days = store.settings.default_lookback_days;
        policy.maximum_messages = store.settings.default_maximum_messages;
        policy.attachments = store.settings.attachment_auto_download;
        policy.retention_days = store.settings.offline_retention_days;
    }
    policy
}

fn sync_group_with(
    store: &mut Store,
    client: &Client,
    key: &str,
    group_id: &str,
) -> Result<(), String> {
    let policy = effective_policy(store, group_id);
    if !policy.enabled {
        return Ok(());
    }
    if let Ok(perms) = get(client, key, "/getperms", &[("group_id", group_id.into())]) {
        if let Some(group) = store.groups.iter_mut().find(|g| g.id == group_id) {
            group.can_post = flag(&perms, &["can_post", "canPost"]);
            group.can_reply = flag(&perms, &["can_reply", "canReply"]);
            group.download_archives = flag(&perms, &["download_archives", "downloadArchives"]);
        }
    }
    if let Ok(feed) = get(
        client,
        key,
        "/getsinglefeed",
        &[("group_id", group_id.into())],
    ) {
        let info = field(&feed, &["group", "group_info"]).unwrap_or(&feed);
        let member = field(&feed, &["member_info"]).unwrap_or(&feed);
        if let Some(group) = store.groups.iter_mut().find(|group| group.id == group_id) {
            let title = text(info, &["title", "nice_group_name", "display_name"]);
            if !title.is_empty() {
                group.title = title;
            }
            group.can_post |= flag(member, &["can_post", "post"]);
            group.can_reply |= flag(member, &["can_reply", "reply"]);
            group.download_archives |= flag(member, &["download_archives"]);
        }
    }
    let root = get(
        client,
        key,
        "/gettopics",
        &[
            ("group_id", group_id.into()),
            ("sort_dir", "desc".into()),
            ("limit", PAGE_SIZE.to_string()),
        ],
    )?;
    if let Some(token) = next_token(&root) {
        store.topic_tokens.insert(group_id.into(), token);
    } else {
        store.topic_tokens.remove(group_id);
    }
    let topics = rows(&root);
    let prior: HashSet<String> = store
        .messages
        .iter()
        .filter(|m| m.group_id == group_id)
        .map(|m| m.id.clone())
        .collect();
    let lookback_cutoff = now().saturating_sub(u64::from(policy.lookback_days) * 86_400);
    let mut attachment_messages = Vec::new();
    for topic_value in topics.iter().take(MAX_TOPICS_PER_GROUP) {
        let Some(topic) = topic_from(group_id, topic_value) else {
            continue;
        };
        let topic_id = topic.id.clone();
        upsert(&mut store.topics, topic, |a, b| {
            a.id == b.id && a.group_id == b.group_id
        });
        let messages_root = get(
            client,
            key,
            "/gettopic",
            &[
                ("topic_id", topic_id.clone()),
                ("sort_dir", "asc".into()),
                ("limit", PAGE_SIZE.to_string()),
            ],
        )?;
        if let Some(token) = next_token(&messages_root) {
            store.message_tokens.insert(topic_id.clone(), token);
        } else {
            store.message_tokens.remove(&topic_id);
        }
        for message in rows(&messages_root)
            .iter()
            .filter_map(|v| message_from(group_id, &topic_id, v))
            .filter(|message| message_within_lookback(message, lookback_cutoff))
            .take(policy.maximum_messages)
        {
            if !prior.is_empty() && !prior.contains(&message.id) {
                store.new_message_ids.push(message.id.clone());
            }
            if policy.attachments && message.attachment_count > 0 {
                attachment_messages.push(message.clone());
            }
            upsert(&mut store.messages, message, |a, b| a.id == b.id);
        }
    }
    if policy.attachments {
        for message in &attachment_messages {
            let _ = cache_incoming_for_message(client, key, store, message);
        }
    }
    let cutoff = now().saturating_sub(u64::from(policy.retention_days) * 86_400);
    store.messages.retain(|message| {
        message.group_id != group_id || message.created_unix == 0 || message.created_unix >= cutoff
    });
    let mut group_messages: Vec<(String, u64)> = store
        .messages
        .iter()
        .filter(|message| message.group_id == group_id)
        .map(|message| (message.id.clone(), message.created_unix))
        .collect();
    group_messages.sort_by_key(|(_, created)| std::cmp::Reverse(*created));
    let keep: HashSet<String> = group_messages
        .into_iter()
        .take(policy.maximum_messages)
        .map(|(id, _)| id)
        .collect();
    store
        .messages
        .retain(|message| message.group_id != group_id || keep.contains(&message.id));
    normalize_new_message_ids(&mut store.new_message_ids);
    if let Some(group) = store.groups.iter_mut().find(|g| g.id == group_id) {
        group.last_sync_unix = now();
    }
    store.last_sync_unix = now();
    Ok(())
}

#[tauri::command(async)]
pub fn groups_io_status() -> Result<Status, String> {
    read_store().map(status_for)
}

#[tauri::command(async)]
pub fn groups_io_save_settings(settings: GroupsIoSettings) -> Result<Status, String> {
    let (store, ()) = mutate_store(|store| {
        store.settings = validate_settings(settings);
        Ok(())
    })?;
    Ok(status_for(store))
}

#[tauri::command(async)]
pub fn groups_io_connect(api_key_value: String) -> Result<Status, String> {
    let key = api_key_value.trim();
    if key.len() < 8 || key.len() > 4096 {
        return Err("Enter a valid Groups.io API key".into());
    }
    let _transaction = store_transaction();
    let _network = NETWORK_LOCK
        .try_lock()
        .map_err(|_| "another Groups.io operation is already running".to_string())?;
    let client = client()?;
    let mut store = read_store()?;
    sync_memberships_with(&mut store, &client, key)?;
    keychain()?.set_password(key).map_err(|e| {
        format!("couldn't save the Groups.io key to the system credential vault: {e}")
    })?;
    store.settings.enabled = true;
    write_store(&store)?;
    Ok(status_for(store))
}

#[tauri::command(async)]
pub fn groups_io_disconnect() -> Result<Status, String> {
    match keychain()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(e) => {
            return Err(format!(
                "couldn't clear the Groups.io key from the system credential vault: {e}"
            ))
        }
    }
    read_store().map(status_for)
}

#[tauri::command(async)]
pub fn groups_io_sync(group_id: Option<String>) -> Result<Status, String> {
    let _transaction = store_transaction();
    let _network = NETWORK_LOCK
        .try_lock()
        .map_err(|_| "another Groups.io operation is already running".to_string())?;
    let key = api_key()?;
    let client = client()?;
    let mut store = read_store()?;
    if group_id.is_none() {
        store.new_message_ids.clear();
    }
    if let Some(id) = group_id.filter(|id| !id.trim().is_empty()) {
        sync_group_with(&mut store, &client, &key, id.trim())?;
    } else {
        sync_memberships_with(&mut store, &client, &key)?;
        let ids: Vec<String> = store
            .groups
            .iter()
            .filter(|g| g.active)
            .map(|g| g.id.clone())
            .take(MAX_GROUPS_PER_REFRESH)
            .collect();
        for id in ids {
            if !store.settings.include_muted_groups && store.settings.muted_group_ids.contains(&id)
            {
                continue;
            }
            sync_group_with(&mut store, &client, &key, &id)?;
        }
    }
    let draft_cutoff =
        now().saturating_sub(u64::from(store.settings.draft_retention_days) * 86_400);
    store
        .outbox
        .retain(|draft| draft.updated_unix == 0 || draft.updated_unix >= draft_cutoff);
    write_store(&store)?;
    Ok(status_for(store))
}

#[tauri::command(async)]
pub fn groups_io_load_older_topics(
    group_id: String,
    page_token: Option<String>,
) -> Result<Status, String> {
    let _transaction = store_transaction();
    let _network = NETWORK_LOCK
        .try_lock()
        .map_err(|_| "another Groups.io operation is already running".to_string())?;
    let key = api_key()?;
    let client = client()?;
    let mut store = read_store()?;
    let token = page_token
        .filter(|value| !value.trim().is_empty())
        .or_else(|| store.topic_tokens.get(&group_id).cloned())
        .ok_or_else(|| "No older Groups.io topic page is available".to_string())?;
    let root = get(
        &client,
        &key,
        "/gettopics",
        &[
            ("group_id", group_id.clone()),
            ("sort_dir", "desc".into()),
            ("limit", PAGE_SIZE.to_string()),
            ("page_token", token),
        ],
    )?;
    for topic in rows(&root)
        .iter()
        .filter_map(|value| topic_from(&group_id, value))
    {
        upsert(&mut store.topics, topic, |a, b| {
            a.id == b.id && a.group_id == b.group_id
        });
    }
    if let Some(next) = next_token(&root) {
        store.topic_tokens.insert(group_id, next);
    } else {
        store.topic_tokens.remove(&group_id);
    }
    write_store(&store)?;
    Ok(status_for(store))
}

#[tauri::command(async)]
pub fn groups_io_load_more_messages(
    group_id: String,
    topic_id: String,
    page_token: Option<String>,
) -> Result<Status, String> {
    let _transaction = store_transaction();
    let _network = NETWORK_LOCK
        .try_lock()
        .map_err(|_| "another Groups.io operation is already running".to_string())?;
    let key = api_key()?;
    let client = client()?;
    let mut store = read_store()?;
    let token = page_token
        .filter(|value| !value.trim().is_empty())
        .or_else(|| store.message_tokens.get(&topic_id).cloned());
    let mut params = vec![
        ("topic_id", topic_id.clone()),
        ("sort_dir", "asc".into()),
        ("limit", PAGE_SIZE.to_string()),
    ];
    if let Some(value) = token {
        params.push(("page_token", value));
    }
    let root = get(&client, &key, "/gettopic", &params)?;
    for message in rows(&root)
        .iter()
        .filter_map(|value| message_from(&group_id, &topic_id, value))
    {
        upsert(&mut store.messages, message, |a, b| a.id == b.id);
    }
    if let Some(next) = next_token(&root) {
        store.message_tokens.insert(topic_id, next);
    } else {
        store.message_tokens.remove(&topic_id);
    }
    write_store(&store)?;
    Ok(status_for(store))
}

#[tauri::command(async)]
pub fn groups_io_search(
    group_id: String,
    query: String,
    online: bool,
    page_token: Option<String>,
) -> Result<SearchPage, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(SearchPage {
            items: Vec::new(),
            next_page_token: None,
        });
    }
    if !online {
        let needle = q.to_lowercase();
        let store = read_store()?;
        let items = store
            .messages
            .into_iter()
            .filter(|m| {
                (group_id.is_empty() || m.group_id == group_id)
                    && format!("{} {} {}", m.subject, m.sender, m.body)
                        .to_lowercase()
                        .contains(&needle)
            })
            .take(500)
            .collect();
        return Ok(SearchPage {
            items,
            next_page_token: None,
        });
    }
    let _transaction = store_transaction();
    let _network = NETWORK_LOCK
        .try_lock()
        .map_err(|_| "another Groups.io operation is already running".to_string())?;
    let key = api_key()?;
    let client = client()?;
    let mut params = vec![
        ("group_id", group_id.clone()),
        ("q", q.into()),
        ("limit", PAGE_SIZE.to_string()),
        ("exclude_sigs", "true".into()),
        ("collapse_topics", "false".into()),
    ];
    if let Some(token) = page_token.filter(|v| !v.is_empty()) {
        params.push(("page_token", token));
    }
    let root = get(&client, &key, "/searcharchives", &params)?;
    let items: Vec<Message> = rows(&root)
        .iter()
        .filter_map(|v| {
            let topic = text(v, &["topic_id"]);
            message_from(&group_id, &topic, v)
        })
        .collect();
    let mut store = read_store()?;
    for message in &items {
        if !store
            .topics
            .iter()
            .any(|topic| topic.id == message.topic_id)
        {
            store.topics.push(Topic {
                id: message.topic_id.clone(),
                group_id: group_id.clone(),
                subject: message.subject.clone(),
                message_count: 1,
                last_message_unix: message.created_unix,
                closed: false,
            });
        }
        upsert(&mut store.messages, message.clone(), |a, b| a.id == b.id);
    }
    write_store(&store)?;
    Ok(SearchPage {
        items,
        next_page_token: next_token(&root),
    })
}

#[tauri::command(async)]
pub fn groups_io_save_draft(mut draft: Draft) -> Result<Status, String> {
    if draft.id.trim().is_empty() {
        draft.id = format!("draft-{}", now());
    }
    draft.subject = draft.subject.trim().chars().take(500).collect();
    draft.body = draft.body.chars().take(200_000).collect();
    draft.updated_unix = now();
    if draft.state.is_empty() {
        draft.state = "draft_local".into();
    }
    let (store, ()) = mutate_store(|store| merge_saved_draft(store, draft))?;
    Ok(status_for(store))
}

#[tauri::command(async)]
pub fn groups_io_delete_draft(id: String) -> Result<Status, String> {
    let _transaction = store_transaction();
    let mut store = read_store()?;
    if store
        .drafts
        .iter()
        .any(|draft| draft.id == id && requires_delivery_reconciliation(draft))
    {
        return Err(
            "Reconcile the uncertain Groups.io delivery before deleting its receipt".into(),
        );
    }
    store.drafts.retain(|d| d.id != id);
    let removed: Vec<String> = store
        .attachments
        .iter()
        .filter(|attachment| attachment.draft_id == id)
        .map(|attachment| attachment.id.clone())
        .collect();
    store
        .attachments
        .retain(|attachment| attachment.draft_id != id);
    for attachment_id in removed {
        let _ = fs::remove_file(outgoing_attachment_path(&attachment_id));
    }
    write_store(&store)?;
    Ok(status_for(store))
}

#[tauri::command(async)]
pub fn groups_io_queue_draft(id: String) -> Result<Status, String> {
    let _transaction = store_transaction();
    let mut store = read_store()?;
    let draft = store
        .drafts
        .iter_mut()
        .find(|draft| draft.id == id)
        .ok_or_else(|| "Groups.io draft was not found".to_string())?;
    if requires_delivery_reconciliation(draft) {
        return Err("Reconcile the uncertain Groups.io delivery before queueing a retry".into());
    }
    if draft.subject.trim().is_empty() || draft.body.trim().is_empty() {
        return Err("A Groups.io subject and message are required".into());
    }
    draft.state = "queued".into();
    draft.note = "Queued locally; it can leave only after an explicit foreground action.".into();
    draft.updated_unix = now();
    write_store(&store)?;
    Ok(status_for(store))
}

#[tauri::command(async)]
pub fn groups_io_draft_attachments(draft_id: String) -> Result<Vec<Attachment>, String> {
    let store = read_store()?;
    Ok(store
        .attachments
        .into_iter()
        .filter(|attachment| attachment.draft_id == draft_id)
        .collect())
}

#[tauri::command(async)]
pub fn groups_io_add_attachment(
    draft_id: String,
    filename: String,
    media_type: String,
    bytes: Vec<u8>,
) -> Result<Vec<Attachment>, String> {
    let _transaction = store_transaction();
    let mut store = read_store()?;
    ensure_draft_mutable(&store, &draft_id)?;
    if bytes.is_empty() || bytes.len() as u64 > store.settings.maximum_attachment_bytes {
        return Err(format!(
            "Groups.io attachment exceeds the configured {} byte limit",
            store.settings.maximum_attachment_bytes
        ));
    }
    let digest = sha256(&bytes);
    let id = format!("{}-{}", now(), &digest[..24]);
    write_private_file(&outgoing_attachment_path(&id), &bytes)?;
    let requested = safe_filename(&filename);
    let existing: HashSet<String> = store
        .attachments
        .iter()
        .filter(|attachment| attachment.draft_id == draft_id)
        .map(|attachment| attachment.filename.clone())
        .collect();
    let mut stored_name = requested.clone();
    let path = Path::new(&requested);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment");
    let extension = path.extension().and_then(|value| value.to_str());
    let mut suffix = 1;
    while existing.contains(&stored_name) {
        stored_name = match extension {
            Some(extension) => format!("{stem}-{suffix}.{extension}"),
            None => format!("{stem}-{suffix}"),
        };
        suffix += 1;
    }
    store.attachments.push(Attachment {
        id,
        draft_id: draft_id.clone(),
        filename: stored_name,
        media_type: if media_type.trim().is_empty() {
            "application/octet-stream".into()
        } else {
            media_type.chars().take(160).collect()
        },
        byte_size: bytes.len() as u64,
        sha256: digest,
        remote_id: None,
        upload_state: "queued".into(),
        created_unix: now(),
    });
    write_store(&store)?;
    Ok(store
        .attachments
        .into_iter()
        .filter(|attachment| attachment.draft_id == draft_id)
        .collect())
}

#[tauri::command(async)]
pub fn groups_io_remove_attachment(
    draft_id: String,
    attachment_id: String,
) -> Result<Vec<Attachment>, String> {
    let _transaction = store_transaction();
    let mut store = read_store()?;
    ensure_draft_mutable(&store, &draft_id)?;
    let attachment = store
        .attachments
        .iter()
        .find(|attachment| attachment.id == attachment_id && attachment.draft_id == draft_id)
        .cloned()
        .ok_or_else(|| "Groups.io attachment was not found".to_string())?;
    if let Some(remote_id) = attachment.remote_id.as_ref() {
        let remote_draft = store
            .drafts
            .iter()
            .find(|draft| draft.id == draft_id)
            .and_then(|draft| draft.remote_draft_id.clone())
            .ok_or_else(|| "Groups.io remote draft id is unavailable".to_string())?;
        let _network = NETWORK_LOCK
            .try_lock()
            .map_err(|_| "another Groups.io operation is already running".to_string())?;
        post(
            &client()?,
            &api_key()?,
            "/deleteattachment",
            &[
                ("draft_id", remote_draft),
                ("attachment_id", remote_id.clone()),
            ],
        )?;
    }
    store
        .attachments
        .retain(|value| !(value.id == attachment_id && value.draft_id == draft_id));
    let _ = fs::remove_file(outgoing_attachment_path(&attachment_id));
    write_store(&store)?;
    Ok(store
        .attachments
        .into_iter()
        .filter(|attachment| attachment.draft_id == draft_id)
        .collect())
}

#[tauri::command(async)]
pub fn groups_io_server_drafts(page_token: Option<String>) -> Result<ServerDraftPage, String> {
    let _network = NETWORK_LOCK
        .try_lock()
        .map_err(|_| "another Groups.io operation is already running".to_string())?;
    let mut params = vec![("limit", PAGE_SIZE.to_string())];
    if let Some(token) = page_token.filter(|value| !value.trim().is_empty()) {
        params.push(("page_token", token));
    }
    let root = get(&client()?, &api_key()?, "/getdrafts", &params)?;
    Ok(ServerDraftPage {
        items: rows(&root).iter().filter_map(server_draft_from).collect(),
        next_page_token: next_token(&root),
    })
}

#[tauri::command(async)]
pub fn groups_io_server_draft_attachments(
    draft_id: String,
) -> Result<Vec<RemoteAttachment>, String> {
    let _network = NETWORK_LOCK
        .try_lock()
        .map_err(|_| "another Groups.io operation is already running".to_string())?;
    let root = get(
        &client()?,
        &api_key()?,
        "/getattachments",
        &[("draft_id", draft_id)],
    )?;
    Ok(rows(&root)
        .iter()
        .enumerate()
        .filter_map(|(index, value)| remote_attachment_from(value, index))
        .collect())
}

#[tauri::command(async)]
pub fn groups_io_delete_server_draft(draft_id: String) -> Result<(), String> {
    let _network = NETWORK_LOCK
        .try_lock()
        .map_err(|_| "another Groups.io operation is already running".to_string())?;
    post(
        &client()?,
        &api_key()?,
        "/deletedraft",
        &[("draft_id", draft_id)],
    )?;
    Ok(())
}

#[tauri::command(async)]
pub fn groups_io_delete_server_attachment(
    draft_id: String,
    attachment_id: String,
) -> Result<(), String> {
    let _network = NETWORK_LOCK
        .try_lock()
        .map_err(|_| "another Groups.io operation is already running".to_string())?;
    post(
        &client()?,
        &api_key()?,
        "/deleteattachment",
        &[("draft_id", draft_id), ("attachment_id", attachment_id)],
    )?;
    Ok(())
}

fn live_incoming_attachments(
    client: &Client,
    key: &str,
    group_id: &str,
    message_number: &str,
) -> Result<Vec<RemoteAttachment>, String> {
    let root = get(
        client,
        key,
        "/getmessage",
        &[
            ("group_id", group_id.into()),
            ("msg_num", message_number.into()),
        ],
    )?;
    let message = field(&root, &["message", "data"]).unwrap_or(&root);
    let values = field(message, &["attachments"])
        .or_else(|| field(&root, &["attachments"]))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| remote_attachment_from(value, index))
        .collect())
}

fn cache_incoming_for_message(
    client: &Client,
    key: &str,
    store: &mut Store,
    message: &Message,
) -> Result<(), String> {
    let items = live_incoming_attachments(client, key, &message.group_id, &message.message_number)?;
    for item in items {
        if store.incoming_attachments.iter().any(|saved| {
            saved.id == item.id
                && saved.group_id == message.group_id
                && saved.message_number == message.message_number
        }) {
            continue;
        }
        let Some(url) = item.url.clone() else {
            continue;
        };
        let response = checked(
            attachment_get(client, &url)
                .send()
                .map_err(|e| format!("Groups.io attachment download failed: {e}"))?,
        )?;
        let media_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or(&item.media_type)
            .to_string();
        let bytes = match read_limited_response(
            response,
            store.settings.maximum_attachment_bytes,
            "Groups.io attachment",
        ) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        write_private_file(
            &incoming_attachment_path(&message.group_id, &message.message_number, &item.id),
            &bytes,
        )?;
        store.incoming_attachments.push(IncomingAttachment {
            id: item.id,
            group_id: message.group_id.clone(),
            message_number: message.message_number.clone(),
            filename: item.filename,
            media_type,
            byte_size: bytes.len() as u64,
            cached: true,
            created_unix: now(),
        });
    }
    Ok(())
}

#[tauri::command(async)]
pub fn groups_io_incoming_attachments(
    group_id: String,
    message_number: String,
) -> Result<Vec<RemoteAttachment>, String> {
    let store = read_store()?;
    let cached: Vec<IncomingAttachment> = store
        .incoming_attachments
        .iter()
        .filter(|item| item.group_id == group_id && item.message_number == message_number)
        .cloned()
        .collect();
    if !has_key() || !store.settings.enabled {
        return Ok(cached
            .into_iter()
            .map(|item| RemoteAttachment {
                id: item.id,
                filename: item.filename,
                media_type: item.media_type,
                byte_size: item.byte_size,
                cached: true,
                ..RemoteAttachment::default()
            })
            .collect());
    }
    let _network = NETWORK_LOCK
        .try_lock()
        .map_err(|_| "another Groups.io operation is already running".to_string())?;
    let mut live = live_incoming_attachments(&client()?, &api_key()?, &group_id, &message_number)?;
    for item in &mut live {
        item.cached = cached.iter().any(|saved| saved.id == item.id);
    }
    for saved in cached {
        if !live.iter().any(|item| item.id == saved.id) {
            live.push(RemoteAttachment {
                id: saved.id,
                filename: saved.filename,
                media_type: saved.media_type,
                byte_size: saved.byte_size,
                cached: true,
                ..RemoteAttachment::default()
            });
        }
    }
    Ok(live)
}

#[tauri::command(async)]
pub fn groups_io_download_incoming_attachment(
    group_id: String,
    message_number: String,
    attachment_id: String,
    preview: bool,
) -> Result<FilePayload, String> {
    let _transaction = store_transaction();
    let mut store = read_store()?;
    if let Some(saved) = store
        .incoming_attachments
        .iter()
        .find(|item| {
            item.id == attachment_id
                && item.group_id == group_id
                && item.message_number == message_number
        })
        .cloned()
    {
        let bytes = fs::read(incoming_attachment_path(
            &saved.group_id,
            &saved.message_number,
            &saved.id,
        ))
        .map_err(|e| format!("couldn't read the cached Groups.io attachment: {e}"))?;
        return Ok(FilePayload {
            filename: saved.filename,
            media_type: saved.media_type,
            byte_size: bytes.len() as u64,
            bytes,
        });
    }
    let _network = NETWORK_LOCK
        .try_lock()
        .map_err(|_| "another Groups.io operation is already running".to_string())?;
    let client = client()?;
    let items = live_incoming_attachments(&client, &api_key()?, &group_id, &message_number)?;
    let item = items
        .into_iter()
        .find(|item| item.id == attachment_id)
        .ok_or_else(|| "Groups.io attachment was not found".to_string())?;
    let url = if preview {
        item.preview_url.clone().or(item.url.clone())
    } else {
        item.url.clone()
    }
    .ok_or_else(|| "Groups.io attachment URL is unavailable".to_string())?;
    if !url.starts_with("https://") {
        return Err("Groups.io attachment URL must use HTTPS".into());
    }
    let response = checked(
        attachment_get(&client, &url)
            .send()
            .map_err(|e| format!("Groups.io attachment download failed: {e}"))?,
    )?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or(&item.media_type)
        .to_string();
    let limit = if preview {
        8 * 1024 * 1024
    } else {
        store.settings.maximum_attachment_bytes as usize
    };
    let bytes = read_limited_response(response, limit as u64, "Groups.io attachment")?;
    if !preview {
        write_private_file(
            &incoming_attachment_path(&group_id, &message_number, &item.id),
            &bytes,
        )?;
        upsert(
            &mut store.incoming_attachments,
            IncomingAttachment {
                id: item.id.clone(),
                group_id,
                message_number,
                filename: item.filename.clone(),
                media_type: content_type.clone(),
                byte_size: bytes.len() as u64,
                cached: true,
                created_unix: now(),
            },
            |a, b| a.id == b.id && a.group_id == b.group_id && a.message_number == b.message_number,
        );
        write_store(&store)?;
    }
    Ok(FilePayload {
        filename: item.filename,
        media_type: content_type,
        byte_size: bytes.len() as u64,
        bytes,
    })
}

#[tauri::command(async)]
pub fn groups_io_official_archive(
    group_id: String,
    start_message_number: Option<String>,
) -> Result<FilePayload, String> {
    let _network = NETWORK_LOCK
        .try_lock()
        .map_err(|_| "another Groups.io operation is already running".to_string())?;
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("RD-Nexus/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "could not create the Groups.io archive client")?;
    let mut query = vec![("group_id", group_id.clone())];
    if let Some(start) = start_message_number.filter(|value| !value.trim().is_empty()) {
        query.push(("start_msg_num", start));
    }
    let response = checked(
        client
            .get(format!("{API_BASE}/downloadarchives"))
            .header(AUTHORIZATION, bearer(&api_key()?))
            .query(&query)
            .send()
            .map_err(|e| format!("Groups.io official archive download failed: {e}"))?,
    )?;
    let media_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/zip")
        .to_string();
    let bytes = read_limited_response(
        response,
        MAX_OFFICIAL_ARCHIVE_BYTES,
        "Groups.io official archive",
    )?;
    Ok(FilePayload {
        filename: format!("groupsio-{group_id}-archive.zip"),
        media_type,
        byte_size: bytes.len() as u64,
        bytes,
    })
}

#[tauri::command(async)]
pub fn groups_io_send_draft(id: String, confirmed: bool) -> Result<Status, String> {
    if !confirmed {
        return Err("Confirm the foreground send before posting to Groups.io".into());
    }
    let _transaction = store_transaction();
    let _network = NETWORK_LOCK
        .try_lock()
        .map_err(|_| "another Groups.io operation is already running".to_string())?;
    let key = api_key()?;
    let client = client()?;
    let mut store = read_store()?;
    let index = store
        .drafts
        .iter()
        .position(|d| d.id == id)
        .ok_or_else(|| "Groups.io draft was not found".to_string())?;
    let mut draft = store.drafts[index].clone();
    if requires_delivery_reconciliation(&draft) {
        return Err(
            "Groups.io delivery must be reconciled before this draft can be sent again".into(),
        );
    }
    if draft.subject.trim().is_empty() || draft.body.trim().is_empty() {
        return Err("A Groups.io subject and message are required".into());
    }
    let fail = |store: &mut Store, index: usize, draft: &mut Draft, error: String| {
        draft.state = "failed_retryable".into();
        draft.note = error.clone();
        draft.updated_unix = now();
        store.drafts[index] = draft.clone();
        let _ = write_store(store);
        error
    };
    let remote_id = if let Some(existing) = draft
        .remote_draft_id
        .clone()
        .filter(|value| !value.is_empty())
    {
        existing
    } else {
        draft.state = "creating_remote".into();
        store.drafts[index] = draft.clone();
        write_store(&store)?;
        let kind = if draft.kind == "reply" {
            "reply"
        } else {
            "post"
        };
        let mut create = vec![
            ("group_id", draft.group_id.clone()),
            ("draft_type", format!("draft_type_{kind}")),
        ];
        if draft.kind == "reply" {
            let mut message_id = draft.reply_api_message_id.clone().unwrap_or_default();
            if message_id.is_empty() {
                let message_number = draft.reply_message_number.clone().unwrap_or_default();
                let fetched = get(
                    &client,
                    &key,
                    "/getmessage",
                    &[
                        ("group_id", draft.group_id.clone()),
                        ("msg_num", message_number),
                    ],
                )
                .map_err(|error| fail(&mut store, index, &mut draft, error))?;
                let provider = field(&fetched, &["message", "data"]).unwrap_or(&fetched);
                message_id = text(provider, &["id", "message_id"]);
            }
            if message_id.is_empty() {
                return Err(fail(
                    &mut store,
                    index,
                    &mut draft,
                    "Groups.io reply message id is unavailable".into(),
                ));
            }
            create.push(("message_id", message_id));
        }
        let created = post(&client, &key, "/newdraft", &create)
            .map_err(|error| fail(&mut store, index, &mut draft, error))?;
        let created_id = text(&created, &["draft_id", "id"]);
        if created_id.is_empty() {
            return Err(fail(
                &mut store,
                index,
                &mut draft,
                "Groups.io did not return a draft id".into(),
            ));
        }
        created_id
    };
    draft.remote_draft_id = Some(remote_id.clone());
    draft.state = "updating_remote".into();
    store.drafts[index] = draft.clone();
    write_store(&store)?;
    if let Err(error) = post(
        &client,
        &key,
        "/updatedraft",
        &[
            ("draft_id", remote_id.clone()),
            ("subject", draft.subject.clone()),
            ("body", safe_html(&draft.body)),
            ("body_type", "html".into()),
            ("reply_to", draft.reply_destination.clone()),
        ],
    ) {
        return Err(fail(&mut store, index, &mut draft, error));
    }
    let outgoing: Vec<Attachment> = store
        .attachments
        .iter()
        .filter(|attachment| attachment.draft_id == draft.id && attachment.remote_id.is_none())
        .cloned()
        .collect();
    for attachment in outgoing {
        draft.state = "uploading".into();
        store.drafts[index] = draft.clone();
        write_store(&store)?;
        let bytes = fs::read(outgoing_attachment_path(&attachment.id)).map_err(|error| {
            fail(
                &mut store,
                index,
                &mut draft,
                format!("couldn't read the queued Groups.io attachment: {error}"),
            )
        })?;
        let part = multipart::Part::bytes(bytes)
            .file_name(attachment.filename.clone())
            .mime_str(&attachment.media_type)
            .map_err(|_| {
                fail(
                    &mut store,
                    index,
                    &mut draft,
                    "Groups.io attachment media type is invalid".into(),
                )
            })?;
        let form = multipart::Form::new()
            .text("draft_id", remote_id.clone())
            .text("inline", "false")
            .part("fileupload", part);
        let uploaded = post_multipart(&client, &key, "/uploadattachments", form)
            .map_err(|error| fail(&mut store, index, &mut draft, error))?;
        let confirmation = rows(&uploaded)
            .into_iter()
            .find(|value| text(value, &["filename"]) == attachment.filename)
            .or_else(|| rows(&uploaded).into_iter().next());
        let remote_attachment_id = confirmation
            .as_ref()
            .map(|value| text(value, &["id", "attachment_id"]))
            .unwrap_or_default();
        if remote_attachment_id.is_empty() {
            return Err(fail(
                &mut store,
                index,
                &mut draft,
                "Groups.io did not confirm the attachment upload".into(),
            ));
        }
        if let Some(saved) = store
            .attachments
            .iter_mut()
            .find(|value| value.id == attachment.id)
        {
            saved.remote_id = Some(remote_attachment_id);
            saved.upload_state = "uploaded".into();
        }
        write_store(&store)?;
    }
    draft = persist_final_post_intent(&mut store, index, draft, &remote_id, write_store)?;
    match post_final(&client, &key, remote_id) {
        Ok(value) => complete_final_post(&mut store, index, draft, &value),
        Err(failure) => {
            let error = record_final_post_failure(&mut store, index, draft, failure);
            write_store(&store)?;
            return Err(error);
        }
    }
    write_store(&store)?;
    Ok(status_for(store))
}

#[tauri::command(async)]
pub fn groups_io_reconcile_draft(id: String, resolution: String) -> Result<Status, String> {
    let _transaction = store_transaction();
    let mut store = read_store()?;
    let index = store
        .drafts
        .iter()
        .position(|draft| draft.id == id)
        .ok_or_else(|| "Groups.io draft was not found".to_string())?;
    if !matches!(
        store.drafts[index].state.as_str(),
        "posting" | "delivery_unknown"
    ) {
        return Err("Only an uncertain Groups.io delivery can be reconciled".into());
    }
    match resolution.as_str() {
        "delivered" => {
            let mut draft = store.drafts.remove(index);
            draft.state = "reconciled_posted".into();
            draft.note = "Operator reconciled this final-post attempt as delivered.".into();
            draft.updated_unix = now();
            store.outbox.push(draft);
        }
        "not_delivered" => {
            let draft = &mut store.drafts[index];
            draft.state = "failed_retryable".into();
            draft.note =
                "Operator reconciled this attempt as not delivered; explicit retry is now allowed."
                    .into();
            draft.final_post_attempt_id = None;
            draft.updated_unix = now();
        }
        _ => return Err("Choose delivered or not_delivered for Groups.io reconciliation".into()),
    }
    write_store(&store)?;
    Ok(status_for(store))
}

#[tauri::command(async)]
pub fn groups_io_archive(group_id: String, range_days: Option<u32>) -> Result<Status, String> {
    // Never queue an archive behind another transaction: a queued start could otherwise clear a
    // pause signal that belongs to the currently running archive.
    let _transaction = try_store_transaction()?;
    let _network = NETWORK_LOCK
        .try_lock()
        .map_err(|_| "another Groups.io operation is already running".to_string())?;
    ARCHIVE_CANCEL.store(false, Ordering::Release);
    let key = api_key()?;
    let client = client()?;
    let mut store = read_store()?;
    let cutoff = range_days.map(|days| now().saturating_sub(u64::from(days) * 86_400));
    let mut token = if store.archive.group_id.as_deref() == Some(group_id.as_str())
        && store.archive.range_days == range_days
    {
        store.archive.page_token.clone()
    } else {
        None
    };
    store.archive = ArchiveState {
        state: "running".into(),
        group_id: Some(group_id.clone()),
        downloaded: 0,
        total: None,
        range_days,
        page_token: token.clone(),
        message: "Downloading complete archive pages.".into(),
    };
    write_store(&store)?;
    for _ in 0..MAX_ARCHIVE_PAGES {
        if ARCHIVE_CANCEL.load(Ordering::Acquire) {
            store.archive.state = "paused".into();
            store.archive.message = "Archive paused; completed pages were preserved.".into();
            break;
        }
        let mut params = vec![
            ("group_id", group_id.clone()),
            ("limit", "100".into()),
            ("sort_dir", "desc".into()),
        ];
        if let Some(value) = token.clone() {
            params.push(("page_token", value));
        }
        let root = match get(&client, &key, "/getmessages", &params) {
            Ok(root) => root,
            Err(error) => {
                store.archive.state = "failed".into();
                store.archive.message = error.clone();
                write_store(&store)?;
                return Err(error);
            }
        };
        let values = rows(&root);
        let mut reached_cutoff = false;
        for message in values.iter().filter_map(|v| {
            let topic = text(v, &["topic_id"]);
            message_from(&group_id, &topic, v)
        }) {
            if cutoff
                .is_some_and(|minimum| message.created_unix > 0 && message.created_unix < minimum)
            {
                reached_cutoff = true;
                break;
            }
            upsert(&mut store.messages, message, |a, b| a.id == b.id);
            store.archive.downloaded += 1;
        }
        token = if reached_cutoff {
            None
        } else {
            next_token(&root)
        };
        store.archive.page_token = token.clone();
        write_store(&store)?;
        if token.is_none() || store.archive.downloaded >= store.settings.default_archive_depth {
            store.archive.state = "complete".into();
            store.archive.message = "Offline archive is current for the requested range.".into();
            break;
        }
    }
    if store.archive.state == "running" {
        store.archive.state = "paused".into();
        store.archive.message =
            "Archive paused at the foreground safety bound; resume to continue.".into();
    }
    write_store(&store)?;
    Ok(status_for(store))
}

#[tauri::command(async)]
pub fn groups_io_pause_archive() -> Result<Status, String> {
    // Signal the running archive before waiting for its transaction; otherwise pause would wait
    // behind the archive and could never cause its loop to stop.
    ARCHIVE_CANCEL.store(true, Ordering::Release);
    let _transaction = store_transaction();
    let mut store = read_store()?;
    store.archive.state = "paused".into();
    store.archive.message = "Archive paused; completed pages were preserved.".into();
    write_store(&store)?;
    Ok(status_for(store))
}

#[tauri::command(async)]
pub fn groups_io_clear_cache(delete_all: bool) -> Result<Status, String> {
    let _transaction = store_transaction();
    let mut store = read_store()?;
    for attachment in &store.incoming_attachments {
        let _ = fs::remove_file(incoming_attachment_path(
            &attachment.group_id,
            &attachment.message_number,
            &attachment.id,
        ));
    }
    store.incoming_attachments.clear();
    store.groups.clear();
    store.topics.clear();
    store.messages.clear();
    store.topic_tokens.clear();
    store.message_tokens.clear();
    store.archive = ArchiveState::default();
    store.new_message_ids.clear();
    if delete_all {
        let protected = uncertain_draft_ids(&store);
        for attachment in store
            .attachments
            .iter()
            .filter(|attachment| !protected.contains(&attachment.draft_id))
        {
            let _ = fs::remove_file(outgoing_attachment_path(&attachment.id));
        }
        store
            .attachments
            .retain(|attachment| protected.contains(&attachment.draft_id));
        store
            .drafts
            .retain(|draft| requires_delivery_reconciliation(draft));
        store.outbox.clear();
    }
    write_store(&store)?;
    Ok(status_for(store))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rd-nexus-groupsio-{name}-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn defaults_are_foreground_only_and_do_not_contain_a_credential() {
        let mut store = Store::default();
        store.drafts.push(Draft {
            body: "positive-control-visible-content".into(),
            ..Draft::default()
        });
        let encoded = serde_json::to_string(&store).unwrap();
        assert!(GroupsIoSettings::default().refresh_on_open);
        assert!(encoded.contains("positive-control-visible-content"));
        assert!(!encoded.to_lowercase().contains("api_key"));
        assert!(!encoded.to_lowercase().contains("apikey"));
        assert!(!encoded.contains("sentinel-user-api-secret"));
    }

    #[test]
    fn store_round_trip_is_atomic_and_preserves_offline_content() {
        let path = temp_file("roundtrip");
        let mut store = Store::default();
        store.groups.push(Group {
            id: "42".into(),
            name: "test".into(),
            active: true,
            ..Group::default()
        });
        write_store_to(&path, &store).unwrap();
        let loaded = read_store_from(&path).unwrap();
        assert_eq!(loaded.groups[0].id, "42");
        assert!(!path.with_extension("json.tmp").exists());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn provider_html_is_reduced_to_plain_text() {
        assert_eq!(
            plain_text("<p>Hello &amp; <b>world</b></p><script>bad()</script>"),
            "Hello & world"
        );
        assert!(!plain_text("<img src=x onerror=bad()>").contains("onerror"));
    }

    #[test]
    fn settings_are_bounded_before_persistence() {
        let mut settings = GroupsIoSettings::default();
        settings.default_maximum_messages = usize::MAX;
        settings.maximum_attachment_bytes = u64::MAX;
        settings.foreground_refresh_minutes = 0;
        let settings = validate_settings(settings);
        assert_eq!(settings.default_maximum_messages, 20_000);
        assert_eq!(settings.maximum_attachment_bytes, 100 * 1024 * 1024);
        assert_eq!(settings.foreground_refresh_minutes, 5);
    }

    #[test]
    fn parser_accepts_groups_io_snake_case_fields() {
        let group =
            group_from(&json!({"group_id":"7","group_name":"nexus","can_post":true})).unwrap();
        assert_eq!(group.id, "7");
        assert!(group.can_post);
        let message = message_from(
            "7",
            "9",
            &json!({"msg_num":11,"subject":"Hello","body":"<p>Safe</p>"}),
        )
        .unwrap();
        assert_eq!(message.id, "7:11");
        assert_eq!(message.body, "Safe");
        assert_eq!(
            parse_unix(&json!({"created":"2026-09-27T12:00:00Z"})),
            1_790_510_400
        );
    }

    #[test]
    fn untrusted_attachment_ids_never_become_path_components() {
        let root = files_dir().join("incoming");
        for id in [
            "../settings.json",
            "/tmp/escape",
            r"..\escape",
            "..∕escape",
            "..⁄escape",
        ] {
            let path = incoming_attachment_path("../group", "/message", id);
            assert_eq!(path.parent(), Some(root.as_path()));
            let name = path.file_name().and_then(|value| value.to_str()).unwrap();
            assert_eq!(name.len(), 64);
            assert!(name.chars().all(|value| value.is_ascii_hexdigit()));
        }
        let poisoned = IncomingAttachment {
            id: "../../groupsio.json".into(),
            group_id: "/absolute".into(),
            message_number: r"..\message".into(),
            ..IncomingAttachment::default()
        };
        let path =
            incoming_attachment_path(&poisoned.group_id, &poisoned.message_number, &poisoned.id);
        assert!(path.starts_with(root));
    }

    #[test]
    fn streaming_limit_rejects_declared_missing_and_lying_lengths() {
        let payload = vec![7_u8; 17];
        assert!(read_limited(payload.as_slice(), Some(17), 16, "payload").is_err());
        assert!(read_limited(payload.as_slice(), None, 16, "payload").is_err());
        assert!(read_limited(payload.as_slice(), Some(4), 16, "payload").is_err());
        assert_eq!(
            read_limited(&payload[..16], None, 16, "payload")
                .unwrap()
                .len(),
            16
        );
    }

    #[test]
    fn attachment_requests_never_carry_the_groups_io_bearer() {
        let request = attachment_get(&client().unwrap(), "https://attachments.example/file")
            .build()
            .unwrap();
        assert!(!request.headers().contains_key(AUTHORIZATION));
    }

    #[test]
    fn final_post_failures_distinguish_retryable_from_delivery_unknown() {
        let draft = Draft {
            id: "draft-1".into(),
            state: "posting".into(),
            ..Draft::default()
        };
        let mut deterministic = Store::default();
        deterministic.drafts.push(draft.clone());
        let error = record_final_post_failure(
            &mut deterministic,
            0,
            draft.clone(),
            FinalPostError::Deterministic("HTTP 422".into()),
        );
        assert_eq!(error, "HTTP 422");
        assert_eq!(deterministic.drafts[0].state, "failed_retryable");

        let mut ambiguous = Store::default();
        ambiguous.drafts.push(draft.clone());
        let error = record_final_post_failure(
            &mut ambiguous,
            0,
            draft,
            FinalPostError::Ambiguous("connection closed after send".into()),
        );
        assert!(error.contains("delivery is unknown"));
        assert_eq!(ambiguous.drafts[0].state, "delivery_unknown");
        assert!(ambiguous.drafts[0].note.contains("reconciled before retry"));
    }

    #[test]
    fn final_post_http_boundaries_fail_closed() {
        for status in [400, 401, 403, 422] {
            assert!(matches!(
                classify_final_post_status(status, format!("HTTP {status}")),
                FinalPostError::Deterministic(_)
            ));
        }
        for status in [418, 499] {
            let draft = Draft {
                id: format!("ambiguous-{status}"),
                state: "posting".into(),
                ..Draft::default()
            };
            let mut store = Store::default();
            store.drafts.push(draft.clone());
            let failure = classify_final_post_status(status, format!("HTTP {status}"));
            assert!(matches!(failure, FinalPostError::Ambiguous(_)));
            record_final_post_failure(&mut store, 0, draft, failure);
            assert_eq!(store.drafts[0].state, "delivery_unknown");
            assert!(requires_delivery_reconciliation(&store.drafts[0]));
        }
        for status in [404, 408, 409, 425, 429, 500, 502, 503, 504] {
            assert!(matches!(
                classify_final_post_status(status, format!("HTTP {status}")),
                FinalPostError::Ambiguous(_)
            ));
        }
    }

    #[test]
    fn lookback_filters_old_messages_but_keeps_unknown_dates() {
        let cutoff = 1_000;
        assert!(!message_within_lookback(
            &Message {
                created_unix: 999,
                ..Message::default()
            },
            cutoff
        ));
        assert!(message_within_lookback(
            &Message {
                created_unix: 1_000,
                ..Message::default()
            },
            cutoff
        ));
        assert!(message_within_lookback(&Message::default(), cutoff));
    }

    #[test]
    fn retryable_draft_edit_preserves_remote_identity_and_state() {
        let mut store = Store::default();
        store.drafts.push(Draft {
            id: "draft-1".into(),
            group_id: "original-group".into(),
            topic_id: Some("original-topic".into()),
            reply_message_number: Some("7".into()),
            reply_api_message_id: Some("api-original".into()),
            kind: "reply".into(),
            remote_draft_id: Some("remote-1".into()),
            state: "failed_retryable".into(),
            note: "retry".into(),
            ..Draft::default()
        });
        merge_saved_draft(
            &mut store,
            Draft {
                id: "draft-1".into(),
                group_id: "different-group".into(),
                topic_id: Some("different-topic".into()),
                reply_message_number: Some("99".into()),
                reply_api_message_id: Some("api-replacement".into()),
                kind: "new_topic".into(),
                subject: "Edited".into(),
                body: "New body".into(),
                state: "draft_local".into(),
                ..Draft::default()
            },
        )
        .unwrap();
        let saved = &store.drafts[0];
        assert_eq!(saved.remote_draft_id.as_deref(), Some("remote-1"));
        assert_eq!(saved.state, "failed_retryable");
        assert_eq!(saved.group_id, "original-group");
        assert_eq!(saved.topic_id.as_deref(), Some("original-topic"));
        assert_eq!(saved.reply_message_number.as_deref(), Some("7"));
        assert_eq!(
            saved.reply_api_message_id.as_deref(),
            Some("api-original")
        );
        assert_eq!(saved.kind, "reply");
        assert_eq!(saved.subject, "Edited");
        assert_eq!(saved.body, "New body");
    }

    #[test]
    fn uncertain_attempts_are_immutable_and_protected_from_delete_all() {
        let mut store = posting_store();
        store.attachments.push(Attachment {
            id: "file-1".into(),
            draft_id: "crash-draft".into(),
            ..Attachment::default()
        });
        assert!(ensure_draft_mutable(&store, "crash-draft").is_err());
        let protected = uncertain_draft_ids(&store);
        assert!(protected.contains("crash-draft"));
        store
            .attachments
            .retain(|file| protected.contains(&file.draft_id));
        store
            .drafts
            .retain(|draft| requires_delivery_reconciliation(draft));
        assert_eq!(store.drafts.len(), 1);
        assert_eq!(store.attachments.len(), 1);
    }

    #[test]
    fn archive_start_never_queues_behind_an_existing_transaction() {
        let transaction = store_transaction();
        assert!(try_store_transaction().is_err());
        drop(transaction);
        assert!(try_store_transaction().is_ok());
    }

    #[test]
    fn new_message_ids_keep_provider_order_and_only_the_latest_twenty() {
        let mut ids = (0..25)
            .map(|index| format!("m-{index}"))
            .collect::<Vec<_>>();
        ids.push("m-24".into());
        normalize_new_message_ids(&mut ids);
        assert_eq!(ids.len(), 20);
        assert_eq!(ids.first().map(String::as_str), Some("m-5"));
        assert_eq!(ids.last().map(String::as_str), Some("m-24"));
    }

    #[test]
    fn concurrent_store_mutations_do_not_lose_independent_updates() {
        let path = Arc::new(temp_file("concurrent"));
        write_store_to(&path, &Store::default()).unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let handles: Vec<_> = (0..3)
            .map(|worker| {
                let path = Arc::clone(&path);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    mutate_store_at(&path, |store| {
                        match worker {
                            0 => store.drafts.push(Draft {
                                id: "draft-a".into(),
                                ..Draft::default()
                            }),
                            1 => store.attachments.push(Attachment {
                                id: "attachment-b".into(),
                                draft_id: "draft-a".into(),
                                ..Attachment::default()
                            }),
                            _ => store.settings.default_lookback_days = 77,
                        }
                        Ok(())
                    })
                    .unwrap();
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let stored = read_store_from(&path).unwrap();
        assert_eq!(stored.drafts.len(), 1);
        assert_eq!(stored.attachments.len(), 1);
        assert_eq!(stored.settings.default_lookback_days, 77);
        let _ = fs::remove_file(path.as_ref());
    }

    #[test]
    fn repeated_atomic_replacement_never_exposes_torn_json() {
        let path = temp_file("replace");
        write_store_to(&path, &Store::default()).unwrap();
        for index in 0..64 {
            mutate_store_at(&path, |store| {
                store.drafts.push(Draft {
                    id: format!("draft-{index}"),
                    body: "x".repeat(index * 17),
                    ..Draft::default()
                });
                Ok(())
            })
            .unwrap();
            let bytes = fs::read(&path).unwrap();
            let decoded: Store = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(decoded.drafts.len(), index + 1);
            assert!(!path.with_extension("json.tmp").exists());
        }
        let _ = fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn parent_sync_failure_blocks_provider_and_recovers_prepared_intent() {
        let path = temp_file("parent-sync-failure");
        let mut store = Store::default();
        let draft = Draft {
            id: "durability-draft".into(),
            remote_draft_id: Some("remote-durable".into()),
            subject: "Durability".into(),
            body: "Body".into(),
            ..Draft::default()
        };
        store.drafts.push(draft.clone());
        let result =
            persist_final_post_intent(&mut store, 0, draft, "remote-durable", |prepared| {
                let parent = path.parent().unwrap();
                fs::create_dir_all(parent).unwrap();
                let bytes = serde_json::to_vec_pretty(prepared).unwrap();
                let mut file = tempfile::NamedTempFile::new_in(parent).unwrap();
                file.write_all(&bytes).unwrap();
                file.as_file().sync_all().unwrap();
                let temp = file.into_temp_path();
                replace_path_durably_with_sync(temp.as_ref(), &path, |_| {
                    Err(std::io::Error::other("injected directory sync failure"))
                })
            });
        let mut provider_called = false;
        let submission = result.and_then(|_| {
            provider_called = true;
            Ok(())
        });
        assert!(submission.is_err());
        assert!(!provider_called);
        assert_eq!(read_store_from(&path).unwrap().drafts[0].state, "posting");
        let recovered = load_recovered_store_from(&path).unwrap();
        assert_eq!(recovered.drafts[0].state, "delivery_unknown");
        let _ = fs::remove_file(path);
    }

    #[cfg(windows)]
    #[test]
    fn windows_atomic_replace_executes_with_write_through() {
        assert_ne!(WINDOWS_DURABLE_REPLACE_FLAGS & 0x0000_0008, 0);
        assert_ne!(WINDOWS_DURABLE_REPLACE_FLAGS & 0x0000_0001, 0);
        let path = temp_file("windows-write-through");
        write_store_to(&path, &Store::default()).unwrap();
        assert_eq!(
            windows_file_attributes(&path).unwrap() & WINDOWS_FILE_ATTRIBUTE_TEMPORARY,
            0
        );
        let mut second = Store::default();
        second.settings.default_lookback_days = 77;
        write_store_to(&path, &second).unwrap();
        assert_eq!(
            windows_file_attributes(&path).unwrap() & WINDOWS_FILE_ATTRIBUTE_TEMPORARY,
            0
        );
        assert_eq!(
            read_store_from(&path)
                .unwrap()
                .settings
                .default_lookback_days,
            77
        );
        let _ = fs::remove_file(path);
    }

    fn posting_store() -> Store {
        let mut store = Store::default();
        let draft = Draft {
            id: "crash-draft".into(),
            remote_draft_id: Some("remote-42".into()),
            subject: "Crash boundary".into(),
            body: "Body".into(),
            ..Draft::default()
        };
        store.drafts.push(draft.clone());
        begin_final_post(&mut store, 0, draft, "remote-42");
        store
    }

    #[test]
    fn restart_after_final_post_intent_requires_reconciliation() {
        let path = temp_file("intent-crash");
        write_store_to(&path, &posting_store()).unwrap();
        let recovered = load_recovered_store_from(&path).unwrap();
        assert_eq!(recovered.drafts[0].state, "delivery_unknown");
        assert!(requires_delivery_reconciliation(&recovered.drafts[0]));
        assert!(recovered.drafts[0].final_post_attempt_id.is_some());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn status_read_does_not_recover_an_active_final_post() {
        let path = temp_file("active-final-post");
        write_store_to(&path, &posting_store()).unwrap();
        let transaction = store_transaction();
        let active = read_store_with_recovery_at(&path).unwrap();
        assert_eq!(active.drafts[0].state, "posting");
        drop(transaction);
        let restarted = read_store_with_recovery_at(&path).unwrap();
        assert_eq!(restarted.drafts[0].state, "delivery_unknown");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn provider_success_before_receipt_commit_still_requires_reconciliation() {
        let path = temp_file("accepted-crash");
        let persisted = posting_store();
        write_store_to(&path, &persisted).unwrap();
        let mut memory = persisted.clone();
        complete_final_post(&mut memory, 0, persisted.drafts[0].clone(), &json!({}));
        assert!(memory.drafts.is_empty());
        assert_eq!(memory.outbox[0].state, "posted");
        let recovered = load_recovered_store_from(&path).unwrap();
        assert_eq!(recovered.drafts[0].state, "delivery_unknown");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn receipt_persistence_failure_leaves_durable_attempt_reconcilable() {
        let path = temp_file("receipt-failure");
        let persisted = posting_store();
        write_store_to(&path, &persisted).unwrap();
        let mut memory = persisted.clone();
        complete_final_post(&mut memory, 0, persisted.drafts[0].clone(), &json!({}));

        let blocker = temp_file("blocked-parent");
        fs::write(&blocker, b"not a directory").unwrap();
        assert!(write_store_to(&blocker.join("store.json"), &memory).is_err());

        let recovered = load_recovered_store_from(&path).unwrap();
        assert_eq!(recovered.drafts[0].state, "delivery_unknown");
        assert!(requires_delivery_reconciliation(&recovered.drafts[0]));
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(blocker);
    }
}
