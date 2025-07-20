use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

pub fn setup_tracing_and_logging() -> Result<(), Box<dyn std::error::Error>> {
    let fmt_filter = EnvFilter::from_default_env();
    let fmt_layer = fmt::layer().with_filter(fmt_filter);
    tracing_subscriber::registry().with(fmt_layer).try_init()?;

    Ok(())
}