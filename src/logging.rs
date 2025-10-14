use color_eyre::Result;
use time::macros::format_description;
use tracing_error::ErrorLayer;
use tracing_subscriber::{
    EnvFilter,
    fmt::{self, time::LocalTime},
    prelude::*,
};

use crate::config;

pub fn init() -> Result<()> {
    let directory = config::get_data_dir();
    std::fs::create_dir_all(directory.clone())?;
    let log_path = directory.join("lsn.log");
    let log_file = std::fs::File::create(log_path)?;
    let env_filter = EnvFilter::builder().with_default_directive(tracing::Level::INFO.into());
    let timer = LocalTime::new(format_description!(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond]"
    ));

    // If the `RUST_LOG` environment variable is set, use that as the default, otherwise use the
    // value of the `LOG_ENV` environment variable. If the `LOG_ENV` environment variable contains
    // errors, then this will return an error.
    let env_filter = env_filter
        .try_from_env()
        .or_else(|_| env_filter.with_env_var("LSN_LOG_LEVEL").from_env())?;
    let file_subscriber = fmt::layer()
        .with_file(true)
        .with_line_number(true)
        .with_writer(log_file)
        .with_target(false)
        .with_ansi(false)
        .with_timer(timer)
        .with_filter(env_filter);
    tracing_subscriber::registry()
        .with(file_subscriber)
        .with(ErrorLayer::default())
        .try_init()?;
    Ok(())
}
