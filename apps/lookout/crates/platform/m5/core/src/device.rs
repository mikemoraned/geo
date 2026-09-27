use platform_core::pointset::PointSet;
use platform_core::{Model, Shell};

use crate::carried;
use crate::panel::{self, NEAREST_ON_SCREEN, NO_FIX_YET, NO_TIME_YET, ViewModel};

#[derive(Debug, Default, Clone, Copy)]
pub struct Device;

impl Shell for Device {
    type ViewModel = ViewModel;
    type Crossings = PointSet<'static>;

    fn carried() -> Option<Self::Crossings> {
        Some(carried::crossings())
    }

    fn received(_points: Vec<domain::CrossingCompact<f64>>) -> Option<Self::Crossings> {
        None
    }

    fn project(model: &Model<Self>) -> ViewModel {
        let now = model.now();
        let fix = model.fix();
        let predictions = model.predictions();

        ViewModel {
            clock: now
                .map(|now| now.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| NO_TIME_YET.to_string()),
            latitude: fix
                .map(|fix| format!("{:.5}", fix.latitude()))
                .unwrap_or_else(|| NO_FIX_YET.to_string()),
            longitude: fix
                .map(|fix| format!("{:.5}", fix.longitude()))
                .unwrap_or_default(),
            battery: model
                .charge()
                .map(panel::bars)
                .unwrap_or_default()
                .to_string(),
            quality: match (
                fix.and_then(|fix| fix.gps.satellites),
                fix.and_then(|fix| fix.gps.hdop),
            ) {
                (Some(satellites), Some(hdop)) => format!("{satellites}sat h{hdop:.1}"),
                (Some(satellites), None) => format!("{satellites}sat"),
                _ => String::new(),
            },
            within: match fix {
                None => String::new(),
                Some(_) => panel::within(predictions.len()),
            },
            nearest: predictions
                .iter()
                .take(NEAREST_ON_SCREEN)
                .map(|prediction| panel::line(prediction, now))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeDelta, Utc};
    use crux_core::Core;
    use predictor::fixtures::{Fix, RMC_VOID, captured};

    use super::*;
    use crate::panel::{CHARACTERS_PER_LINE, NEAREST_ON_SCREEN, NO_ARRIVAL};
    use platform_core::{Effect, Event, Lookout};

    fn at_the_station() -> Fix {
        Fix::at(20, 43, 29, 51.0403, 13.7322)
            .with_speed_knots(54.0)
            .with_course_degrees(79.94)
    }

    fn moved_on() -> Fix {
        Fix::at(20, 43, 30, 51.0404, 13.73235)
            .with_speed_knots(54.0)
            .with_course_degrees(79.94)
    }

    fn stopped() -> Fix {
        Fix::at(20, 48, 58, 51.0403, 13.7322)
    }

    fn fix_instant() -> DateTime<Utc> {
        at_the_station().t()
    }

    fn core() -> Core<Lookout<Device>> {
        Core::new()
    }

    fn fixed() -> Core<Lookout<Device>> {
        let core = core();
        core.process_event(Event::Sentence(at_the_station().rmc()));
        core.process_event(Event::Sentence(at_the_station().gga()));
        core
    }

    #[test]
    fn reports_no_fix_until_a_position_arrives() {
        let core = core();

        assert_eq!(core.view().latitude, NO_FIX_YET);
        assert_eq!(core.view().longitude, "");
        assert_eq!(core.view().clock, NO_TIME_YET);
    }

    #[test]
    fn a_sentence_carrying_a_position_produces_a_fix() {
        let core = fixed();

        assert_eq!(core.view().latitude, "51.04030");
        assert_eq!(core.view().longitude, "13.73220");
    }

    #[test]
    fn a_void_sentence_leaves_the_last_fix_alone() {
        let core = fixed();

        core.process_event(Event::Sentence(captured(RMC_VOID)));

        assert_eq!(core.view().latitude, "51.04030");
    }

    #[test]
    fn a_sentence_asks_the_shell_to_render() {
        let core = core();

        let effects = core.process_event(Event::Sentence(at_the_station().rmc()));

        assert!(matches!(effects.as_slice(), [Effect::Render(_)]));
    }

    #[test]
    fn a_sentence_that_changes_nothing_asks_for_no_render() {
        let core = fixed();

        let effects = core.process_event(Event::Sentence(at_the_station().rmc()));

        assert!(effects.is_empty());
    }

    #[test]
    fn a_battery_reading_that_fills_the_same_bars_asks_for_no_render() {
        let core = core();

        assert!(!core.process_event(Event::Battery(4_200)).is_empty());
        assert!(core.process_event(Event::Battery(4_190)).is_empty());
    }

    #[test]
    fn the_clock_shows_what_the_shell_last_reported() {
        let core = core();

        core.process_event(Event::Tick(fix_instant()));

        assert_eq!(core.view().clock, "20:43:29");
    }

    #[test]
    fn a_clock_behind_the_receiver_does_not_move_the_panel_back() {
        let core = fixed();
        let epoch = DateTime::<Utc>::from_timestamp_nanos(0);

        let effects = core.process_event(Event::Tick(epoch));

        assert_eq!(core.view().clock, "20:43:29");
        assert!(effects.is_empty());
    }

    #[test]
    fn the_fix_reports_how_good_it_is() {
        let core = fixed();

        assert_eq!(core.view().quality, "6sat h4.4");
    }

    #[test]
    fn nothing_is_said_about_crossings_until_there_is_a_fix() {
        let core = core();

        assert!(core.view().nearest.is_empty());
        assert_eq!(core.view().within, "");
    }

    #[test]
    fn a_fix_fills_the_screen_with_the_nearest_crossings() {
        let core = fixed();

        assert_eq!(core.view().nearest.len(), NEAREST_ON_SCREEN);
    }

    #[test]
    fn the_count_and_the_list_agree() {
        let core = fixed();
        let view = core.view();

        assert_eq!(view.within, "20 in 5km");
        assert!(view.nearest.len() <= 20);
    }

    #[test]
    fn a_line_says_how_far_and_how_long() {
        let core = fixed();

        assert_eq!(core.view().nearest[0], "2.3km 1:24");
    }

    #[test]
    fn a_countdown_shortens_as_time_passes() {
        let core = fixed();

        core.process_event(Event::Tick(fix_instant() + TimeDelta::seconds(30)));

        assert_eq!(core.view().nearest[0], "2.3km 0:54");
    }

    #[test]
    fn a_fix_that_is_not_moving_predicts_a_distance_and_no_time() {
        let core = core();

        core.process_event(Event::Sentence(stopped().rmc()));

        assert_eq!(core.view().nearest[0], format!("2.3km {NO_ARRIVAL}"));
    }

    #[test]
    fn moving_changes_what_is_predicted() {
        let core = fixed();
        let before = core.view().nearest;

        core.process_event(Event::Sentence(moved_on().rmc()));

        assert_ne!(core.view().nearest, before);
    }

    #[test]
    fn the_predictions_do_not_change_while_the_fix_does_not() {
        let core = fixed();
        let first = core.view().nearest;

        core.process_event(Event::Sentence(at_the_station().rmc()));

        assert_eq!(core.view().nearest, first);
    }

    #[test]
    fn the_battery_is_blank_until_it_is_measured() {
        let core = core();

        assert_eq!(core.view().battery, "");
    }

    #[test]
    fn a_measured_battery_fills_its_bars() {
        let core = core();

        core.process_event(Event::Battery(4_200));
        assert_eq!(core.view().battery, "[===]");

        core.process_event(Event::Battery(3_200));
        assert_eq!(core.view().battery, "[   ]");
    }

    #[test]
    fn every_line_fits_the_panel() {
        let core = fixed();
        core.process_event(Event::Tick(fix_instant()));
        core.process_event(Event::Battery(4_200));
        let view = core.view();

        for line in [
            &view.clock,
            &view.latitude,
            &view.longitude,
            &view.quality,
            &view.within,
        ]
        .into_iter()
        .chain(view.nearest.iter())
        {
            assert!(
                line.chars().count() <= CHARACTERS_PER_LINE,
                "{line:?} is {} characters, more than a line holds",
                line.chars().count(),
            );
        }
    }
}
