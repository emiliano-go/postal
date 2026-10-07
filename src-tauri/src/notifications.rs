use crate::AppState;
use crate::command_error::{CommandError, CommandResult};
use postal_core::WhatsAppService;
use std::sync::Arc;
#[cfg(any(windows, all(unix, not(target_os = "macos"))))]
use std::sync::{Mutex, OnceLock};
#[cfg(any(windows, all(unix, not(target_os = "macos"))))]
use std::{collections::HashMap, sync::atomic::{AtomicU64, Ordering}};
use tauri::{AppHandle, Emitter, Manager, State};

const NOTIFICATION_TIMEOUT_MS: u32 = 10_000;
#[cfg(windows)]
const DEV_TOAST_APP_ID: &str = "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

#[cfg(all(unix, not(target_os = "macos")))]
type NotificationKey = (String, String);
#[cfg(all(unix, not(target_os = "macos")))]
static ACTIVE_NOTIFICATIONS: OnceLock<Mutex<HashMap<NotificationKey, (u32, u64)>>> = OnceLock::new();
#[cfg(all(unix, not(target_os = "macos")))]
static NEXT_NOTIFICATION: AtomicU64 = AtomicU64::new(1);

#[cfg(all(unix, not(target_os = "macos")))]
fn active_notifications() -> &'static Mutex<HashMap<NotificationKey, (u32, u64)>> {
    ACTIVE_NOTIFICATIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn show_xdg_notification(
    mut note: notify_rust::Notification, key: &NotificationKey,
) -> CommandResult<(notify_rust::NotificationHandle, u64)> {
    let mut active = active_notifications().lock().unwrap();
    if let Some((id, _)) = active.get(key) { note.id(*id); }
    let handle = note.show().map_err(CommandError::operation_failed)?;
    let generation = NEXT_NOTIFICATION.fetch_add(1, Ordering::Relaxed);
    active.insert(key.clone(), (handle.id(), generation));
    Ok((handle, generation))
}

#[cfg(windows)]
static ACTIVE_WINDOWS_NOTIFICATIONS: OnceLock<Mutex<HashMap<(String, String), u64>>> = OnceLock::new();
#[cfg(windows)]
static NEXT_WINDOWS_NOTIFICATION: AtomicU64 = AtomicU64::new(1);

#[cfg(windows)]
fn active_windows_notifications() -> &'static Mutex<HashMap<(String, String), u64>> {
    ACTIVE_WINDOWS_NOTIFICATIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(windows)]
fn toast_key(value: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hash);
    format!("{:016x}", hash.finish())
}

#[cfg(windows)]
fn windows_app_id(app: &AppHandle) -> CommandResult<String> {
    let exe = tauri::utils::platform::current_exe().map_err(CommandError::operation_failed)?;
    let directory = exe.parent().ok_or_else(|| CommandError::code("error.notification_executable_directory_unavailable"))?;
    let directory = directory.display().to_string();
    let sep = std::path::MAIN_SEPARATOR;
    if directory.ends_with(format!("{sep}target{sep}debug").as_str())
        || directory.ends_with(format!("{sep}target{sep}release").as_str()) {
        Ok(DEV_TOAST_APP_ID.to_owned())
    } else {
        Ok(app.config().identifier.clone())
    }
}

#[cfg(windows)]
fn append_toast_action(
    xml: &windows::Data::Xml::Dom::XmlDocument, actions: &windows::Data::Xml::Dom::IXmlNode,
    id: &str, label: &str,
) -> windows::core::Result<()> {
    use windows::core::{h, HSTRING};
    let action = xml.CreateElement(h!("action"))?;
    action.SetAttribute(h!("arguments"), &HSTRING::from(id))?;
    action.SetAttribute(h!("content"), &HSTRING::from(label))?;
    action.SetAttribute(h!("activationType"), h!("foreground"))?;
    actions.AppendChild(&action)?;
    Ok(())
}

#[cfg(windows)]
fn windows_notification(
    title: &str, body: &str, muted: bool, labels: &ActionLabels, account_id: &str, chat: &str,
) -> windows::core::Result<windows::UI::Notifications::ToastNotification> {
    use windows::core::{h, Interface, HSTRING};
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::{DateTime, IReference, PropertyValue};
    use windows::UI::Notifications::ToastNotification;

    let xml = XmlDocument::new()?;
    xml.LoadXml(h!("<toast duration=\"short\"><visual><binding template=\"ToastGeneric\"><text/><text/></binding></visual><actions/></toast>"))?;
    let texts = xml.GetElementsByTagName(h!("text"))?;
    texts.Item(0)?.SetInnerText(&HSTRING::from(title))?;
    texts.Item(1)?.SetInnerText(&HSTRING::from(body))?;
    let actions = xml.GetElementsByTagName(h!("actions"))?.Item(0)?;
    append_toast_action(&xml, &actions, "open-chat", &labels.open)?;
    if direct_chat(chat) { append_toast_action(&xml, &actions, "mark-read", &labels.mark_read)?; }
    append_toast_action(&xml, &actions, "mute-chat", &labels.mute)?;
    if muted {
        let audio = xml.CreateElement(h!("audio"))?;
        audio.SetAttribute(h!("silent"), h!("true"))?;
        xml.DocumentElement()?.AppendChild(&audio)?;
    }
    let toast = ToastNotification::CreateToastNotification(&xml)?;
    toast.SetGroup(&HSTRING::from(toast_key(account_id)))?;
    toast.SetTag(&HSTRING::from(toast_key(chat)))?;
    let deadline = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64
        + NOTIFICATION_TIMEOUT_MS as i64;
    let expiry = PropertyValue::CreateDateTime(DateTime { UniversalTime: deadline * 10_000 + 116_444_736_000_000_000 })?
        .cast::<IReference<DateTime>>()?;
    toast.SetExpirationTime(&expiry)?;
    Ok(toast)
}

#[cfg(windows)]
fn show_windows_toast(
    app: &AppHandle, service: &Arc<WhatsAppService>, target: &DesktopChatTarget,
    title: &str, body: &str, muted: bool, labels: &ActionLabels, app_id: &str,
) -> windows::core::Result<(windows::UI::Notifications::ToastNotifier, windows::UI::Notifications::ToastNotification, u64)> {
    use windows::core::{Interface, HSTRING};
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::{ToastActivatedEventArgs, ToastNotificationManager};

    let toast = windows_notification(title, body, muted, labels, &target.account_id, &target.chat)?;
    let generation = NEXT_WINDOWS_NOTIFICATION.fetch_add(1, Ordering::Relaxed);
    let key = (target.account_id.clone(), target.chat.clone());
    let callback_app = app.clone();
    let callback_target = target.clone();
    let callback_service = Arc::downgrade(service);
    let handler: TypedEventHandler<windows::UI::Notifications::ToastNotification, windows::core::IInspectable> = TypedEventHandler::new(move |_, args: windows::core::Ref<'_, windows::core::IInspectable>| {
        let action = args.as_ref().and_then(|args| args.cast::<ToastActivatedEventArgs>().ok())
            .and_then(|args| args.Arguments().ok()).map(|value| value.to_string());
        let Some(action) = action_for_id(action.as_deref(), &callback_target.chat) else { return Ok(()); };
        let key = (callback_target.account_id.clone(), callback_target.chat.clone());
        if active_windows_notifications().lock().unwrap().get(&key).copied() != Some(generation) { return Ok(()); }
        dispatch_action(&callback_app, &callback_target, &callback_service, action);
        Ok(())
    });
    toast.Activated(&handler)?;
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))?;
    let mut active = active_windows_notifications().lock().unwrap();
    notifier.Show(&toast)?;
    active.insert(key, generation);
    Ok((notifier, toast, generation))
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct DesktopChatTarget {
    pub account_id: String,
    pub chat: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NotificationAction { Open, MarkRead, Mute }

struct ActionLabels { open: String, mark_read: String, mute: String }

fn direct_chat(chat: &str) -> bool {
    let Some((user, server)) = chat.split_once('@') else { return false; };
    !user.is_empty() && user.bytes().all(|byte| byte.is_ascii_digit())
        && matches!(server, "s.whatsapp.net" | "lid")
}

fn action_for_id(id: Option<&str>, chat: &str) -> Option<NotificationAction> {
    match id.unwrap_or("") {
        "" | "default" | "open-chat" => Some(NotificationAction::Open),
        "mark-read" if direct_chat(chat) => Some(NotificationAction::MarkRead),
        "mute-chat" => Some(NotificationAction::Mute),
        _ => None,
    }
}

#[cfg(any(not(windows), test))]
fn action_for_response(response: &notify_rust::NotificationResponse, chat: &str) -> Option<NotificationAction> {
    match response {
        notify_rust::NotificationResponse::Default => action_for_id(None, chat),
        notify_rust::NotificationResponse::Action(id) => action_for_id(Some(id), chat),
        _ => None,
    }
}

fn current(state: &AppState, account_id: &str, service: &Arc<WhatsAppService>) -> CommandResult<()> {
    let active = state.service_for_account(account_id)
        .map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if !Arc::ptr_eq(service, &active) {
        return Err(CommandError::code("error.account_changed"));
    }
    Ok(())
}

fn open_chat(app: &AppHandle, target: &DesktopChatTarget, service_ref: &std::sync::Weak<WhatsAppService>) {
    let Some(service) = service_ref.upgrade() else { return; };
    if current(&app.state::<AppState>(), &target.account_id, &service).is_err() { return; }
    crate::tray::show_main(app);
    if let Err(error) = app.emit_to("main", "desktop-open-chat", target) {
        log::warn!("could not open notification chat: {error}");
    }
}

async fn execute_notification_action<M, MF, U, UF>(
    action: NotificationAction, chat: &str, receipts: bool, mark_read: M, mute: U,
) -> anyhow::Result<()>
where
    M: FnOnce(bool) -> MF, MF: std::future::Future<Output = anyhow::Result<usize>>,
    U: FnOnce() -> UF, UF: std::future::Future<Output = anyhow::Result<()>>,
{
    match action {
        NotificationAction::MarkRead if direct_chat(chat) => { mark_read(receipts).await?; Ok(()) }
        NotificationAction::Mute => mute().await,
        _ => Ok(()),
    }
}

fn dispatch_action(
    app: &AppHandle, target: &DesktopChatTarget, service_ref: &std::sync::Weak<WhatsAppService>,
    action: NotificationAction,
) {
    if action == NotificationAction::Open { open_chat(app, target, service_ref); return; }
    if action == NotificationAction::MarkRead && !direct_chat(&target.chat) { return; }
    let Some(service) = service_ref.upgrade() else { return; };
    if current(&app.state::<AppState>(), &target.account_id, &service).is_err() { return; }
    let app = app.clone();
    let target = target.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if current(&state, &target.account_id, &service).is_err() { return; }
        let receipts = if action == NotificationAction::MarkRead {
            let receipts = crate::settings::sends_privacy(&state, &service, &target.chat).await.1;
            if current(&state, &target.account_id, &service).is_err() { return; }
            receipts
        } else { false };
        let result = execute_notification_action(action, &target.chat, receipts,
            |send| service.mark_read(&target.chat, send), || service.set_muted(&target.chat, -1)).await;
        if let Err(error) = result { log::warn!("notification action failed for {}: {error}", target.chat); }
    });
}

#[tauri::command]
pub(crate) async fn chat_sound_muted(
    state: State<'_, AppState>, account_id: String, chat: String,
) -> CommandResult<Option<bool>> {
    let service = state.service_for_account(&account_id)?;
    let muted = service.chat_sound_muted(&chat).await.map_err(CommandError::from)?;
    current(&state, &account_id, &service)?;
    Ok(muted)
}

#[tauri::command]
pub(crate) async fn set_chat_sound_muted(
    state: State<'_, AppState>, account_id: String, chat: String, muted: Option<bool>,
) -> CommandResult<()> {
    let service = state.service_for_account(&account_id)?;
    service.set_chat_sound_muted(&chat, muted).await.map_err(CommandError::from)?;
    current(&state, &account_id, &service)
}

#[cfg(not(windows))]
fn notification(title: &str, body: &str, muted: bool, labels: &ActionLabels, chat: &str, replacement_id: Option<u32>) -> notify_rust::Notification {
    let mut note = notify_rust::Notification::new();
    note.summary(title).body(body).auto_icon().action("open-chat", &labels.open)
        .timeout(notify_rust::Timeout::Milliseconds(NOTIFICATION_TIMEOUT_MS));
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(id) = replacement_id { note.id(id); }
    #[cfg(target_os = "macos")]
    let _ = replacement_id;
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        note.action("default", &labels.open);
        if direct_chat(chat) { note.action("mark-read", &labels.mark_read); }
        note.action("mute-chat", &labels.mute);
    }
    #[cfg(target_os = "macos")]
    let _ = chat;
    #[cfg(all(unix, not(target_os = "macos")))]
    if muted {
        note.hint(notify_rust::Hint::SuppressSound(true));
    }
    #[cfg(target_os = "macos")]
    let _ = muted;
    note
}

fn native_audio_plan(
    chat_muted: bool, preset: crate::settings::NotificationSound,
) -> (bool, Option<crate::settings::NotificationSound>) {
    let custom = preset != crate::settings::NotificationSound::System;
    (chat_muted || custom, (!chat_muted && custom).then_some(preset))
}

fn mute_active(until: i64, now: i64) -> bool { until < 0 || until > now }

fn chat_notifications_muted(service: &WhatsAppService, chat: &str) -> CommandResult<bool> {
    let until = tauri::async_runtime::block_on(service.muted_until(chat)).map_err(CommandError::from)?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    Ok(mute_active(until, now))
}

async fn native_audio_selection(
    state: &AppState, service: &WhatsAppService, account_id: &str, chat: &str,
) -> CommandResult<(bool, Option<crate::settings::NotificationSound>)> {
    let chat_muted = service.chat_sound_muted(chat).await.map_err(CommandError::from)?.unwrap_or(false);
    Ok({
        let settings = state.settings.lock().unwrap();
        let preset = *crate::settings::notification_sound_for(&settings, account_id, chat);
        native_audio_plan(chat_muted, preset)
    })
}

async fn notification_presentation(
    app: &AppHandle, state: &AppState, service: &WhatsAppService, account_id: &str, chat: &str,
) -> CommandResult<(bool, Option<crate::settings::NotificationSound>, ActionLabels)> {
    let (muted, custom_sound) = native_audio_selection(state, service, account_id, chat).await?;
    let labels = ActionLabels {
        open: crate::native_locale::text(app, "native.open_chat"),
        mark_read: crate::native_locale::text(app, "chat.mark_read"),
        mute: crate::native_locale::text(app, "chat.mute"),
    };
    Ok((muted, custom_sound, labels))
}

fn playback_sound(
    app: &AppHandle, service: &Arc<WhatsAppService>, target: &DesktopChatTarget,
    expected: Option<crate::settings::NotificationSound>,
) -> Option<crate::settings::NotificationSound> {
    let expected = expected?;
    let state = app.state::<AppState>();
    if current(&state, &target.account_id, service).is_err() || !state.settings.lock().unwrap().notifications_enabled { return None; }
    if chat_notifications_muted(service, &target.chat).ok()? { return None; }
    let sound = tauri::async_runtime::block_on(native_audio_selection(&state, service, &target.account_id, &target.chat))
        .map(|(_, sound)| sound).unwrap_or(None);
    if current(&state, &target.account_id, service).is_err() || !state.settings.lock().unwrap().notifications_enabled { return None; }
    (sound == Some(expected)).then_some(expected)
}

fn prepare_notification(
    app: &AppHandle, service: &Arc<WhatsAppService>, target: &DesktopChatTarget,
) -> CommandResult<Option<(bool, Option<crate::settings::NotificationSound>, ActionLabels)>> {
    let state = app.state::<AppState>();
    current(&state, &target.account_id, service)?;
    if !state.settings.lock().unwrap().notifications_enabled { return Ok(None); }
    if chat_notifications_muted(service, &target.chat)? { return Ok(None); }
    let presentation = tauri::async_runtime::block_on(notification_presentation(
        app, &state, service, &target.account_id, &target.chat,
    ))?;
    current(&state, &target.account_id, service)?;
    if !state.settings.lock().unwrap().notifications_enabled { return Ok(None); }
    Ok(Some(presentation))
}

#[tauri::command]
pub(crate) async fn show_chat_notification(
    app: AppHandle, state: State<'_, AppState>, account_id: String, chat: String,
    title: String, body: String,
) -> CommandResult<Option<crate::settings::NotificationSound>> {
    let service = state.service_for_account(&account_id)?;
    #[cfg(windows)]
    let app_id = windows_app_id(&app)?;
    #[cfg(target_os = "macos")]
    let _ = notify_rust::set_application(if tauri::is_dev() { "com.apple.Terminal" } else { &app.config().identifier });
    current(&state, &account_id, &service)?;
    if !state.settings.lock().unwrap().notifications_enabled {
        return Ok(None);
    }
    let service_ref = Arc::downgrade(&service);
    drop(service);
    let target = DesktopChatTarget { account_id, chat };
    let (shown, result) = tokio::sync::oneshot::channel();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(service) = service_ref.upgrade() else { let _ = shown.send(Err(CommandError::code("error.account_changed"))); return; };
        let (muted, custom_sound, labels) = match prepare_notification(&app, &service, &target) {
            Ok(Some(value)) => value,
            Ok(None) => { let _ = shown.send(Ok(None)); return; }
            Err(error) => { let _ = shown.send(Err(error)); return; }
        };
        #[cfg(not(windows))]
        let note = notification(&title, &body, muted, &labels, &target.chat, None);
        #[cfg(windows)]
        {
            let (notifier, toast, generation) = match show_windows_toast(&app, &service, &target, &title, &body, muted, &labels, &app_id) {
                Ok(value) => value,
                Err(error) => { let _ = shown.send(Err(CommandError::operation_failed(error))); return; }
            };
            let _ = shown.send(Ok(playback_sound(&app, &service, &target, custom_sound)));
            drop(service);
            std::thread::sleep(std::time::Duration::from_millis(NOTIFICATION_TIMEOUT_MS as u64));
            let key = (target.account_id, target.chat);
            let mut active = active_windows_notifications().lock().unwrap();
            if active.get(&key).copied() == Some(generation) {
                if let Err(error) = notifier.Hide(&toast) { log::warn!("could not expire notification: {error}"); }
                active.remove(&key);
            }
            return;
        }
        #[cfg(not(windows))]
        {
        #[cfg(all(unix, not(target_os = "macos")))]
        let key = (target.account_id.clone(), target.chat.clone());
        #[cfg(all(unix, not(target_os = "macos")))]
        let (handle, generation) = match show_xdg_notification(note, &key) {
            Ok(value) => value,
            Err(error) => { let _ = shown.send(Err(error)); return; }
        };
        #[cfg(target_os = "macos")]
        let handle = match note.show() {
            Ok(handle) => handle,
            Err(error) => { let _ = shown.send(Err(CommandError::operation_failed(error))); return; }
        };
        let _ = shown.send(Ok(playback_sound(&app, &service, &target, custom_sound)));
        let service_ref = Arc::downgrade(&service);
        drop(service);
        if let Err(error) = handle.wait_for_response(|response: &notify_rust::NotificationResponse| {
            let Some(action) = action_for_response(response, &target.chat) else { return; };
            #[cfg(all(unix, not(target_os = "macos")))]
            if active_notifications().lock().unwrap().get(&key).map(|(_, current)| *current) != Some(generation) { return; }
            dispatch_action(&app, &target, &service_ref, action);
        }) {
            log::warn!("could not listen for notification action: {error}");
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let mut active = active_notifications().lock().unwrap();
            if active.get(&key).map(|(_, current)| *current) == Some(generation) { active.remove(&key); }
        }
        }
    });
    result.await.map_err(|_| CommandError::code("error.notification_worker_stopped"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_mute_deadlines_suppress_pending_notifications() {
        assert!(mute_active(-1, 100));
        assert!(mute_active(101, 100));
        assert!(!mute_active(100, 100));
        assert!(!mute_active(0, 100));
    }

    fn labels() -> ActionLabels {
        ActionLabels { open: "Open chat".into(), mark_read: "Mark read".into(), mute: "Mute chat".into() }
    }

    #[cfg(not(windows))]
    #[test]
    fn sound_muting_keeps_visual_content_and_normal_defaults() {
        let labels = labels();
        let normal = notification("Synthetic title", "Synthetic body", false, &labels, "123@s.whatsapp.net", None);
        let muted = notification("Synthetic title", "Synthetic body", true, &labels, "123@s.whatsapp.net", None);
        assert_eq!(normal.summary, "Synthetic title");
        assert_eq!(normal.body, "Synthetic body");
        assert_eq!(normal.summary, muted.summary);
        assert_eq!(normal.body, muted.body);
        assert_eq!(normal.timeout, notify_rust::Timeout::Milliseconds(NOTIFICATION_TIMEOUT_MS));
        assert!(format!("{normal:?}").contains("Open chat"));
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            assert!(normal.actions.contains(&"mark-read".to_owned()));
            assert!(normal.actions.contains(&"mute-chat".to_owned()));
            let group = notification("Group", "Body", false, &labels, "123@g.us", None);
            assert!(!group.actions.contains(&"mark-read".to_owned()));
            assert!(group.actions.contains(&"mute-chat".to_owned()));
            assert!(!normal.hints.contains(&notify_rust::Hint::SuppressSound(true)));
            assert!(muted.hints.contains(&notify_rust::Hint::SuppressSound(true)));
        }
        #[cfg(any(windows, target_os = "macos"))]
        assert_eq!(format!("{normal:?}"), format!("{muted:?}"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_builder_tags_chat_and_sets_expiry() {
        let labels = labels();
        let a = windows_notification("Alice & Bob", "A < B", false, &labels, "account", "123@s.whatsapp.net").unwrap();
        let repeat = windows_notification("Updated", "Body", true, &labels, "account", "123@s.whatsapp.net").unwrap();
        let other_chat = windows_notification("Other", "Body", false, &labels, "account", "456@s.whatsapp.net").unwrap();
        let other_account = windows_notification("Other", "Body", false, &labels, "other", "123@s.whatsapp.net").unwrap();
        assert_eq!(a.Tag().unwrap(), repeat.Tag().unwrap());
        assert_eq!(a.Group().unwrap(), repeat.Group().unwrap());
        assert_ne!(a.Tag().unwrap(), other_chat.Tag().unwrap());
        assert_ne!(a.Group().unwrap(), other_account.Group().unwrap());
        assert_eq!(a.Tag().unwrap().len(), 16);
        let expiry = a.ExpirationTime().unwrap().Value().unwrap().UniversalTime;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
        let remaining_ms = (expiry - 116_444_736_000_000_000) / 10_000 - now;
        assert!(remaining_ms > 0 && remaining_ms <= NOTIFICATION_TIMEOUT_MS as i64 + 1000);
        let xml = a.Content().unwrap().GetXml().unwrap().to_string();
        assert!(xml.contains("Alice &amp; Bob"));
        assert!(xml.contains("A &lt; B"));
        assert!(xml.contains("open-chat"));
        assert!(xml.contains("mark-read"));
        assert!(xml.contains("mute-chat"));
        let group = windows_notification("Group", "Body", false, &labels, "account", "123@g.us").unwrap();
        let group_xml = group.Content().unwrap().GetXml().unwrap().to_string();
        assert!(!group_xml.contains("mark-read"));
        assert!(group_xml.contains("mute-chat"));
        let muted_xml = repeat.Content().unwrap().GetXml().unwrap().to_string();
        assert!(muted_xml.contains("silent=\"true\""));
        assert_eq!(action_for_id(None, "123@s.whatsapp.net"), Some(NotificationAction::Open));
        assert_eq!(action_for_id(Some("mark-read"), "123@s.whatsapp.net"), Some(NotificationAction::MarkRead));
        assert_eq!(action_for_id(Some("mark-read"), "123@g.us"), None);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "shows synthetic Windows notifications for the expiry interval"]
    fn windows_shell_replaces_and_expires_one_chat() {
        use windows::core::HSTRING;
        use windows::UI::Notifications::{ToastNotification, ToastNotificationManager, ToastNotifier};

        struct Cleanup {
            notifier: ToastNotifier, app_id: HSTRING, tag: HSTRING, group: HSTRING,
            toasts: Vec<ToastNotification>,
        }
        impl Drop for Cleanup {
            fn drop(&mut self) {
                for toast in &self.toasts { let _ = self.notifier.Hide(toast); }
                if let Ok(history) = ToastNotificationManager::History() {
                    let _ = history.RemoveGroupedTagWithId(&self.tag, &self.group, &self.app_id);
                }
            }
        }
        fn matching(app_id: &HSTRING, tag: &HSTRING, group: &HSTRING) -> windows::core::Result<Vec<ToastNotification>> {
            let history = ToastNotificationManager::History()?.GetHistoryWithId(app_id)?;
            let mut matches = Vec::new();
            for index in 0..history.Size()? {
                let toast = history.GetAt(index)?;
                if toast.Tag()? == *tag && toast.Group()? == *group { matches.push(toast); }
            }
            Ok(matches)
        }

        let unique = format!("hermodr-smoke-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let account = format!("{unique}-account");
        let chat = format!("{unique}-chat");
        let app_id = HSTRING::from(DEV_TOAST_APP_ID);
        let labels = labels();
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&app_id).unwrap();
        let mut cleanup = Cleanup { notifier, app_id, tag: HSTRING::from(toast_key(&chat)),
            group: HSTRING::from(toast_key(&account)), toasts: Vec::new() };
        for number in 1..=3 {
            let toast = windows_notification("Postal notification smoke", &format!("Synthetic message {number}"),
                true, &labels, &account, &chat).unwrap();
            cleanup.notifier.Show(&toast).unwrap();
            cleanup.toasts.push(toast);
        }
        let mut current = Vec::new();
        let mut xml = String::new();
        for _ in 0..20 {
            current = matching(&cleanup.app_id, &cleanup.tag, &cleanup.group).unwrap();
            if current.len() == 1 {
                xml = current[0].Content().unwrap().GetXml().unwrap().to_string();
                if xml.contains("Synthetic message 3") { break; }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert_eq!(current.len(), 1, "Windows did not coalesce the synthetic chat");
        assert!(xml.contains("Synthetic message 3"), "Windows kept an older toast: {xml}");
        std::thread::sleep(std::time::Duration::from_millis(NOTIFICATION_TIMEOUT_MS as u64 + 500));
        assert!(matching(&cleanup.app_id, &cleanup.tag, &cleanup.group).unwrap().is_empty(),
            "Windows kept the synthetic toast after its timeout");
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn replacement_id_is_set_for_xdg() {
        let note = notification("Title", "Body", false, &labels(), "123@s.whatsapp.net", Some(42));
        assert!(format!("{note:?}").contains("id: Some(42)"));
        assert_eq!(note.timeout, notify_rust::Timeout::Milliseconds(NOTIFICATION_TIMEOUT_MS));
    }

    #[test]
    fn desktop_notification_clicks_only_open_matching_actions() {
        use notify_rust::{CloseReason, NotificationResponse};
        assert_eq!(action_for_response(&NotificationResponse::Default, "123@s.whatsapp.net"), Some(NotificationAction::Open));
        assert_eq!(action_for_response(&NotificationResponse::Action("open-chat".into()), "123@s.whatsapp.net"), Some(NotificationAction::Open));
        assert_eq!(action_for_response(&NotificationResponse::Action("mark-read".into()), "123@s.whatsapp.net"), Some(NotificationAction::MarkRead));
        assert_eq!(action_for_response(&NotificationResponse::Action("mark-read".into()), "123@g.us"), None);
        assert_eq!(action_for_response(&NotificationResponse::Action("mute-chat".into()), "123@g.us"), Some(NotificationAction::Mute));
        assert_eq!(action_for_response(&NotificationResponse::Action("unrelated".into()), "123@s.whatsapp.net"), None);
        assert_eq!(action_for_response(&NotificationResponse::Closed(CloseReason::Dismissed), "123@s.whatsapp.net"), None);
        assert_eq!(action_for_response(&NotificationResponse::Reply("open-chat".into()), "123@s.whatsapp.net"), None);
        let target = DesktopChatTarget { account_id: "synthetic-account".into(), chat: "synthetic-chat".into() };
        assert_eq!(serde_json::to_value(target).unwrap(), serde_json::json!({"account_id":"synthetic-account", "chat":"synthetic-chat"}));
    }

    #[test]
    fn notification_actions_forward_privacy_and_skip_non_direct_read() {
        let calls = std::cell::RefCell::new(Vec::new());
        tauri::async_runtime::block_on(async {
            for (action, chat, receipts) in [
                (NotificationAction::MarkRead, "123@s.whatsapp.net", false),
                (NotificationAction::MarkRead, "456@lid", true),
                (NotificationAction::MarkRead, "123@g.us", true),
                (NotificationAction::Mute, "123@g.us", false),
            ] {
                execute_notification_action(action, chat, receipts,
                    |send| { calls.borrow_mut().push(format!("read:{send}")); async { Ok(1) } },
                    || { calls.borrow_mut().push("mute".to_owned()); async { Ok(()) } },
                ).await.unwrap();
            }
        });
        assert_eq!(*calls.borrow(), ["read:false", "read:true", "mute"]);
    }

    #[test]
    fn native_sound_respects_mute_and_system_preset() {
        use crate::settings::NotificationSound::{Chime, System};
        assert_eq!(native_audio_plan(false, System), (false, None));
        assert_eq!(native_audio_plan(true, System), (true, None));
        assert_eq!(native_audio_plan(false, Chime), (true, Some(Chime)));
        assert_eq!(native_audio_plan(true, Chime), (true, None));
    }
}
