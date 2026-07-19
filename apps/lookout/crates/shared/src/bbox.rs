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

impl BBox {
    /// This box scaled about its centre by `factor`: each dimension's span is
    /// multiplied by `factor` with the centre held fixed (`factor == 2.0` doubles it,
    /// `1.0` is the identity). For `factor >= 1.0` the result always contains `self`.
    pub fn scaled_about_centre(&self, factor: f64) -> BBox {
        let grow_lat = (self.max_lat - self.min_lat) / 2.0 * (factor - 1.0);
        let grow_lon = (self.max_lon - self.min_lon) / 2.0 * (factor - 1.0);
        BBox {
            min_lat: self.min_lat - grow_lat,
            max_lat: self.max_lat + grow_lat,
            min_lon: self.min_lon - grow_lon,
            max_lon: self.max_lon + grow_lon,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: BBox = BBox {
        min_lat: 50.0,
        max_lat: 52.0,
        min_lon: 8.0,
        max_lon: 12.0,
    };

    #[test]
    fn scaling_by_one_is_the_identity() {
        assert_eq!(BOX.scaled_about_centre(1.0), BOX);
    }

    #[test]
    fn doubling_extends_each_side_by_half_the_span() {
        // lat span 2 (centre 51) → 49..53; lon span 4 (centre 10) → 6..14.
        assert_eq!(
            BOX.scaled_about_centre(2.0),
            BBox {
                min_lat: 49.0,
                max_lat: 53.0,
                min_lon: 6.0,
                max_lon: 14.0,
            }
        );
    }

    #[test]
    fn scaling_a_point_box_leaves_it_a_point() {
        let point = BBox {
            min_lat: 50.0,
            max_lat: 50.0,
            min_lon: 8.0,
            max_lon: 8.0,
        };
        assert_eq!(point.scaled_about_centre(2.0), point);
    }
}
