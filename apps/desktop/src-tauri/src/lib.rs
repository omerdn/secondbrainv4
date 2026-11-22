//! Tauri 2 desktop shell around the client's web build (ADR-0002, ADR-0020, T4.1.1).
//!
//! The web app keeps its session token in the OS secure storage through the
//! `session_token_*` commands (macOS Keychain, Windows Credential Manager, Linux Secret
//! Service) and opens `/ws` through the WebSocket plugin, which can send the bearer header.
//! On launch it checks GitHub Releases for a signed update and installs it (ADR-0023).
//! `routune://` links open the running app and route inside it (T4.1.5).

use tauri::Manager;

/// Keychain service and account of the session token.
const SERVICE: &str = "app.secondbrain.desktop";
const ACCOUNT: &str = "session-token";

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(|err| err.to_string())
}

/// The stored session token, if any.
#[tauri::command]
fn session_token_get() -> Result<Option<String>, String> {
    match entry()?.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

/// Stores the session token, replacing any previous one.
#[tauri::command]
fn session_token_set(token: String) -> Result<(), String> {
    entry()?.set_password(&token).map_err(|err| err.to_string())
}

/// Forgets the session token (sign-out); a missing token is not an error.
#[tauri::command]
fn session_token_clear() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

/// Builds and runs the desktop app.
pub fn run() {
    tauri::Builder::default()
        // First: a second launch (e.g. a clicked routune:// link on Windows/Linux) hands its
        // arguments to the running app instead of opening another window.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .setup(|_app| {
            // macOS registers the scheme from the bundle; Windows and Linux at runtime.
            #[cfg(any(windows, target_os = "linux"))]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                _app.deep_link().register_all()?;
            }
            Ok(())
        })
        .plugin(tauri_plugin_websocket::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            session_token_get,
            session_token_set,
            session_token_clear
        ])
        .run(tauri::generate_context!())
        .expect("error while running the desktop app");
}
