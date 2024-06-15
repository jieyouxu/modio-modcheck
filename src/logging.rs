use tracing::*;

pub(crate) fn setup_logging() {
    use tracing::metadata::LevelFilter;
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{fmt, EnvFilter};

    let stderr_log = fmt::layer()
        .with_writer(std::io::stderr)
        .compact()
        .with_level(true)
        .with_target(true)
        .without_time()
        .with_filter(
            EnvFilter::builder().with_default_directive(LevelFilter::INFO.into()).from_env_lossy(),
        );
    let subscriber = tracing_subscriber::registry().with(stderr_log);

    tracing::subscriber::set_global_default(subscriber).unwrap();

    debug!("tracing subscriber setup");
}
