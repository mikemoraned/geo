//! The per-`(device_id, UTC day)` grouping of GPS fixes and its bounding box — the
//! spatial extent a later step queries Overture transport data against. The grouping
//! is done in SQL by [`crate::archive::Archive::groups`]; this module is just the
//! result shape.

/// A group's identity: one bounding box is derived per device per UTC day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupKey {
    pub device_id: String,
    /// UTC day as whole days since the Unix epoch.
    pub day: i64,
}

/// An axis-aligned lat/lon bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lon: f64,
    pub max_lon: f64,
}

/// A group and the bounding box of its fixes.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub key: GroupKey,
    pub bbox: BBox,
}
