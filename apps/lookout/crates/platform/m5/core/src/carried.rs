use platform_core::pointset::{Aligned, PointSet, holds_points};

include!(concat!(env!("OUT_DIR"), "/carried.rs"));

const _: () = assert!(
    holds_points(&PACKED.0),
    "the carried crossings are not a point set this reader understands — repack them",
);

pub fn crossings() -> PointSet<'static> {
    PointSet::new(&PACKED.0).expect("a set checked where it is built into the binary")
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use domain::Sample;
    use predictor::{CrowFlies, DEFAULT_RADIUS_METRES, Event, Predict};

    use super::*;

    #[test]
    fn the_carried_crossings_are_not_empty() {
        assert!(!crossings().is_empty());
    }

    #[test]
    fn the_carried_bytes_are_aligned_for_casting() {
        assert_eq!(PACKED.0.as_ptr() as usize % 4, 0);
    }

    #[test]
    fn every_carried_crossing_is_somewhere_in_germany() {
        let crossings = crossings();

        for point in crossings.iter() {
            assert!(
                (47.0..=55.0).contains(&point.latitude) && (6.0..=15.1).contains(&point.longitude),
                "{point:?} is not in Germany",
            );
        }
    }

    const DRESDEN_HBF: (f64, f64) = (51.0403, 13.7322);
    const NEAREST_TO_DRESDEN: [(u32, f32); 5] = [
        (0x5c9f_65c9, 2335.0),
        (0x3a81_47c0, 2338.4),
        (0xb85f_3371, 2343.1),
        (0xf51f_7627, 2347.4),
        (0xfd00_6a89, 2351.6),
    ];
    const TOLERANCE_M: f32 = 1.0;

    fn at_dresden() -> CrowFlies<f32, PointSet<'static>> {
        let mut predictor = CrowFlies::new(crossings(), DEFAULT_RADIUS_METRES);
        let t = DateTime::<Utc>::from_timestamp_millis(1_785_098_609_000).expect("an instant");
        predictor
            .observe(Event::Sampled(
                Sample::at(t, DRESDEN_HBF.0, DRESDEN_HBF.1).expect("on the globe"),
            ))
            .expect("the first event");
        predictor
    }

    #[test]
    fn the_device_agrees_with_the_notebook_about_what_is_nearest() {
        let predictor = at_dresden();

        let ids: Vec<u32> = predictor
            .predictions()
            .iter()
            .take(NEAREST_TO_DRESDEN.len())
            .map(|prediction| prediction.crossing_compact_id.get())
            .collect();

        assert_eq!(
            ids,
            NEAREST_TO_DRESDEN.map(|(id, _)| id).to_vec(),
            "a different five crossings, or a different order",
        );
    }

    #[test]
    fn the_device_agrees_with_the_notebook_about_how_far() {
        let predictor = at_dresden();

        for (prediction, (id, metres)) in predictor.predictions().iter().zip(NEAREST_TO_DRESDEN) {
            assert_eq!(prediction.crossing_compact_id.get(), id);
            assert!(
                (prediction.metres - metres).abs() < TOLERANCE_M,
                "{:08x}: the device says {}m, the notebook says {metres}m",
                id,
                prediction.metres,
            );
        }
    }

    #[test]
    fn the_device_agrees_with_the_notebook_about_what_is_near() {
        let predictor = at_dresden();

        assert_eq!(predictor.predictions().len(), 20);
    }
}
