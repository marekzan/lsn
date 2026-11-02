use std::path::PathBuf;

use clap::Parser;

/// Explore files in the terminal via a tree view.
///
/// lsn runs inline below your prompt by default. Picked paths (`enter` on a
/// file, `o`, `O`) are printed to stdout, so it can be used in scripts:
///
///     cd "$(lsn ~/projects)"      # press O on a directory
///     vim $(lsn)                  # mark files with space, press o
#[derive(Parser, Debug)]
#[command(author, version = VERSION_MESSAGE, about, verbatim_doc_comment)]
pub struct Cli {
    /// Directories to open, one tab each
    #[arg(value_name = "DIR", default_value = ".")]
    pub dirs: Vec<PathBuf>,

    /// Use the whole terminal (alternate screen) instead of running inline
    #[arg(short, long)]
    pub fullscreen: bool,

    /// Height of the inline viewport in lines, capped to the terminal height
    #[arg(short = 'H', long, value_name = "LINES", default_value_t = 20)]
    pub height: u16,
}

const VERSION_MESSAGE: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "-",
    env!("VERGEN_GIT_DESCRIBE"),
    " (",
    env!("VERGEN_BUILD_DATE"),
    ")"
);
