#![cfg_attr(windows, windows_subsystem = "windows")]
mod dialog_theme;
mod model;
mod settings;
mod view;
#[cfg(windows)]
mod win;
fn main() {
    #[cfg(windows)]
    {
        if let Err(e) = win::run() {
            eprintln!("A2AHandoff UI: {e}");
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!("This is the Windows shell; model and layout are portable.");
    }
}
