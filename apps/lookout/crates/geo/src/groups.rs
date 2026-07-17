//! Group GPS fixes by `(device_id, UTC day)` and reduce each group to its bounding
//! box. The bboxes bound how much Overture transport data a later step needs to
//! fetch: one box per device per day, small enough for a live S3 query.

use std::collections::BTreeMap;

/// A single GPS fix from the archive's `gps` table — only the fields bbox grouping
/// needs (device, capture time, position).
#[derive(Debug, Clone, PartialEq)]
pub struct GpsRow {
    pub device_id: String,
    /// Capture time, epoch milliseconds (the device-stamped `t`).
    pub t: i64,
    pub lat: f64,
    pub lon: f64,
}

/// Milliseconds in a day, for mapping an epoch-ms timestamp to its UTC day.
const MS_PER_DAY: i64 = 86_400_000;

/// The UTC day a fix falls in, as whole days since the Unix epoch. `div_euclid`
/// floors toward negative infinity, so a pre-epoch time (not expected, but harmless)
/// still maps to the correct day rather than truncating toward zero.
pub fn utc_day(t_ms: i64) -> i64 {
    t_ms.div_euclid(MS_PER_DAY)
}

/// A group's identity: one bounding box is derived per device per UTC day.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GroupKey {
    pub device_id: String,
    /// UTC day as whole days since the Unix epoch (see [`utc_day`]).
    pub day: i64,
}

/// An axis-aligned lat/lon bounding box, folded up from a group's fixes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lon: f64,
    pub max_lon: f64,
}

impl BBox {
    /// A degenerate box around a single point (min == max on both axes), the seed a
    /// group's remaining fixes [`extend`](Self::extend) outward from.
    fn around(lat: f64, lon: f64) -> Self {
        Self {
            min_lat: lat,
            max_lat: lat,
            min_lon: lon,
            max_lon: lon,
        }
    }

    /// Grow the box to include `(lat, lon)`.
    fn extend(&mut self, lat: f64, lon: f64) {
        self.min_lat = self.min_lat.min(lat);
        self.max_lat = self.max_lat.max(lat);
        self.min_lon = self.min_lon.min(lon);
        self.max_lon = self.max_lon.max(lon);
    }
}

/// A group and the bounding box of its fixes.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub key: GroupKey,
    pub bbox: BBox,
}

/// Group fixes by `(device_id, UTC day)` and reduce each group to its bounding box.
/// Returned sorted by key, so the result is deterministic regardless of input order.
pub fn group_bboxes(rows: impl IntoIterator<Item = GpsRow>) -> Vec<Group> {
    let mut boxes: BTreeMap<GroupKey, BBox> = BTreeMap::new();
    for row in rows {
        let key = GroupKey {
            device_id: row.device_id,
            day: utc_day(row.t),
        };
        boxes
            .entry(key)
            .and_modify(|b| b.extend(row.lat, row.lon))
            .or_insert_with(|| BBox::around(row.lat, row.lon));
    }
    boxes
        .into_iter()
        .map(|(key, bbox)| Group { key, bbox })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Start-of-day and later-that-same-day UTC timestamps (2023-11-14), plus one the
    /// next day, as epoch-ms — enough to exercise the day boundary.
    const DAY0_MORNING: i64 = 1_700_000_000_000; // 2023-11-14T22:13:20Z
    const DAY0_EARLIER: i64 = 1_699_920_000_000; // 2023-11-14T00:00:00Z
    const DAY1: i64 = 1_700_006_400_000; // 2023-11-15T00:00:00Z

    fn row(device_id: &str, t: i64, lat: f64, lon: f64) -> GpsRow {
        GpsRow {
            device_id: device_id.to_string(),
            t,
            lat,
            lon,
        }
    }

    #[test]
    fn utc_day_is_stable_within_a_day_and_advances_across_midnight() {
        assert_eq!(utc_day(DAY0_MORNING), utc_day(DAY0_EARLIER));
        assert_eq!(utc_day(DAY1), utc_day(DAY0_MORNING) + 1);
    }

    /// A group's bbox spans the min/max lat/lon of its fixes.
    #[test]
    fn bbox_spans_the_extent_of_a_groups_fixes() {
        let groups = group_bboxes([
            row("dev-a", DAY0_MORNING, 55.95, -3.19),
            row("dev-a", DAY0_EARLIER, 55.93, -3.22),
            row("dev-a", DAY0_MORNING, 55.97, -3.15),
        ]);

        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0].bbox,
            BBox {
                min_lat: 55.93,
                max_lat: 55.97,
                min_lon: -3.22,
                max_lon: -3.15,
            }
        );
    }

    /// A single fix yields a degenerate (point) box.
    #[test]
    fn single_fix_yields_a_point_box() {
        let groups = group_bboxes([row("dev-a", DAY0_MORNING, 55.95, -3.19)]);
        assert_eq!(
            groups[0].bbox,
            BBox {
                min_lat: 55.95,
                max_lat: 55.95,
                min_lon: -3.19,
                max_lon: -3.19,
            }
        );
    }

    /// Same device on different UTC days are distinct groups; the boxes don't merge.
    #[test]
    fn same_device_different_day_are_separate_groups() {
        let groups = group_bboxes([
            row("dev-a", DAY0_MORNING, 55.95, -3.19),
            row("dev-a", DAY1, 56.00, -3.10),
        ]);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].key.day + 1, groups[1].key.day);
        assert_eq!(groups[0].bbox.max_lat, 55.95);
        assert_eq!(groups[1].bbox.max_lat, 56.00);
    }

    /// Different devices on the same day are distinct groups.
    #[test]
    fn different_devices_same_day_are_separate_groups() {
        let groups = group_bboxes([
            row("dev-a", DAY0_MORNING, 55.95, -3.19),
            row("dev-b", DAY0_MORNING, 51.50, -0.12),
        ]);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].key.device_id, "dev-a");
        assert_eq!(groups[1].key.device_id, "dev-b");
    }

    /// Output is sorted by key regardless of the order fixes arrive in.
    #[test]
    fn groups_are_returned_sorted_by_key() {
        let groups = group_bboxes([
            row("dev-b", DAY1, 51.50, -0.12),
            row("dev-a", DAY0_MORNING, 55.95, -3.19),
            row("dev-b", DAY0_MORNING, 51.51, -0.13),
        ]);

        let keys: Vec<_> = groups.iter().map(|g| &g.key).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn no_rows_yields_no_groups() {
        assert!(group_bboxes([]).is_empty());
    }
}
