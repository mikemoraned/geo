pub const REDIS_URL_VAR: &str = "LOOKOUT_REDIS_URL";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, clap::ValueEnum)]
pub enum Store {
    Redis,
    InMemory,
}
