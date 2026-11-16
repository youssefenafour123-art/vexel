//! VEXEL desktop shell. Per `CLAUDE.md` section 3 this is a **thin adapter**: it wires Tauri commands
//! and events to the `vxl-*` crates and holds no product logic itself.

mod telemetry;

use serde::Serialize;
use tauri::Manager;
use vxl_core::brand;

/// Static build and brand info for the UI.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    product: &'static str,
    os_tag: &'static str,
    tagline: &'static str,
    version: &'static str,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        product: brand::PRODUCT,
        os_tag: brand::OS_TAG,
        tagline: brand::TAGLINE,
        version: brand::VERSION,
    }
}

/// Entry point shared by the binary (and, in future, mobile targets).
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,vexel_desktop_lib=debug".into()),
        )
        .init();

    let result = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_info])
        .setup(|app| {
            telemetry::spawn(app.handle().clone());
            if let Some(window) = app.get_webview_window("main") {
                tracing::debug!(label = window.label(), "main window ready");
            }
            Ok(())
        })
        .run(tauri::generate_context!());

    if let Err(err) = result {
        tracing::error!(%err, "VEXEL shell exited with error");
        std::process::exit(1);
    }
}
