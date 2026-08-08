//! maps — one binary, two roles (clappkit `role`): `maps app` is the Tauri process Clatch
//! launches (window + control pipe + the IPC server the agent's CLI dials), and
//! `maps <verb>` is that CLI. Both drive ONE [`state::AppState`], which is the entire point
//! of the app: a map is a thing two people look at together.
//!
//! `geo` is the world — three open services (Photon, Nominatim, Valhalla), spoken to
//! natively over HTTPS, with the rate discipline they are owed. `state` is what both
//! surfaces are looking at. `app` is the wiring.
//!
//! There is deliberately NO `windows_subsystem = "windows"` attribute here. It applies to
//! the whole image, but this image is two roles: a GUI-subsystem process gets no console
//! and is not waited on by the `.cmd` shim Clatch links onto the agent's PATH, so every
//! `maps <verb>` call would return instantly, empty, with exit code 0 — the agent's entire
//! interface, silently dead, in release builds only. Clatch already spawns the launch
//! command with `CREATE_NO_WINDOW`, so a console-subsystem clapp shows no console.

mod app;
mod cli;
mod geo;
mod state;
mod webview;

const APP_ID: &str = "com.arfium.maps";
const CLI: &str = "maps";

fn main() {
    // Windows draws this window with the Edge WebView2 Runtime, which is not part of the
    // app — and this app leans on it harder than most, because the map is WebGL. Checked
    // here, before Tauri looks for it, so a missing runtime is a sentence with a download
    // link instead of a modal dialog from a loader nobody has heard of. A no-op everywhere
    // else, and never on the CLI path — `maps --help` needs no webview.
    clappkit::role::main_dispatch(APP_ID, CLI, cli::run, || {
        webview::ensure(CLI);
        app::run()
    })
}
