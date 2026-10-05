export type SettingSearchDefinition = {
  id: string;
  section: string;
  titleKey: string;
  descriptionKey?: string;
  descriptionParams?: Record<string, string | number | boolean | null>;
};

export type SettingSearchItem = SettingSearchDefinition & {
  title: string;
  description: string;
};

export function settingsSearchShortcut(event: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey" | "isComposing" | "defaultPrevented">): boolean {
  return event.key.toLowerCase() === "f" && (event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey && !event.isComposing && !event.defaultPrevented;
}

export const SETTING_SEARCH_FIELDS: readonly SettingSearchDefinition[] = [
  { id: "profile-photo", section: "profile", titleKey: "settings.main.change_photo", descriptionKey: "settings.main.profile_hint" },
  { id: "profile-name", section: "profile", titleKey: "settings.main.name" },
  { id: "profile-about", section: "profile", titleKey: "settings.main.profile_about" },
  { id: "whatsapp-typing", section: "whatsapp", titleKey: "settings.main.send_typing", descriptionKey: "settings.main.send_typing_hint" },
  { id: "whatsapp-receipts", section: "whatsapp", titleKey: "settings.main.send_receipts", descriptionKey: "settings.main.send_receipts_hint" },
  { id: "accounts-add", section: "accounts", titleKey: "settings.main.add_account", descriptionKey: "settings.main.accounts_hint" },
  { id: "privacy-encryption", section: "privacy", titleKey: "settings.encrypt_databases", descriptionKey: "settings.encrypt_databases_description" },
  { id: "privacy-ram", section: "privacy", titleKey: "settings.main.ram_messages", descriptionKey: "settings.main.ram_messages_hint", descriptionParams: { min: 50, max: 2000, count: 150 } },
  { id: "privacy-history", section: "privacy", titleKey: "settings.main.keep_history", descriptionKey: "settings.main.keep_history_hint" },
  { id: "privacy-history-folder", section: "privacy", titleKey: "settings.main.history_folder", descriptionKey: "settings.main.history_folder_hint" },
  { id: "privacy-retention-age", section: "privacy", titleKey: "settings.main.disk_duration", descriptionKey: "settings.main.disk_duration_hint" },
  { id: "privacy-retention-count", section: "privacy", titleKey: "settings.main.disk_messages", descriptionKey: "settings.main.disk_messages_hint" },
  { id: "privacy-full-history", section: "privacy", titleKey: "settings.main.request_full_history", descriptionKey: "settings.main.request_full_history_hint", descriptionParams: { count: 10000 } },
  { id: "privacy-backfill", section: "privacy", titleKey: "settings.main.download_history", descriptionKey: "settings.main.download_history_hint" },
  { id: "privacy-clear-history", section: "privacy", titleKey: "settings.main.clear_message_history", descriptionKey: "settings.main.clear_message_history_hint" },
  { id: "privacy-archive", section: "privacy", titleKey: "settings.archive_title", descriptionKey: "settings.archive_hint" },
  { id: "privacy-archive-scope", section: "privacy", titleKey: "settings.archive_scope" },
  { id: "privacy-archive-export", section: "privacy", titleKey: "settings.archive_export", descriptionKey: "settings.archive_hint" },
  { id: "privacy-archive-restore", section: "privacy", titleKey: "settings.archive_restore", descriptionKey: "settings.archive_private_hint" },
  { id: "chat-keyword-highlight", section: "chats", titleKey: "settings.keywords_highlight", descriptionKey: "settings.keywords_hint" },
  { id: "chat-keyword-hide", section: "chats", titleKey: "settings.keywords_hide", descriptionKey: "settings.keywords_hint" },
  { id: "chat-keep-archived", section: "chats", titleKey: "settings.main.keep_archived", descriptionKey: "settings.main.keep_archived_hint" },
  { id: "chat-freeze-hover", section: "chats", titleKey: "settings.main.freeze_hover", descriptionKey: "settings.main.freeze_hover_hint" },
  { id: "chat-preview", section: "chats", titleKey: "settings.main.chat_preview", descriptionKey: "settings.main.chat_preview_hint" },
  { id: "chat-preview-delay", section: "chats", titleKey: "settings.main.chat_preview_delay", descriptionKey: "settings.main.chat_preview_delay_hint", descriptionParams: { min: 100, max: 3000 } },
  { id: "spaces-export", section: "spaces", titleKey: "settings.main.export_metadata", descriptionKey: "settings.main.export_metadata_hint" },
  { id: "spaces-import", section: "spaces", titleKey: "settings.main.import_metadata", descriptionKey: "settings.main.import_metadata_hint" },
  { id: "notifications-enabled", section: "notifications", titleKey: "settings.main.enable_notifications", descriptionKey: "settings.main.enable_notifications_hint" },
  { id: "sync-center", section: "sync", titleKey: "sync.title", descriptionKey: "sync.scope" },
  { id: "notifications-mute-all", section: "notifications", titleKey: "settings.main.mute_all", descriptionKey: "settings.main.mute_all_hint" },
  { id: "notifications-system", section: "notifications", titleKey: "settings.main.system_permission", descriptionKey: "settings.main.notifications_hint" },
  { id: "notifications-history", section: "notifications", titleKey: "content.notification_history", descriptionKey: "content.recent_notification_events_from_this_account" },
  { id: "device-companion", section: "device", titleKey: "settings.main.pair_companion", descriptionKey: "settings.main.companion_pair_hint" },
  { id: "media-stickers", section: "media", titleKey: "content.resync_known_stickers", descriptionKey: "content.refreshes_known_shared_packs_favorites_and_recents" },
  { id: "media-quality", section: "media", titleKey: "settings.main.upload_quality", descriptionKey: "settings.main.upload_quality_hint", descriptionParams: { pixels: 1600, resolution: 480 } },
  { id: "media-preview-warning", section: "media", titleKey: "settings.main.video_preview_warning", descriptionKey: "settings.main.video_preview_warning_hint" },
  { id: "media-folder", section: "media", titleKey: "settings.main.download_folder", descriptionKey: "settings.main.download_folder_hint" },
  { id: "media-clear", section: "media", titleKey: "settings.main.clear_downloaded_media", descriptionKey: "settings.main.clear_downloaded_media_hint" },
  { id: "transcription-plugin", section: "transcription", titleKey: "settings.plugin", descriptionKey: "settings.transcription_label" },
  { id: "transcription-provider", section: "transcription", titleKey: "settings.provider" },
  { id: "transcription-trust", section: "transcription", titleKey: "settings.transcription_trust", descriptionKey: "settings.native_plugin_access" },
  { id: "transcription-enable", section: "transcription", titleKey: "settings.grant_enable", descriptionKey: "settings.plugin_transcribe_hint" },
  { id: "transcription-disable", section: "transcription", titleKey: "settings.transcription_disable" },
  { id: "transcription-cloud-consent", section: "transcription", titleKey: "settings.transcription_consent", descriptionKey: "settings.transcription_consent_auto" },
  { id: "transcription-api-key", section: "transcription", titleKey: "settings.api_key_configure", descriptionKey: "settings.transcription_cloud_hint" },
  { id: "transcription-api-key-remove", section: "transcription", titleKey: "settings.api_key_remove" },
  { id: "transcription-decoder", section: "transcription", titleKey: "settings.decoder_executable", descriptionKey: "settings.decoder_path" },
  { id: "transcription-whisper", section: "transcription", titleKey: "settings.whisper_executable", descriptionKey: "settings.whisper_path" },
  { id: "transcription-model-filename", section: "transcription", titleKey: "settings.model_filename" },
  { id: "transcription-model-hash", section: "transcription", titleKey: "settings.model_hash" },
  { id: "transcription-model-url", section: "transcription", titleKey: "settings.model_url", descriptionKey: "settings.model_download_hint" },
  { id: "transcription-model-download", section: "transcription", titleKey: "settings.model_download", descriptionKey: "settings.model_download_hint" },
  { id: "transcription-language", section: "transcription", titleKey: "settings.transcription_language", descriptionKey: "settings.transcription_language_hint" },
  { id: "transcription-idle-timeout", section: "transcription", titleKey: "settings.transcription_idle" },
  { id: "transcription-save", section: "transcription", titleKey: "settings.transcription_save" },
  { id: "transcription-auto", section: "transcription", titleKey: "settings.transcription_auto_downloaded", descriptionKey: "settings.transcription_auto_hint" },
  { id: "startup-login", section: "startup", titleKey: "settings.main.start_on_login", descriptionKey: "settings.main.start_on_login_hint" },
  { id: "startup-skip-loading", section: "startup", titleKey: "settings.main.skip_loading", descriptionKey: "settings.main.skip_loading_hint" },
  { id: "keybind-reset-all", section: "keybinds", titleKey: "settings.main.reset_keybinds", descriptionKey: "settings.main.reset_keybinds_hint" },
  { id: "appearance-language", section: "appearance", titleKey: "settings.language", descriptionKey: "settings.language_description" },
  { id: "appearance-theme", section: "appearance", titleKey: "settings.theme" },
  { id: "appearance-accent", section: "appearance", titleKey: "settings.accent_colour", descriptionKey: "settings.accent_hint" },
  { id: "appearance-background", section: "appearance", titleKey: "settings.background_picture", descriptionKey: "settings.background_hint" },
  { id: "appearance-picture-darken", section: "appearance", titleKey: "settings.picture_darken", descriptionKey: "settings.picture_darken_hint" },
  { id: "appearance-text-size", section: "appearance", titleKey: "settings.text_size", descriptionKey: "settings.text_size_hint" },
  { id: "appearance-density", section: "appearance", titleKey: "settings.density", descriptionKey: "settings.density_hint" },
  { id: "appearance-chat-width", section: "appearance", titleKey: "settings.chat_list_width", descriptionKey: "settings.chat_list_width_hint" },
  { id: "appearance-roundness", section: "appearance", titleKey: "settings.roundness", descriptionKey: "settings.roundness_hint" },
  { id: "appearance-animations", section: "appearance", titleKey: "settings.animations", descriptionKey: "settings.animations_hint" },
  { id: "appearance-fine-tune", section: "appearance", titleKey: "settings.fine_tune", descriptionKey: "settings.fine_tune_hint" },
  { id: "appearance-css", section: "appearance", titleKey: "settings.css_extensions", descriptionKey: "settings.css_hint" },
  { id: "accessibility-mode", section: "accessibility", titleKey: "settings.a11y.mode_title", descriptionKey: "settings.a11y.mode_hint" },
  { id: "accessibility-reduce-motion", section: "accessibility", titleKey: "settings.a11y.reduce_motion", descriptionKey: "settings.a11y.reduce_motion_hint" },
  { id: "accessibility-pause-media", section: "accessibility", titleKey: "settings.a11y.pause_media", descriptionKey: "settings.a11y.pause_media_hint" },
  { id: "accessibility-high-contrast", section: "accessibility", titleKey: "settings.a11y.high_contrast", descriptionKey: "settings.a11y.high_contrast_hint" },
  { id: "accessibility-transparency", section: "accessibility", titleKey: "settings.a11y.reduce_transparency", descriptionKey: "settings.a11y.reduce_transparency_hint" },
  { id: "accessibility-target-size", section: "accessibility", titleKey: "settings.a11y.target_size", descriptionKey: "settings.a11y.target_size_hint" },
  { id: "accessibility-always-focus", section: "accessibility", titleKey: "settings.a11y.always_focus", descriptionKey: "settings.a11y.always_focus_hint" },
  { id: "accessibility-enhanced-focus", section: "accessibility", titleKey: "settings.a11y.enhanced_focus", descriptionKey: "settings.a11y.enhanced_focus_hint" },
  { id: "accessibility-color-blind", section: "accessibility", titleKey: "settings.a11y.color_blind", descriptionKey: "settings.a11y.color_blind_hint" },
  { id: "accessibility-text-size", section: "accessibility", titleKey: "settings.a11y.text_size", descriptionKey: "settings.a11y.text_size_hint" },
  { id: "accessibility-text-spacing", section: "accessibility", titleKey: "settings.a11y.text_spacing", descriptionKey: "settings.a11y.text_spacing_hint" },
  { id: "accessibility-font", section: "accessibility", titleKey: "settings.a11y.font_choice", descriptionKey: "settings.a11y.font_choice_hint" },
  { id: "accessibility-shortcut-hints", section: "accessibility", titleKey: "settings.a11y.shortcut_hints", descriptionKey: "settings.a11y.shortcut_hints_hint" },
  { id: "accessibility-char-shortcuts", section: "accessibility", titleKey: "settings.a11y.char_shortcuts", descriptionKey: "settings.a11y.char_shortcuts_hint" },
  { id: "accessibility-announcements", section: "accessibility", titleKey: "settings.a11y.announcements", descriptionKey: "settings.a11y.announcements_hint" },
  { id: "accessibility-autoplay", section: "accessibility", titleKey: "settings.a11y.autoplay", descriptionKey: "settings.a11y.autoplay_hint" },
  { id: "advanced-verbose-logs", section: "advanced", titleKey: "settings.main.verbose_logs", descriptionKey: "settings.main.verbose_logs_hint" },
  { id: "about-store-health", section: "about", titleKey: "settings.store_title", descriptionKey: "settings.store_recovery_hint" },
] as const;

export function dynamicSettingSearchFields(input: {
  privacy: readonly { category: string; label: string }[];
  keybinds: readonly { id: string }[];
  mediaKinds: readonly string[];
  themeTokens: readonly { key: string }[];
}): SettingSearchDefinition[] {
  return [
    ...input.privacy.map(({ category, label }) => ({ id: `whatsapp-privacy-${category}`, section: "whatsapp", titleKey: label })),
    ...input.keybinds.map(({ id }) => ({ id: `keybind-${id}`, section: "keybinds", titleKey: `settings.main.keybind.${id}.label`, descriptionKey: `settings.main.keybind.${id}.description` })),
    ...input.mediaKinds.map((kind) => ({ id: `media-download-${kind}`, section: "media", titleKey: `settings.media_${kind}`, descriptionKey: "settings.download_hint" })),
    ...input.themeTokens.map(({ key }) => ({ id: `appearance-token-${key}`, section: "appearance", titleKey: `settings.token_${key}`, descriptionKey: "settings.fine_tune_hint" })),
  ];
}

export function localizeSettingSearchFields(
  fields: readonly SettingSearchDefinition[],
  translate: (key: string, params?: Record<string, string | number | boolean | null>) => string,
  sections: readonly string[],
): SettingSearchItem[] {
  const visible = new Set(sections);
  return fields.filter((field) => visible.has(field.section)).map((field) => ({
    ...field,
    title: translate(field.titleKey),
    description: field.descriptionKey ? translate(field.descriptionKey, field.descriptionParams) : "",
  }));
}

function normalize(value: string) {
  return value.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

export function matchSettingSearch<T extends Pick<SettingSearchItem, "title" | "description">>(items: readonly T[], query: string): T[] {
  const terms = normalize(query.trim()).split(/\s+/).filter(Boolean);
  if (!terms.length) return [];
  return items.filter(({ title, description }) => {
    const searchable = normalize(`${title} ${description}`);
    return terms.every((term) => searchable.includes(term));
  });
}
