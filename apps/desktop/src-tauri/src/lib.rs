//! VEXEL desktop shell. Per `CLAUDE.md` section 3 this is a **thin adapter**: it wires Tauri commands
//! and events to the `vxl-*` crates and holds no product logic itself.

mod deeplink;
mod net;
mod telemetry;

use std::sync::Arc;

use serde::Serialize;
use tauri::Manager;
use vxl_core::brand;
use vxl_metrics::CoreMetrics;

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
                .unwrap_or_else(|_| "info,vexel_desktop_lib=debug,iroh=warn".into()),
        )
        .init();

    let result = tauri::Builder::default()
        // Must be the first plugin: a second launch (e.g. clicking a vxl:// link) forwards its
        // arguments here and exits; the deep-link feature turns them into `on_open_url` events.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            deeplink::focus_main(app);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .invoke_handler(tauri::generate_handler![
            app_info,
            deeplink::deeplink_take,
            net::profile_get,
            net::profile_set_name,
            net::link_status,
            net::sectors_list,
            net::sector_create,
            net::sector_key_preview,
            net::sector_join,
            net::sector_key,
            net::sector_leave,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let metrics = Arc::new(CoreMetrics::default());
            telemetry::spawn(handle.clone(), Arc::clone(&metrics));
            deeplink::setup(&handle);
            tauri::async_runtime::spawn(async move {
                if let Err(err) = net::start(handle, metrics).await {
                    tracing::error!(%err, "link node failed to start; running offline");
                }
            });
            if let Some(window) = app.get_webview_window("main") {
                tracing::debug!(label = window.label(), "main window ready");
            }
            Ok(())
        })
        .build(tauri::generate_context!());

    let app = match result {
        Ok(app) => app,
        Err(err) => {
            tracing::error!(%err, "VEXEL shell failed to start");
            std::process::exit(1);
        }
    };
    app.run(|handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            // Announce Leave in every Sector so peers see us go offline at once.
            tauri::async_runtime::block_on(net::shutdown(handle));
        }
    });
}
