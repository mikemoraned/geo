//! The `--medallion-root` argument every CLI touching the store shares.

use std::path::PathBuf;

use crate::path::Root;

/// Flattened into each CLI's own args struct, so the flag name and default are identical
/// everywhere the store is read or written.
#[derive(Debug, Clone, clap::Args)]
pub struct MedallionArgs {
    /// Root of the medallion data store.
    #[arg(long = "medallion-root", default_value_os_t = Root::default_path())]
    pub medallion_root: PathBuf,
}

impl MedallionArgs {
    pub fn root(&self) -> Root {
        Root::new(self.medallion_root.clone())
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        medallion: MedallionArgs,
    }

    #[test]
    fn the_root_defaults_to_the_documented_location() {
        let cli = Cli::parse_from(["a-cli"]);

        assert_eq!(cli.medallion.root(), Root::default());
    }

    #[test]
    fn the_root_can_be_pointed_elsewhere() {
        let cli = Cli::parse_from(["a-cli", "--medallion-root", "/Volumes/PRO-G40/medallion"]);

        assert_eq!(
            cli.medallion.root().path(),
            PathBuf::from("/Volumes/PRO-G40/medallion")
        );
    }
}
