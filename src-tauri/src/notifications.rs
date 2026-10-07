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
fn windows_notification(
    title: &str, body: &str, muted: bool, action_label: &str, account_id: &str, chat: &str,
) -> windows::core::Result<windows::UI::Notifications::ToastNotification> {
    use windows::core::{h, Interface, HSTRING};
    use windows::Data::Xml::Dom::{XmlDocument, XmlElement};
    use windows::Foundation::{DateTime, IReference, PropertyValue};
    use windows::UI::Notifications::ToastNotification;

    let xml = XmlDocument::new()?;
    xml.LoadXml(h!("<toast duration=\"short\"><visual><binding template=\"ToastGeneric\"><text/><text/></binding></visual><actions><action arguments=\"open-chat\" activationType=\"foreground\"/></actions></toast>"))?;
    let texts = xml.GetElementsByTagName(h!("text"))?;
    texts.Item(0)?.SetInnerText(&HSTRING::from(title))?;
    texts.Item(1)?.SetInnerText(&HSTRING::from(body))?;
    let action: XmlElement = xml.GetElementsByTagName(h!("action"))?.Item(0)?.cast()?;
    action.SetAttribute(h!("content"), &HSTRING::from(action_label))?;
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
    title: &str, body: &str, muted: bool, action_label: &str, app_id: &str,
) -> windows::core::Result<(windows::UI::Notifications::ToastNotifier, windows::UI::Notifications::ToastNotification, u64)> {
    use windows::core::{Interface, HSTRING};
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::{ToastActivatedEventArgs, ToastNotificationManager};

    let toast = windows_notification(title, body, muted, action_label, &target.account_id, &target.chat)?;
    let generation = NEXT_WINDOWS_NOTIFICATION.fetch_add(1, Ordering::Relaxed);
    let key = (target.account_id.clone(), target.chat.clone());
    let callback_app = app.clone();
    let callback_target = target.clone();
    let callback_service = Arc::downgrade(service);
    let handler: TypedEventHandler<windows::UI::Notifications::ToastNotification, windows::core::IInspectable> = TypedEventHandler::new(move |_, args: windows::core::Ref<'_, windows::core::IInspectable>| {
        let action = args.as_ref().and_then(|args| args.cast::<ToastActivatedEventArgs>().ok())
            .and_then(|args| args.Arguments().ok()).map(|value| value.to_string());
        if !windows_opens_chat(action.as_deref()) { return Ok(()); }
        let key = (callback_target.account_id.clone(), callback_target.chat.clone());
        if active_windows_notifications().lock().unwrap().get(&key).copied() != Some(generation) { return Ok(()); }
        open_chat(&callback_app, &callback_target, &callback_service);
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

#[cfg(any(not(windows), test))]
fn opens_chat(response: &notify_rust::NotificationResponse) -> bool {
    matches!(response, notify_rust::NotificationResponse::Default)
        || matches!(response, notify_rust::NotificationResponse::Action(action) if action == "open-chat")
}

#[cfg(windows)]
fn windows_opens_chat(action: Option<&str>) -> bool {
    action.is_none_or(|action| action.is_empty() || action == "open-chat")
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
fn notification(title: &str, body: &str, muted: bool, action_label: &str, replacement_id: Option<u32>) -> notify_rust::Notification {
    let mut note = notify_rust::Notification::new();
    note.summary(title).body(body).auto_icon().action("open-chat", action_label)
        .timeout(notify_rust::Timeout::Milliseconds(NOTIFICATION_TIMEOUT_MS));
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(id) = replacement_id { note.id(id); }
    #[cfg(target_os = "macos")]
    let _ = replacement_id;
    #[cfg(all(unix, not(target_os = "macos")))]
    note.action("default", action_label);
    #[cfg(all(unix, not(target_os = "macos")))]
    if muted {
        note.hint(notify_rust::Hint::SuppressSound(true));
    }
    #[cfg(target_os = "macos")]
    let _ = muted;
    note
}

#[tauri::command]
pub(crate) async fn show_chat_notification(
    app: AppHandle, state: State<'_, AppState>, account_id: String, chat: String,
    title: String, body: String,
) -> CommandResult<()> {
    let service = state.service_for_account(&account_id)?;
    let muted = service.chat_sound_muted(&chat).await.map_err(CommandError::from)?.unwrap_or(false);
    let action_label = crate::native_locale::text(&app, "native.open_chat");
    #[cfg(not(windows))]
    let note = notification(&title, &body, muted, &action_label, None);
    #[cfg(windows)]
    let app_id = windows_app_id(&app)?;
    #[cfg(target_os = "macos")]
    let _ = notify_rust::set_application(if tauri::is_dev() { "com.apple.Terminal" } else { &app.config().identifier });
    current(&state, &account_id, &service)?;
    if !state.settings.lock().unwrap().notifications_enabled {
        return Ok(());
    }
    let service_ref = Arc::downgrade(&service);
    drop(service);
    let target = DesktopChatTarget { account_id, chat };
    let (shown, result) = tokio::sync::oneshot::channel();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(service) = service_ref.upgrade() else { let _ = shown.send(Err(CommandError::code("error.account_changed"))); return; };
        if let Err(error) = current(&app.state::<AppState>(), &target.account_id, &service) {
            let _ = shown.send(Err(error)); return;
        }
        if !app.state::<AppState>().settings.lock().unwrap().notifications_enabled {
            let _ = shown.send(Ok(())); return;
        }
        #[cfg(windows)]
        {
            let (notifier, toast, generation) = match show_windows_toast(&app, &service, &target, &title, &body, muted, &action_label, &app_id) {
                Ok(value) => value,
                Err(error) => { let _ = shown.send(Err(CommandError::operation_failed(error))); return; }
            };
            let _ = shown.send(Ok(()));
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
        let _ = shown.send(Ok(()));
        let service_ref = Arc::downgrade(&service);
        drop(service);
        if let Err(error) = handle.wait_for_response(|response: &notify_rust::NotificationResponse| {
            if !opens_chat(response) { return; }
            #[cfg(all(unix, not(target_os = "macos")))]
            if active_notifications().lock().unwrap().get(&key).map(|(_, current)| *current) != Some(generation) { return; }
            open_chat(&app, &target, &service_ref);
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

    #[cfg(not(windows))]
    #[test]
    fn sound_muting_keeps_visual_content_and_normal_defaults() {
        let normal = notification("Synthetic title", "Synthetic body", false, "Synthetic localized action", None);
        let muted = notification("Synthetic title", "Synthetic body", true, "Synthetic localized action", None);
        assert_eq!(normal.summary, "Synthetic title");
        assert_eq!(normal.body, "Synthetic body");
        assert_eq!(normal.summary, muted.summary);
        assert_eq!(normal.body, muted.body);
        assert_eq!(normal.timeout, notify_rust::Timeout::Milliseconds(NOTIFICATION_TIMEOUT_MS));
        assert!(format!("{normal:?}").contains("Synthetic localized action"));
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            assert!(!normal.hints.contains(&notify_rust::Hint::SuppressSound(true)));
            assert!(muted.hints.contains(&notify_rust::Hint::SuppressSound(true)));
        }
        #[cfg(any(windows, target_os = "macos"))]
        assert_eq!(format!("{normal:?}"), format!("{muted:?}"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_builder_tags_chat_and_sets_expiry() {
        let a = windows_notification("Alice & Bob", "A < B", false, "Open chat", "account", "chat").unwrap();
        let repeat = windows_notification("Updated", "Body", true, "Open chat", "account", "chat").unwrap();
        let other_chat = windows_notification("Other", "Body", false, "Open chat", "account", "other").unwrap();
        let other_account = windows_notification("Other", "Body", false, "Open chat", "other", "chat").unwrap();
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
        let muted_xml = repeat.Content().unwrap().GetXml().unwrap().to_string();
        assert!(muted_xml.contains("silent=\"true\""));
        assert!(windows_opens_chat(None));
        assert!(windows_opens_chat(Some("open-chat")));
        assert!(!windows_opens_chat(Some("unrelated")));
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
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&app_id).unwrap();
        let mut cleanup = Cleanup { notifier, app_id, tag: HSTRING::from(toast_key(&chat)),
            group: HSTRING::from(toast_key(&account)), toasts: Vec::new() };
        for number in 1..=3 {
            let toast = windows_notification("Postal notification smoke", &format!("Synthetic message {number}"),
                true, "Open chat", &account, &chat).unwrap();
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
        let note = notification("Title", "Body", false, "Open", Some(42));
        assert!(format!("{note:?}").contains("id: Some(42)"));
        assert_eq!(note.timeout, notify_rust::Timeout::Milliseconds(NOTIFICATION_TIMEOUT_MS));
    }

    #[test]
    fn desktop_notification_clicks_only_open_matching_actions() {
        use notify_rust::{CloseReason, NotificationResponse};
        assert!(opens_chat(&NotificationResponse::Default));
        assert!(opens_chat(&NotificationResponse::Action("open-chat".into())));
        assert!(!opens_chat(&NotificationResponse::Action("unrelated".into())));
        assert!(!opens_chat(&NotificationResponse::Closed(CloseReason::Dismissed)));
        assert!(!opens_chat(&NotificationResponse::Reply("open-chat".into())));
        let target = DesktopChatTarget { account_id: "synthetic-account".into(), chat: "synthetic-chat".into() };
        assert_eq!(serde_json::to_value(target).unwrap(), serde_json::json!({"account_id":"synthetic-account", "chat":"synthetic-chat"}));
    }
}
