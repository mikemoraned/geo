use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use chrono::NaiveDate;

const DATE_FORMAT: &str = "%Y-%m-%d";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Release {
    date: NaiveDate,
    reissue: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
#[error("{0} is not a release name of the form YYYY-MM-DD.N")]
pub struct NotARelease(String);

impl Release {
    pub fn new(name: impl Into<String>) -> Result<Self, NotARelease> {
        let name = name.into();
        let parsed = name.rsplit_once('.').and_then(|(date, reissue)| {
            Some(Self {
                date: NaiveDate::parse_from_str(date, DATE_FORMAT).ok()?,
                reissue: reissue.parse().ok()?,
            })
        });
        parsed.ok_or(NotARelease(name))
    }
}

impl FromStr for Release {
    type Err = NotARelease;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::new(name)
    }
}

impl fmt::Display for Release {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}", self.date.format(DATE_FORMAT), self.reissue)
    }
}

pub fn find_superseded(served: &[Release]) -> BTreeMap<Release, Release> {
    let mut latest_of_each_date: BTreeMap<NaiveDate, &Release> = BTreeMap::new();
    for release in served {
        latest_of_each_date
            .entry(release.date)
            .and_modify(|latest| *latest = (*latest).max(release))
            .or_insert(release);
    }
    served
        .iter()
        .filter_map(|release| {
            let latest = latest_of_each_date[&release.date];
            (latest != release).then(|| (release.clone(), latest.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::*;

    fn release(name: &str) -> Release {
        name.parse().expect("a release name")
    }

    fn releases(names: &[&str]) -> Vec<Release> {
        names.iter().map(|name| release(name)).collect()
    }

    #[test]
    fn a_name_without_a_reissue_is_not_a_release() {
        assert!(Release::new("2026-09-23").is_err());
    }

    #[test]
    fn a_name_whose_date_does_not_exist_is_not_a_release() {
        assert!(Release::new("2026-02-30.0").is_err());
    }

    #[test]
    fn a_release_prints_as_its_name() {
        assert_eq!(release("2026-09-23.1").to_string(), "2026-09-23.1");
    }

    #[test]
    fn a_later_release_of_the_same_date_supersedes_the_earlier() {
        assert_eq!(
            find_superseded(&releases(&["2026-08-19.0", "2026-09-23.0", "2026-09-23.1"])),
            BTreeMap::from([(release("2026-09-23.0"), release("2026-09-23.1"))])
        );
    }

    #[test]
    fn a_tenth_reissue_outranks_a_second() {
        assert_eq!(
            find_superseded(&releases(&["2026-09-23.10", "2026-09-23.2"])),
            BTreeMap::from([(release("2026-09-23.2"), release("2026-09-23.10"))])
        );
    }

    fn served() -> impl Strategy<Value = Vec<Release>> {
        prop::collection::btree_set((1..=28u32, 0..12u32), 0..20).prop_map(|names| {
            names
                .into_iter()
                .map(|(day, reissue)| Release {
                    date: NaiveDate::from_ymd_opt(2026, 9, day).expect("a day in September"),
                    reissue,
                })
                .collect()
        })
    }

    proptest! {
        #[test]
        fn each_date_keeps_exactly_its_latest_release(served in served()) {
            let superseded = find_superseded(&served);
            let current: BTreeSet<&Release> =
                served.iter().filter(|release| !superseded.contains_key(release)).collect();
            let dates: BTreeSet<NaiveDate> = served.iter().map(|release| release.date).collect();

            prop_assert_eq!(current.len(), dates.len());
            for release in &current {
                prop_assert!(served.iter().all(|other| other.date != release.date || other <= release));
            }
        }

        #[test]
        fn a_release_is_superseded_by_a_later_current_one_of_its_date(served in served()) {
            let superseded = find_superseded(&served);
            for (earlier, latest) in &superseded {
                prop_assert_eq!(earlier.date, latest.date);
                prop_assert!(earlier.reissue < latest.reissue);
                prop_assert!(!superseded.contains_key(latest));
            }
        }
    }
}
