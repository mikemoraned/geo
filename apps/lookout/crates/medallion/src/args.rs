//! The `--medallion-root` argument every CLI touching the store shares.

use std::path::PathBuf;

use crate::path::Root;

/// Flattened into each CLI's own args struct, so the flag name and default are identical
/// everywhere the store is read or written.
#[derive(Debug, Clone, clap::Args)]
pub struct MedallionArgs {
    /// Root of the medallion data store.
    ///
    /// Global, so it is accepted wherever it reads naturally on the command line —
    /// before a subcommand or after it.
    #[arg(long = "medallion-root", global = true, default_value_os_t = Root::default_path())]
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
        #[command(subcommand)]
        command: Option<Command>,
        #[command(flatten)]
        medallion: MedallionArgs,
    }

    #[derive(clap::Subcommand)]
    enum Command {
        Run,
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

    /// A CLI with subcommands takes the flag on either side of the subcommand, so the
    /// order it is typed in never has to be remembered.
    #[test]
    fn the_root_is_accepted_before_or_after_a_subcommand() {
        for args in [
            ["a-cli", "--medallion-root", "/store", "run"],
            ["a-cli", "run", "--medallion-root", "/store"],
        ] {
            let cli = Cli::parse_from(args);

            assert_eq!(
                cli.medallion.root().path(),
                PathBuf::from("/store"),
                "parsing {args:?}"
            );
        }
    }
}
