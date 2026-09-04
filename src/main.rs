//! protonctx — launch executables inside a Steam game's Proton context.

mod app;
mod launcher;
mod models;
mod steam;

fn main() -> Result<(), slint::PlatformError> {
    app::run()
}
