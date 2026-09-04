mod app;
mod auto_save;
mod dap_client;
mod debug_state;
mod errors;
mod file_watch;
mod folder_picker;
mod goto_definition;
mod jdk;
mod jdk_registry;
mod lsp_client;
mod lsp_manager;
mod lsp_settings;
mod lsp_state;
mod panels;
mod pty_session;
mod rename;
mod style;
mod terminal;
mod toasts;
mod tool_manager;
mod widgets;
mod workspace_edit;

use app::FoxGardenApp;

// A downscaled copy of assets/icon/icon.png: winit's X11 backend silently
// drops oversized `_NET_WM_ICON` property writes (see AGENTS.md), so the
// window icon needs a small bundled copy rather than the pristine
// 1024x1024 source.
const ICON_PNG: &[u8] = include_bytes!("../assets/icon/icon_128.png");

fn main() -> eframe::Result {
    let icon = eframe::icon_data::from_png_bytes(ICON_PNG).expect("bundled icon.png must decode");

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "FoxGarden",
        native_options,
        Box::new(|cc| {
            style::fonts::install(&cc.egui_ctx);
            Ok(Box::new(FoxGardenApp::new(cc)))
        }),
    )
}
