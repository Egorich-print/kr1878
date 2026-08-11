// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/

use std::sync::Mutex;

mod ops;

use ops::{BackendKind, SessionSpec};
use tauri::State;

/// The live device backend shared across program/verify/read calls, so a
/// multi-step session (write then verify then read) sees consistent state —
/// mirroring a real programmer attached to the same chip.
struct AppBackend(Mutex<Box<dyn kr1878_core::backend::Backend>>);

impl AppBackend {
    fn lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Box<dyn kr1878_core::backend::Backend>>, CommandError>
    {
        self.0
            .lock()
            .map_err(|_| CommandError::new("internal error: backend lock poisoned"))
    }
}

#[derive(serde::Serialize)]
struct CommandError {
    message: String,
}

impl CommandError {
    fn new(e: impl std::fmt::Display) -> Self {
        CommandError {
            message: e.to_string(),
        }
    }
}

type CmdResult<T> = Result<T, CommandError>;

#[tauri::command]
fn list_backends() -> CmdResult<Vec<(String, String)>> {
    Ok(BackendKind::ALL
        .into_iter()
        .map(|b| (b.id().to_string(), b.label().to_string()))
        .collect())
}

#[tauri::command]
fn assemble(source: String) -> CmdResult<ops::AssembleResult> {
    ops::assemble(&source).map_err(CommandError::new)
}

/// Connect the selected backend; replaces the session backend in the managed
/// state so all subsequent program/verify/read calls target this device.
#[tauri::command]
fn connect(
    state: State<'_, AppBackend>,
    backend: BackendKind,
    sim_state: Option<String>,
) -> CmdResult<String> {
    let conn = ops::connect(backend, sim_state.as_deref()).map_err(CommandError::new)?;
    let label = conn.label.clone();
    let mut slot = state.lock()?;
    *slot = conn.backend;
    Ok(label)
}

#[tauri::command]
fn program(state: State<'_, AppBackend>, spec: SessionSpec) -> CmdResult<bool> {
    let mut backend = state.lock()?;
    ops::program(&mut **backend, &spec).map_err(CommandError::new)
}

#[tauri::command]
fn verify(state: State<'_, AppBackend>, spec: SessionSpec) -> CmdResult<bool> {
    let mut backend = state.lock()?;
    ops::verify(&mut **backend, &spec).map_err(CommandError::new)
}

#[tauri::command]
fn read(state: State<'_, AppBackend>) -> CmdResult<ops::ImageContents> {
    let mut backend = state.lock()?;
    ops::read(&mut **backend).map_err(CommandError::new)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppBackend(Mutex::new(Box::new(
            kr1878_programmer::MockBackend::new(),
        ))))
        .invoke_handler(tauri::generate_handler![
            list_backends,
            assemble,
            connect,
            program,
            verify,
            read
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
