//! `vxl://` deep links. The OS launches VEXEL with the URL. If VEXEL is already running, the
//! single-instance plugin forwards it here. The UI then opens the join dialog prefilled; we never
//! auto-join without the user confirming.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;
use vxl_core::brand;

/// Event carrying a Sector Key string. Mirrored in `apps/desktop/src/lib/net.ts`.
pub const EVENT_SECTOR_KEY: &str = "deeplink:sector-key";

/// The most recent Sector Key the UI has not picked up yet (e.g. the launch URL).
#[derive(Debug, Default)]
pub struct PendingKey(Mutex<Option<String>>);

pub fn setup(app: &AppHandle) {
    app.manage(PendingKey::default());

    // Installers register the scheme. In dev builds, register it for the current exe (HKCU only).
    #[cfg(debug_assertions)]
    if let Err(err) = app.deep_link().register_all() {
        tracing::warn!(%err, "could not register vxl:// for dev");
    }

    if let Ok(Some(urls)) = app.deep_link().get_current() {
        for url in urls {
            handle(app, url.as_str());
        }
    }
    let handle_app = app.clone();
    app.deep_link().on_open_url(move |event| {
        for url in event.urls() {
            handle(&handle_app, url.as_str());
        }
    });
}

fn handle(app: &AppHandle, url: &str) {
    let scheme = format!("{}://", brand::URL_SCHEME);
    if !url.to_ascii_lowercase().starts_with(&scheme) {
        return;
    }
    tracing::debug!("sector key received via deep link");
    if let Some(p) = app.try_state::<PendingKey>() {
        *p.0.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(url.to_owned());
    }
    let _ = app.emit(EVENT_SECTOR_KEY, url);
    focus_main(app);
}

/// Brings the main window to the front (second launch or deep link).
pub fn focus_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Returns and clears the pending Sector Key.
#[tauri::command]
pub fn deeplink_take(app: AppHandle) -> Option<String> {
    app.try_state::<PendingKey>().and_then(|p| {
        p.0.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    })
}
