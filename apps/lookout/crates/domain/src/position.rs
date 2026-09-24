use geo_types::Point;

use crate::precision::Precision;

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum CoordinateError {
    #[error("latitude {0} outside -90..=90")]
    Latitude(f64),
    #[error("longitude {0} outside -180..=180")]
    Longitude(f64),
}

pub fn position<P: Precision>(
    latitude_degrees: f64,
    longitude_degrees: f64,
) -> Result<Point<P>, CoordinateError> {
    if !(-90.0..=90.0).contains(&latitude_degrees) {
        return Err(CoordinateError::Latitude(latitude_degrees));
    }
    if !(-180.0..=180.0).contains(&longitude_degrees) {
        return Err(CoordinateError::Longitude(longitude_degrees));
    }
    Ok(Point::new(
        degrees(longitude_degrees),
        degrees(latitude_degrees),
    ))
}

pub fn degrees<P: Precision>(value: f64) -> P {
    P::from_f64(value).expect("a degree fits in any float")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinates_outside_the_valid_range_are_rejected() {
        assert_eq!(
            position::<f64>(91.0, 8.5),
            Err(CoordinateError::Latitude(91.0))
        );
        assert_eq!(
            position::<f64>(50.5, -181.0),
            Err(CoordinateError::Longitude(-181.0))
        );
        assert!(position::<f64>(50.5, 8.5).is_ok());
    }

    #[test]
    fn a_position_holds_its_axes_the_way_round_georust_does() {
        let point = position::<f64>(50.5, 8.5).expect("on the globe");

        assert_eq!(point, Point::new(8.5, 50.5));
    }
}
