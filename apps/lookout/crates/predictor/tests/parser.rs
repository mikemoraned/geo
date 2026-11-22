#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test reports failure by panicking"
)]

use chrono::{TimeZone, Utc};

use predictor::parser::Parser;
use test_predictor::{Fix, GGA_NO_FIX, GSA_NO_FIX, RMC_VOID, SPLICED, captured, with_bad_checksum};

fn fix() -> Fix {
    Fix::at(20, 43, 29, 50.5, 8.5)
        .with_speed_knots(4.13)
        .with_course_degrees(79.94)
}

fn later() -> Fix {
    Fix::at(20, 43, 30, 50.5001, 8.50015)
        .with_speed_knots(4.13)
        .with_course_degrees(79.94)
}

fn assert_near(got: Option<f64>, want: f64) {
    let got = got.expect("a value");
    assert!((got - want).abs() < 1e-5, "{got} is not near {want}");
}

fn parser() -> Parser<f64> {
    Parser::new()
}

fn fixed() -> Parser<f64> {
    let mut parser = parser();
    parser.absorb(&fix().rmc()).expect("a first sample");
    parser
}

#[test]
fn an_rmc_sentence_makes_a_sample() {
    let sample = parser().absorb(&fix().rmc()).expect("a sample");

    assert_eq!(sample.latitude(), 50.5);
    assert_eq!(sample.longitude(), 8.5);
    assert_eq!(sample.t, fix().t());
}

#[test]
fn a_speed_in_knots_becomes_metres_per_second() {
    let sample = parser().absorb(&fix().rmc()).expect("a sample");

    assert_near(sample.gps.speed_mps, 4.13 * 1_852.0 / 3_600.0);
    assert_near(sample.gps.heading_degrees, 79.94);
}

#[test]
fn a_gga_sentence_alone_makes_no_sample() {
    assert_eq!(parser().absorb(&fix().gga()), None);
}

#[test]
fn a_gga_after_an_rmc_makes_a_sample_reporting_the_fix_quality() {
    let sample = fixed().absorb(&fix().gga()).expect("a sample");

    assert_eq!(sample.gps.satellites, Some(6));
    assert_near(sample.gps.hdop, 4.4);
    assert_near(sample.gps.altitude_metres, 262.46);
}

#[test]
fn a_later_sentence_moves_the_sample() {
    let sample = fixed().absorb(&later().gga()).expect("a sample");

    assert_eq!(sample.latitude(), 50.5001);
    assert_eq!(
        sample.t,
        Utc.with_ymd_and_hms(2026, 7, 29, 20, 43, 30).unwrap()
    );
}

#[test]
fn a_stationary_rmc_with_no_course_still_makes_a_sample() {
    let stationary = Fix::at(20, 48, 58, 50.5, 8.5).with_speed_knots(0.08);

    let sample = parser().absorb(&stationary.rmc()).expect("a sample");

    assert_eq!(sample.latitude(), 50.5);
    assert_eq!(sample.gps.heading_degrees, None);
}

#[test]
fn a_void_sentence_makes_no_sample() {
    assert_eq!(fixed().absorb(&captured(RMC_VOID)), None);
}

#[test]
fn the_real_indoor_stream_makes_no_samples() {
    let mut parser = parser();

    for sentence in [RMC_VOID, GGA_NO_FIX, GSA_NO_FIX, SPLICED].map(captured) {
        assert_eq!(parser.absorb(&sentence), None, "{sentence}");
    }
}

#[test]
fn a_spliced_sentence_makes_no_sample() {
    assert_eq!(fixed().absorb(&captured(SPLICED)), None);
}

#[test]
fn a_corrupt_sentence_makes_no_sample() {
    assert_eq!(fixed().absorb(&with_bad_checksum(&later().gga())), None);
}

#[test]
fn a_sentence_adding_nothing_makes_no_sample() {
    let mut parser = fixed();

    assert!(parser.absorb(&fix().gga()).is_some());
    assert_eq!(parser.absorb(&fix().gga()), None);
}
