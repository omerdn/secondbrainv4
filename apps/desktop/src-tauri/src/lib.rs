//! Tauri 2 shell around the client's web build (ADR-0002, spike T0.6.3).

/// Spike diagnostics: the web app reports its connectivity checks here, printed to stdout so
/// the run can be verified without driving the window.
#[tauri::command]
fn spike_report(report: serde_json::Value) {
    println!("SPIKE_REPORT {report}");
}

/// Builds and runs the desktop app.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_websocket::init())
        .invoke_handler(tauri::generate_handler![spike_report])
        .run(tauri::generate_context!())
        .expect("error while running the desktop app");
}
