use std::{env, io};

use color_eyre::Result;
use log::info;
use lsn_core::{FsNodeKind, StateManager};
use lsn_ui::{
    ViewItem,
    app::{Action, Ui},
};
use ratatui::crossterm::{
    cursor::{MoveUp, Show},
    execute,
    terminal::{Clear, ClearType, disable_raw_mode},
};

mod projection;
use projection::state_to_view;

fn main() -> Result<()> {
    #[cfg(debug_assertions)]
    init_debug_logger();

    color_eyre::install()?;

    let tui_height = 50;

    let mut terminal = ratatui::init_with_options(ratatui::TerminalOptions {
        viewport: ratatui::Viewport::Inline(tui_height),
    });

    terminal.clear()?;
    let mut should_exit = false;
    let cwd = env::current_dir()?;
    let mut state = StateManager::new(cwd.clone());
    let mut ui_app = Ui::new()?;
    let _ = state.load_dir(&cwd);
    state.set_open(&cwd, true);

    let mut view_cache = Vec::<ViewItem>::new();
    let mut should_rebuild_view = true;

    while !should_exit {
        if should_rebuild_view {
            view_cache = state_to_view(&state, &ui_app.filter, &ui_app.sort);
            should_rebuild_view = false;
        }
        ui_app.draw(&mut terminal, &view_cache)?;

        if let Some(action) = ui_app.handle_input()? {
            match action {
                Action::Quit => should_exit = true,
                Action::ToggleFolder => {
                    toggle_folder(&mut ui_app, &view_cache, &mut state);
                    should_rebuild_view = true;
                }
                Action::CloseNearest => {
                    close_nearest(&mut ui_app, &view_cache, &mut state);
                    should_rebuild_view = true;
                }
                _ => {}
            }
        }
    }

    terminal.clear()?;
    disable_raw_mode()?;

    execute!(
        io::stdout(),
        Show,
        MoveUp(tui_height),
        Clear(ClearType::FromCursorDown)
    )?;

    Ok(())
}

fn toggle_folder(ui_app: &mut Ui, view_cache: &Vec<ViewItem>, state: &mut StateManager) {
    if let Some(selected_index) = ui_app.state.selected()
        && let Some(item) = view_cache.get(selected_index)
        && let Some(entry) = state.get_entry(&item.path)
        && let FsNodeKind::Directory { children, is_open } = &entry.kind
    {
        if !is_open && children.is_empty() {
            let _ = state.load_dir(&item.path);
        }
        state.toggle_open(&item.path);
    }
}

fn close_nearest(ui_app: &mut Ui, view_cache: &Vec<ViewItem>, state: &mut StateManager) {
    if let Some(selected_index) = ui_app.state.selected()
        && let Some(item) = view_cache.get(selected_index)
    {
        match item.kind {
            lsn_ui::ViewItemKind::Directory { is_open: true } => {
                state.set_open(&item.path, false);
            }
            _ => {
                if let Some(parent_path) = item.path.parent()
                    && let Some(idx) = view_cache.iter().position(|i| i.path == parent_path)
                {
                    state.set_open(&parent_path, false);
                    ui_app.state.select(Some(idx));
                }
            }
        }
    }
}

fn init_debug_logger() {
    use simplelog::{Config, WriteLogger};
    use std::fs::File;

    log_panics::init();

    WriteLogger::init(
        log::LevelFilter::Debug,
        Config::default(),
        File::create("debug.log").unwrap(),
    )
    .unwrap();

    info!("debug logger initialized")
}
