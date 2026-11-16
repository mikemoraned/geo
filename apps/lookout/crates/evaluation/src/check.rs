use std::collections::{BTreeMap, BTreeSet};

use countries::{CountryAreas, CountryError};
use crossings::silver::Crossing;
use domain::{CrossingCompactId, SessionId};
use geo_types::{Geometry, Point, Polygon};
use medallion::{Country, Root};
use medallion_model::{EvaluationSplitRowV1, TrainSessionSamplePositionV3};

use crate::a5::{A5CellId, A5Cover, NoA5Cell, NoA5Cover};
use crate::offline::PassesBySession;
use crate::score::Outcome;
use crate::scored::ScoredCrossings;
use crate::split::{ScoredSplit, Split};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitCheck {
    pub split: ScoredSplit,
    pub cover: A5Cover,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SplitCounts {
    pub cells: usize,
    pub cells_in_split: usize,
    pub crossings: usize,
    pub crossings_in_split: usize,
    pub passes_in_split: usize,
    pub passes_outside: usize,
    pub cells_outside_cover: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutcomeCounts {
    pub outcomes_in_split: usize,
    pub outcomes_outside: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum SplitCheckError {
    #[error("reading the country's areas: {0}")]
    Country(#[from] CountryError),
    #[error("the store holds no extract of {0}, so the split cannot be checked against its areas")]
    NoCountry(Country),
    #[error("an area of {0} is not a polygon")]
    NotAnArea(Country),
    #[error(transparent)]
    Cover(#[from] NoA5Cover),
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SplitBroken {
    #[error("the split is not disjoint: {cells} cells hold crossings of both tune and test")]
    NotDisjoint { cells: usize },
    #[error(
        "the split is not complete: of {all} {units}, {in_split} lie in the split and {outside} \
         outside it"
    )]
    Incomplete {
        units: &'static str,
        all: usize,
        in_split: usize,
        outside: usize,
    },
    #[error("{crossings} crossings take a split other than their cell's")]
    NotTheCellsSplit { crossings: usize },
    #[error("{cells} cells holding a crossing or a sample lie outside the country's cover")]
    OutsideTheCountry { cells: usize },
    #[error(transparent)]
    Cell(#[from] NoA5Cell),
}

impl SplitCheck {
    pub async fn read(
        root: &Root,
        country: Country,
        split: &ScoredSplit,
    ) -> Result<Self, SplitCheckError> {
        let areas = CountryAreas::newest_per_country(root)
            .await?
            .shapes_of(country);
        if areas.is_empty() {
            return Err(SplitCheckError::NoCountry(country));
        }
        let polygons = areas
            .into_iter()
            .map(|area| match area {
                Geometry::Polygon(polygon) => Ok(vec![polygon]),
                Geometry::MultiPolygon(polygons) => Ok(polygons.0),
                _ => Err(SplitCheckError::NotAnArea(country)),
            })
            .collect::<Result<Vec<Vec<Polygon<f64>>>, _>>()?;
        Ok(Self {
            split: split.clone(),
            cover: A5Cover::overlapping(polygons.iter().flatten(), split.splitter.resolution)?,
        })
    }

    pub fn before_replay(
        &self,
        crossings: &[Crossing],
        passes: &PassesBySession,
        samples: &BTreeMap<SessionId, Vec<TrainSessionSamplePositionV3>>,
    ) -> Result<SplitCounts, SplitBroken> {
        let splitter = &self.split.splitter;
        let cells: BTreeMap<CrossingCompactId, A5CellId> = crossings
            .iter()
            .map(|crossing| {
                Ok((
                    crossing.compact_id,
                    splitter.cell(crossing.crossing.position)?,
                ))
            })
            .collect::<Result<_, NoA5Cell>>()?;
        let (in_split, outside) = self.both_splits(crossings)?;
        let cells_of = |scored: &ScoredCrossings| -> BTreeSet<A5CellId> {
            cells
                .iter()
                .filter(|(crossing, _)| scored.contains(**crossing))
                .map(|(_, cell)| *cell)
                .collect()
        };
        let (cells_in_split, cells_outside) = (cells_of(&in_split), cells_of(&outside));
        disjoint(&cells_in_split, &cells_outside)?;

        let not_the_cells_split = cells
            .iter()
            .filter(|(crossing, cell)| {
                in_split.contains(**crossing) != (splitter.split(**cell) == self.split.split)
            })
            .count();
        if not_the_cells_split > 0 {
            return Err(SplitBroken::NotTheCellsSplit {
                crossings: not_the_cells_split,
            });
        }

        let all_passes: Vec<CrossingCompactId> = passes
            .values()
            .flatten()
            .map(|(_, crossing)| *crossing)
            .collect();
        let passes_in_split = in_split
            .keep(all_passes.iter(), |crossing| **crossing)
            .len();
        let passes_outside = outside.keep(all_passes.iter(), |crossing| **crossing).len();
        complete("passes", all_passes.len(), passes_in_split, passes_outside)?;

        let sample_cells = samples
            .values()
            .flatten()
            .map(|sample| splitter.cell(Point::new(sample.lon, sample.lat)))
            .collect::<Result<BTreeSet<_>, _>>()?;
        within(
            &cells.values().copied().chain(sample_cells).collect(),
            &self.cover,
        )?;

        Ok(SplitCounts {
            cells: cells.values().collect::<BTreeSet<_>>().len(),
            cells_in_split: cells_in_split.len(),
            crossings: crossings.len(),
            crossings_in_split: crossings
                .iter()
                .filter(|crossing| in_split.contains(crossing.compact_id))
                .count(),
            passes_in_split,
            passes_outside,
            cells_outside_cover: 0,
        })
    }

    pub fn of_outcomes(
        &self,
        crossings: &[Crossing],
        outcomes: &BTreeMap<SessionId, Vec<Outcome>>,
    ) -> Result<OutcomeCounts, SplitBroken> {
        let (in_split, outside) = self.both_splits(crossings)?;
        let all: Vec<&Outcome> = outcomes.values().flatten().collect();
        let outcomes_in_split = in_split
            .keep(all.iter(), |outcome| outcome.crossing_compact_id())
            .len();
        let outcomes_outside = outside
            .keep(all.iter(), |outcome| outcome.crossing_compact_id())
            .len();
        complete("outcomes", all.len(), outcomes_in_split, outcomes_outside)?;
        Ok(OutcomeCounts {
            outcomes_in_split,
            outcomes_outside,
        })
    }

    fn both_splits(
        &self,
        crossings: &[Crossing],
    ) -> Result<(ScoredCrossings, ScoredCrossings), NoA5Cell> {
        let other = ScoredSplit {
            split: match self.split.split {
                Split::Tune => Split::Test,
                Split::Test => Split::Tune,
            },
            ..self.split.clone()
        };
        Ok((
            ScoredCrossings::of(crossings, Some(&self.split))?,
            ScoredCrossings::of(crossings, Some(&other))?,
        ))
    }
}

impl SplitCounts {
    pub fn row(&self, outcomes: OutcomeCounts) -> EvaluationSplitRowV1 {
        EvaluationSplitRowV1 {
            cells: self.cells as u64,
            cells_in_split: self.cells_in_split as u64,
            crossings: self.crossings as u64,
            crossings_in_split: self.crossings_in_split as u64,
            passes_in_split: self.passes_in_split as u64,
            passes_outside: self.passes_outside as u64,
            outcomes_in_split: outcomes.outcomes_in_split as u64,
            outcomes_outside: outcomes.outcomes_outside as u64,
            cells_outside_cover: self.cells_outside_cover as u64,
        }
    }
}

fn disjoint(
    in_split: &BTreeSet<A5CellId>,
    outside: &BTreeSet<A5CellId>,
) -> Result<(), SplitBroken> {
    let shared = in_split.intersection(outside).count();
    if shared == 0 {
        Ok(())
    } else {
        Err(SplitBroken::NotDisjoint { cells: shared })
    }
}

fn complete(
    units: &'static str,
    all: usize,
    in_split: usize,
    outside: usize,
) -> Result<(), SplitBroken> {
    if in_split + outside == all {
        Ok(())
    } else {
        Err(SplitBroken::Incomplete {
            units,
            all,
            in_split,
            outside,
        })
    }
}

fn within(cells: &BTreeSet<A5CellId>, cover: &A5Cover) -> Result<(), SplitBroken> {
    let outside: Vec<A5CellId> = cells
        .iter()
        .copied()
        .filter(|cell| !cover.contains(*cell))
        .collect();
    for cell in &outside {
        match cell.centre() {
            Ok(centre) => tracing::warn!(
                %cell,
                lon = centre.x(),
                lat = centre.y(),
                "a cell lies outside the country's cover"
            ),
            Err(err) => tracing::warn!(%err, "a cell lies outside the country's cover"),
        }
    }
    let outside = outside.len();
    if outside == 0 {
        Ok(())
    } else {
        Err(SplitBroken::OutsideTheCountry { cells: outside })
    }
}

#[cfg(test)]
mod tests {
    use geo_types::{Polygon, polygon};

    use super::*;
    use crate::a5::A5Resolution;
    use crate::fixtures::{crossing, instant, session};
    use crate::split::{Splitter, TuneBuckets};

    const DRESDEN_HBF: (f64, f64) = (13.7322, 51.0403);
    const SOUTH_OF_DRESDEN: (f64, f64) = (13.7322, 50.7403);

    fn resolution() -> A5Resolution {
        A5Resolution::new(8).unwrap()
    }

    fn check(split: Split, half_width_degrees: f64) -> SplitCheck {
        let (lon, lat) = DRESDEN_HBF;
        let half = half_width_degrees;
        let around: Polygon<f64> = polygon![
            (x: lon - half, y: lat - half),
            (x: lon + half, y: lat - half),
            (x: lon + half, y: lat + half),
            (x: lon - half, y: lat + half),
        ];
        SplitCheck {
            split: ScoredSplit {
                splitter: Splitter {
                    resolution: resolution(),
                    salt: "tune-test-1".parse().unwrap(),
                    tune_buckets: TuneBuckets::new(70).unwrap(),
                },
                split,
            },
            cover: A5Cover::overlapping([&around], resolution()).unwrap(),
        }
    }

    fn crossings() -> Vec<Crossing> {
        [DRESDEN_HBF, SOUTH_OF_DRESDEN]
            .iter()
            .zip(0..)
            .map(|(&(lon, lat), n)| Crossing {
                crossing: domain::Crossing::new(crossing(n), Point::new(lon, lat)),
                compact_id: CrossingCompactId::new(n),
                extract_id: "extract".to_string(),
            })
            .collect()
    }

    fn passes() -> PassesBySession {
        BTreeMap::from([(
            session(0),
            vec![
                (instant(60), CrossingCompactId::new(0)),
                (instant(120), CrossingCompactId::new(0)),
                (instant(180), CrossingCompactId::new(1)),
            ],
        )])
    }

    fn samples() -> BTreeMap<SessionId, Vec<TrainSessionSamplePositionV3>> {
        let (lon, lat) = DRESDEN_HBF;
        BTreeMap::from([(
            session(0),
            vec![TrainSessionSamplePositionV3 {
                session_id: session(0),
                t: instant(0),
                seq: 0,
                lat,
                lon,
            }],
        )])
    }

    fn missed(crossing: u32) -> Outcome {
        Outcome::Miss {
            crossing_compact_id: CrossingCompactId::new(crossing),
            crossed_at: instant(0),
        }
    }

    fn cell(n: u64) -> A5CellId {
        A5CellId(n)
    }

    #[test]
    fn a_test_run_counts_dresden_hbf_in_its_split_and_the_crossing_south_of_it_outside() {
        assert_eq!(
            check(Split::Test, 1.0).before_replay(&crossings(), &passes(), &samples()),
            Ok(SplitCounts {
                cells: 2,
                cells_in_split: 1,
                crossings: 2,
                crossings_in_split: 1,
                passes_in_split: 2,
                passes_outside: 1,
                cells_outside_cover: 0,
            })
        );
    }

    #[test]
    fn a_tune_run_counts_the_crossing_south_of_dresden_hbf_in_its_split() {
        assert_eq!(
            check(Split::Tune, 1.0).before_replay(&crossings(), &passes(), &samples()),
            Ok(SplitCounts {
                cells: 2,
                cells_in_split: 1,
                crossings: 2,
                crossings_in_split: 1,
                passes_in_split: 1,
                passes_outside: 2,
                cells_outside_cover: 0,
            })
        );
    }

    #[test]
    fn a_crossing_outside_the_countrys_cover_refuses_the_run() {
        assert_eq!(
            check(Split::Test, 0.1).before_replay(&crossings(), &passes(), &samples()),
            Err(SplitBroken::OutsideTheCountry { cells: 1 })
        );
    }

    #[test]
    fn a_test_run_counts_each_outcome_by_its_crossings_split() {
        let outcomes = BTreeMap::from([(session(0), vec![missed(0), missed(1), missed(1)])]);

        assert_eq!(
            check(Split::Test, 1.0).of_outcomes(&crossings(), &outcomes),
            Ok(OutcomeCounts {
                outcomes_in_split: 1,
                outcomes_outside: 2,
            })
        );
    }

    #[test]
    fn a_cell_holding_crossings_of_both_splits_breaks_the_split() {
        assert_eq!(
            disjoint(
                &BTreeSet::from([cell(1), cell(2)]),
                &BTreeSet::from([cell(2), cell(3)])
            ),
            Err(SplitBroken::NotDisjoint { cells: 1 })
        );
    }

    #[test]
    fn units_that_fall_in_neither_split_break_the_split() {
        assert_eq!(
            complete("passes", 5, 2, 2),
            Err(SplitBroken::Incomplete {
                units: "passes",
                all: 5,
                in_split: 2,
                outside: 2,
            })
        );
    }

    #[test]
    fn units_that_fall_in_one_split_each_complete_the_split() {
        assert_eq!(complete("passes", 5, 2, 3), Ok(()));
    }

    #[test]
    fn a_cell_outside_the_cover_is_counted() {
        let cover = check(Split::Test, 1.0).cover;
        let berlin_hbf = A5CellId::containing(Point::new(13.3695, 52.5251), resolution()).unwrap();

        assert_eq!(
            within(&BTreeSet::from([berlin_hbf]), &cover),
            Err(SplitBroken::OutsideTheCountry { cells: 1 })
        );
    }
}
