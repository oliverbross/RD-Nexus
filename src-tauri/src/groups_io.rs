//! Native Groups.io integration.
//!
//! The API key is deliberately absent from [`Store`]. It lives in the platform credential
//! vault (`keyring`: Keychain / Credential Manager / Secret Service); this profile-scoped file
//! contains only preferences, offline data and delivery receipts. Every network operation is
//! operator-triggered foreground work. In particular, an ambiguous `postdraft` result is never
//! retried automatically.

use reqwest::blocking::{Client, Response};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
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
static STORE_LOCK: Mutex<()> = Mutex::new(());
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
    pub kind: String,
    pub subject: String,
    pub body: String,
    pub reply_destination: String,
    pub state: String,
    pub note: String,
    pub updated_unix: u64,
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

fn store_path() -> PathBuf {
    crate::config_dir().join("groupsio.json")
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
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    read_store_from(&store_path())
}

fn write_store_to(path: &Path, store: &Store) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("couldn't create the Groups.io data folder: {e}"))?;
    }
    let bytes = serde_json::to_vec_pretty(store)
        .map_err(|e| format!("couldn't encode Groups.io local data: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    let mut file =
        fs::File::create(&tmp).map_err(|e| format!("couldn't stage Groups.io local data: {e}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("couldn't write Groups.io local data: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("couldn't commit Groups.io local data: {e}"))
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
    let storage_bytes = fs::metadata(store_path()).map(|m| m.len()).unwrap_or(0);
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
    let bytes = response
        .bytes()
        .map_err(|_| "Groups.io returned an unreadable response")?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(json!({}));
    }
    serde_json::from_slice(&bytes).map_err(|_| "Groups.io returned an unreadable response".into())
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
    field(value, &["created", "date", "updated"])
        .and_then(Value::as_u64)
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
    let root = get(client, key, "/getsubs", &[("limit", "100".into())])?;
    let mut active = HashSet::new();
    for group in rows(&root).iter().filter_map(group_from) {
        active.insert(group.id.clone());
        upsert(&mut store.groups, group, |a, b| a.id == b.id);
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
            .take(policy.maximum_messages)
        {
            if !prior.is_empty() && !prior.contains(&message.id) {
                store.new_message_ids.push(message.id.clone());
            }
            upsert(&mut store.messages, message, |a, b| a.id == b.id);
        }
    }
    store.new_message_ids.sort();
    store.new_message_ids.dedup();
    if store.new_message_ids.len() > 20 {
        store
            .new_message_ids
            .drain(..store.new_message_ids.len() - 20);
    }
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
    let mut store = read_store()?;
    store.settings = validate_settings(settings);
    write_store(&store)?;
    Ok(status_for(store))
}

#[tauri::command(async)]
pub fn groups_io_connect(api_key_value: String) -> Result<Status, String> {
    let key = api_key_value.trim();
    if key.len() < 8 || key.len() > 4096 {
        return Err("Enter a valid Groups.io API key".into());
    }
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
    let _network = NETWORK_LOCK
        .try_lock()
        .map_err(|_| "another Groups.io operation is already running".to_string())?;
    let key = api_key()?;
    let client = client()?;
    let mut store = read_store()?;
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
    let items = rows(&root)
        .iter()
        .filter_map(|v| {
            let topic = text(v, &["topic_id"]);
            message_from(&group_id, &topic, v)
        })
        .collect();
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
    let mut store = read_store()?;
    upsert(&mut store.drafts, draft, |a, b| a.id == b.id);
    write_store(&store)?;
    Ok(status_for(store))
}

#[tauri::command(async)]
pub fn groups_io_delete_draft(id: String) -> Result<Status, String> {
    let mut store = read_store()?;
    store.drafts.retain(|d| d.id != id);
    write_store(&store)?;
    Ok(status_for(store))
}

#[tauri::command(async)]
pub fn groups_io_send_draft(id: String, confirmed: bool) -> Result<Status, String> {
    if !confirmed {
        return Err("Confirm the foreground send before posting to Groups.io".into());
    }
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
    if draft.subject.trim().is_empty() || draft.body.trim().is_empty() {
        return Err("A Groups.io subject and message are required".into());
    }
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
    if let Some(message_id) = draft.reply_api_message_id.clone().filter(|v| !v.is_empty()) {
        create.push(("message_id", message_id));
    }
    let created = post(&client, &key, "/newdraft", &create)?;
    let remote_id = text(&created, &["draft_id", "id"]);
    if remote_id.is_empty() {
        return Err("Groups.io did not return a draft id".into());
    }
    draft.remote_draft_id = Some(remote_id.clone());
    draft.state = "updating_remote".into();
    store.drafts[index] = draft.clone();
    write_store(&store)?;
    post(
        &client,
        &key,
        "/updatedraft",
        &[
            ("draft_id", remote_id.clone()),
            ("subject", draft.subject.clone()),
            ("body", draft.body.clone()),
            ("reply_to", draft.reply_destination.clone()),
        ],
    )?;
    draft.state = "posting".into();
    store.drafts[index] = draft.clone();
    write_store(&store)?;
    match post(&client, &key, "/postdraft", &[("draft_id", remote_id)]) {
        Ok(value) => {
            draft.state = if text(&value, &["extra"]).to_lowercase().contains("pending") {
                "pending_moderation".into()
            } else {
                "posted".into()
            };
            draft.note = "Submitted explicitly from RD-Nexus.".into();
            draft.updated_unix = now();
            store.drafts.remove(index);
            store.outbox.push(draft);
        }
        Err(error) => {
            draft.state = "delivery_unknown".into();
            draft.note = format!("Delivery must be reconciled before retry: {error}");
            draft.updated_unix = now();
            store.drafts[index] = draft;
            write_store(&store)?;
            return Err("Groups.io delivery is unknown; inspect recent messages and server drafts before retrying".into());
        }
    }
    write_store(&store)?;
    Ok(status_for(store))
}

#[tauri::command(async)]
pub fn groups_io_archive(group_id: String, range_days: Option<u32>) -> Result<Status, String> {
    let _network = NETWORK_LOCK
        .try_lock()
        .map_err(|_| "another Groups.io operation is already running".to_string())?;
    ARCHIVE_CANCEL.store(false, Ordering::Release);
    let key = api_key()?;
    let client = client()?;
    let mut store = read_store()?;
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
            ("sort_dir", "asc".into()),
        ];
        if let Some(value) = token.clone() {
            params.push(("page_token", value));
        }
        let root = get(&client, &key, "/getmessages", &params)?;
        let values = rows(&root);
        for message in values.iter().filter_map(|v| {
            let topic = text(v, &["topic_id"]);
            message_from(&group_id, &topic, v)
        }) {
            upsert(&mut store.messages, message, |a, b| a.id == b.id);
            store.archive.downloaded += 1;
        }
        token = next_token(&root);
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
    ARCHIVE_CANCEL.store(true, Ordering::Release);
    let mut store = read_store()?;
    store.archive.state = "paused".into();
    store.archive.message = "Archive paused; completed pages were preserved.".into();
    write_store(&store)?;
    Ok(status_for(store))
}

#[tauri::command(async)]
pub fn groups_io_clear_cache(delete_all: bool) -> Result<Status, String> {
    let mut store = read_store()?;
    store.groups.clear();
    store.topics.clear();
    store.messages.clear();
    store.topic_tokens.clear();
    store.message_tokens.clear();
    store.archive = ArchiveState::default();
    store.new_message_ids.clear();
    if delete_all {
        store.drafts.clear();
        store.outbox.clear();
    }
    write_store(&store)?;
    Ok(status_for(store))
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let encoded = serde_json::to_string(&Store::default()).unwrap();
        assert!(GroupsIoSettings::default().refresh_on_open);
        assert!(!encoded.to_lowercase().contains("api_key"));
        assert!(!encoded.to_lowercase().contains("apikey"));
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
    }
}
