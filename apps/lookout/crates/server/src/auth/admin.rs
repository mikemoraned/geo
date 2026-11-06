use crate::auth::code::ShortCode;
use crate::auth::store::{AllowedPasskey, AuthStore, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, clap::Subcommand)]
pub enum Command {
    /// List the passkeys that can sign in.
    List,
    /// Let the pending passkey with this code sign in.
    Allow {
        /// The short code shown after registration.
        code: ShortCode,
        /// A name to tell this passkey apart in the list, such as the device it is on.
        #[arg(long)]
        label: Option<String>,
    },
    /// Stop the passkey with this code signing in, and end its sessions.
    Revoke {
        /// The short code shown in the list.
        code: ShortCode,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum AdminError {
    #[error("no pending passkey has the code {0}. A code expires an hour after registration.")]
    NoPendingPasskey(ShortCode),
    #[error("no allowed passkey has the code {0}")]
    NoAllowedPasskey(ShortCode),
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub async fn run(store: &dyn AuthStore, command: Command) -> Result<String, AdminError> {
    match command {
        Command::List => Ok(listed(store.list_allowed().await?)),
        Command::Allow { code, label } => {
            let allowed = store
                .allow(&code, label)
                .await?
                .ok_or(AdminError::NoPendingPasskey(code))?;
            Ok(match allowed.label {
                Some(label) => format!("allowed {} as {label}", allowed.code),
                None => format!("allowed {}", allowed.code),
            })
        }
        Command::Revoke { code } => {
            let revoked = store
                .remove_allowed(&code)
                .await?
                .ok_or(AdminError::NoAllowedPasskey(code))?;
            Ok(format!("revoked {}", revoked.code))
        }
    }
}

const NO_LABEL: &str = "-";
const NEVER: &str = "never";
const MINUTES: &str = "%Y-%m-%d %H:%M UTC";

fn listed(mut passkeys: Vec<AllowedPasskey>) -> String {
    if passkeys.is_empty() {
        return "no passkey can sign in".into();
    }
    passkeys.sort_by_key(|passkey| passkey.allowed_at);
    let rows: Vec<[String; 4]> = passkeys
        .into_iter()
        .map(|passkey| {
            [
                passkey.code.to_string(),
                passkey.label.unwrap_or_else(|| NO_LABEL.into()),
                passkey.allowed_at.format(MINUTES).to_string(),
                passkey
                    .last_used_at
                    .map_or_else(|| NEVER.into(), |used| used.format(MINUTES).to_string()),
            ]
        })
        .collect();
    let header = ["CODE", "LABEL", "ALLOWED", "LAST USED"].map(String::from);
    let width = |column: usize| {
        rows.iter()
            .chain([&header])
            .map(|row| row[column].chars().count())
            .max()
            .unwrap_or_default()
    };
    let (code_width, label_width, allowed_width) = (width(0), width(1), width(2));
    [header]
        .iter()
        .chain(&rows)
        .map(|[code, label, allowed, used]| {
            format!("{code:<code_width$}  {label:<label_width$}  {allowed:<allowed_width$}  {used}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use clap::Parser;

    use super::*;
    use crate::auth::contract::{allowed_passkey, pending};
    use crate::auth::memory::MemoryAuthStore;
    use crate::auth::test_passkeys::registered_passkey;

    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        command: Command,
    }

    fn parsed(args: &[&str]) -> Command {
        Cli::try_parse_from([&["passkeys"], args].concat())
            .expect("a command")
            .command
    }

    #[test]
    fn a_typed_code_is_read_as_its_short_code() {
        assert_eq!(
            parsed(&["allow", "abcd-efgh", "--label", "iPhone"]),
            Command::Allow {
                code: ShortCode::new("ABCDEFGH").expect("a code"),
                label: Some("iPhone".into()),
            }
        );
    }

    #[test]
    fn a_malformed_code_is_refused() {
        assert!(Cli::try_parse_from(["passkeys", "revoke", "ABC"]).is_err());
    }

    #[tokio::test]
    async fn allowing_a_pending_passkey_lets_it_sign_in_under_its_label() {
        let store = MemoryAuthStore::default();
        let passkey = registered_passkey().await;
        let id = passkey.cred_id().clone();
        let code = store.put_pending(pending(passkey)).await.expect("stored");

        let said = run(
            &store,
            Command::Allow {
                code: code.clone(),
                label: Some("iPhone".into()),
            },
        )
        .await
        .expect("allowed");

        assert_eq!(said, format!("allowed {code} as iPhone"));
        let allowed = store.find_allowed(&id).await.expect("found");
        assert_eq!(
            allowed.and_then(|allowed| allowed.label),
            Some("iPhone".into())
        );
    }

    #[tokio::test]
    async fn allowing_an_unknown_code_is_refused() {
        let store = MemoryAuthStore::default();
        let code = ShortCode::random();

        let refused = run(
            &store,
            Command::Allow {
                code: code.clone(),
                label: None,
            },
        )
        .await;

        assert!(
            matches!(&refused, Err(AdminError::NoPendingPasskey(refused)) if *refused == code),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn revoking_an_allowed_passkey_stops_it_signing_in() {
        let store = MemoryAuthStore::default();
        let allowed = allowed_passkey(&store).await;

        let said = run(
            &store,
            Command::Revoke {
                code: allowed.code.clone(),
            },
        )
        .await
        .expect("revoked");

        assert_eq!(said, format!("revoked {}", allowed.code));
        let found = store
            .find_allowed(allowed.passkey.cred_id())
            .await
            .expect("found");
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn revoking_an_unknown_code_is_refused() {
        let store = MemoryAuthStore::default();
        allowed_passkey(&store).await;
        let code = ShortCode::random();

        let refused = run(&store, Command::Revoke { code: code.clone() }).await;

        assert!(
            matches!(&refused, Err(AdminError::NoAllowedPasskey(refused)) if *refused == code),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn the_list_shows_each_allowed_passkey_with_its_label_and_last_use() {
        let store = MemoryAuthStore::default();
        let unused = allowed_passkey(&store).await;
        let mut used = allowed_passkey(&store).await;
        used.label = Some("iPhone".into());
        used.last_used_at = Some(Utc::now());
        store.update_allowed(used.clone()).await.expect("updated");

        let said = run(&store, Command::List).await.expect("listed");

        let row = |code: &ShortCode| {
            said.lines()
                .find(|line| line.starts_with(&code.to_string()))
                .unwrap_or_else(|| panic!("no row for {code} in {said}"))
                .to_string()
        };
        assert_eq!(said.lines().count(), 3, "{said}");
        assert!(row(&unused.code).ends_with("never"), "{said}");
        assert!(row(&used.code).contains("iPhone"), "{said}");
        assert!(!row(&used.code).ends_with("never"), "{said}");
    }

    #[tokio::test]
    async fn the_list_without_allowed_passkeys_says_so() {
        let store = MemoryAuthStore::default();

        let said = run(&store, Command::List).await.expect("listed");

        assert_eq!(said, "no passkey can sign in");
    }
}
