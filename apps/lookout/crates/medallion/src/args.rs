//! The `--medallion-root` argument every CLI touching the store shares.

use std::path::PathBuf;

use crate::path::{Root, StoreNotFound};

/// Flattened into each CLI's own args struct, so the flag name and default are identical
/// everywhere the store is read or written.
#[derive(Debug, Clone, clap::Args)]
pub struct MedallionArgs {
    /// Root of the medallion data store. Defaults to `data/medallion` in the repo this was
    /// run from.
    ///
    /// Global, so it is accepted wherever it reads naturally on the command line —
    /// before a subcommand or after it.
    #[arg(long = "medallion-root", global = true)]
    pub medallion_root: Option<PathBuf>,
}

impl MedallionArgs {
    /// The store to work on: the one named, or the repo's own.
    ///
    /// Working out the default can fail — a binary run from outside the repo has no store to
    /// find — which is why this is fallible rather than defaulting to a path that may be the
    /// wrong one. The flag then says where the store is.
    pub fn root(&self) -> Result<Root, StoreNotFound> {
        match &self.medallion_root {
            Some(path) => Ok(Root::new(path.clone())),
            None => Ok(Root::new(Root::default_path()?)),
        }
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

    /// Given nothing, the store is the repo's own — which the tests themselves run inside,
    /// so the walk up for the workspace finds it.
    #[test]
    fn the_root_defaults_to_the_store_in_the_repo() {
        let cli = Cli::parse_from(["a-cli"]);

        let root = cli.medallion.root().expect("locate the store");
        assert!(
            root.path().ends_with("data/medallion"),
            "unexpected default: {}",
            root.path().display()
        );
        assert!(root.path().is_absolute());
    }

    #[test]
    fn the_root_can_be_pointed_elsewhere() {
        let cli = Cli::parse_from(["a-cli", "--medallion-root", "/Volumes/PRO-G40/medallion"]);

        assert_eq!(
            cli.medallion.root().expect("the named store").path(),
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
                cli.medallion.root().expect("the named store").path(),
                PathBuf::from("/store"),
                "parsing {args:?}"
            );
        }
    }
}
