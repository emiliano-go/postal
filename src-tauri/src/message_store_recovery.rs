use std::path::PathBuf;

use postal_core::store::recovery::{StoreIntegrity, StoreNotCorrupt, create_fresh_store,
    preserve_corrupt_store, probe_live_message_store, probe_message_store};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::{AppState, account_store::{active_account, config_for}, command_error::{CommandError, CommandResult}};

#[derive(Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub(crate) enum MessageStoreStatus { Disabled, Missing, Healthy, Corrupt }

#[derive(Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct MessageStoreHealth {
    status: MessageStoreStatus,
    path: Option<String>,
    diagnosis: Option<String>,
}

#[derive(Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub(crate) struct MessageStoreRecovery {
    preserved_directory: String,
    restart_diagnostic: Option<String>,
}

fn account_path(app: &AppHandle, state: &AppState, account: &str) -> CommandResult<Option<PathBuf>> {
    if active_account(state).as_deref() != Some(account) { return Err(CommandError::code("error.unknown_account")); }
    let settings = state.settings.lock().unwrap().clone();
    Ok(settings.keep_history.then(|| config_for(app, &settings, account).messages_path))
}

#[tauri::command]
pub(crate) async fn message_store_health(app: AppHandle, state: State<'_, AppState>, account: String) -> CommandResult<MessageStoreHealth> {
    let _transition = state.account_transition.lock().await;
    let Some(path) = account_path(&app, &state, &account)? else {
        return Ok(MessageStoreHealth { status: MessageStoreStatus::Disabled, path: None, diagnosis: None });
    };
    let key = app.state::<crate::database_encryption::DatabaseEncryption>().key(&account)
        .map_err(crate::database_encryption::command_failure)?;
    let live = state.service.lock().unwrap().is_some() || state.once_service.lock().unwrap().is_some();
    let shown = path.to_string_lossy().into_owned();
    let status = tauri::async_runtime::spawn_blocking(move || if live {
        probe_live_message_store(&path, key.as_ref())
    } else { probe_message_store(&path, key.as_ref()) }).await.map_err(CommandError::operation_failed)?
        .map_err(|error| CommandError::code("error.message_store_health_failed").with_diagnostic(error))?;
    let (status, diagnosis) = match status {
        StoreIntegrity::Missing => (MessageStoreStatus::Missing, None),
        StoreIntegrity::Healthy => (MessageStoreStatus::Healthy, None),
        StoreIntegrity::Corrupt(diagnosis) => (MessageStoreStatus::Corrupt, Some(diagnosis)),
    };
    Ok(MessageStoreHealth { status, path: Some(shown), diagnosis })
}

#[tauri::command]
pub(crate) async fn recover_message_store(app: AppHandle, state: State<'_, AppState>, account: String) -> CommandResult<MessageStoreRecovery> {
    let _transition = state.account_transition.lock().await;
    let path = account_path(&app, &state, &account)?
        .ok_or_else(|| CommandError::code("error.message_store_history_disabled"))?;
    if state.service.lock().unwrap().is_some() || state.once_service.lock().unwrap().is_some()
        || state.account_service.lock().unwrap().is_some()
        || state.once_service_ever_started.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(CommandError::code("error.message_store_in_use"));
    }
    let key = app.state::<crate::database_encryption::DatabaseEncryption>().key(&account)
        .map_err(crate::database_encryption::command_failure)?;
    let preserved = tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<PathBuf> {
        let preserved = preserve_corrupt_store(&path, key.as_ref())?;
        create_fresh_store(&path, key.as_ref())
            .map_err(|error| anyhow::anyhow!("fresh store could not open; preserved in {}: {error:#}", preserved.display()))?;
        Ok(preserved)
    }).await.map_err(CommandError::operation_failed)?
        .map_err(|error| if error.chain().any(|cause| cause.is::<StoreNotCorrupt>()) {
            CommandError::code("error.message_store_not_corrupt")
        } else { CommandError::code("error.message_store_recovery_failed").with_diagnostic(error) })?;
    let restart_diagnostic = crate::connection::start_service(&app, &state, &account).await.err().map(|error| error.to_string());
    Ok(MessageStoreRecovery { preserved_directory: preserved.to_string_lossy().into_owned(), restart_diagnostic })
}
