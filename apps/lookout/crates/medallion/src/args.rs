use std::path::PathBuf;

use crate::path::{Root, StoreNotFound};

#[derive(Debug, Clone, clap::Args)]
pub struct MedallionArgs {
    #[arg(long = "medallion-root", global = true)]
    pub medallion_root: Option<PathBuf>,
}

impl MedallionArgs {
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
