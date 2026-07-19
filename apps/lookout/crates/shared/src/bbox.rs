//! An axis-aligned lat/lon bounding box — a pure spatial data type shared by the
//! crates that derive or query spatial extents.

/// An axis-aligned lat/lon bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lon: f64,
    pub max_lon: f64,
}
