use std::sync::LazyLock;

use crux_core::{
    Core,
    bridge::{Bridge, JsonFfiFormat},
};
use platform_core::Lookout;
use wasm_bindgen::prelude::*;
use web_core::Browser;

static BRIDGE: LazyLock<Bridge<Lookout<Browser>, JsonFfiFormat>> =
    LazyLock::new(|| Bridge::new(Core::new()));

#[wasm_bindgen]
pub fn process_event(event: &str) -> Result<String, JsError> {
    dispatch(event).map_err(|err| JsError::new(&err))
}

#[wasm_bindgen]
pub fn view() -> Result<String, JsError> {
    projection().map_err(|err| JsError::new(&err))
}

fn dispatch(event: &str) -> Result<String, String> {
    let mut requests = Vec::new();
    BRIDGE
        .update(event.as_bytes(), &mut requests)
        .map_err(|err| err.to_string())?;
    #[allow(clippy::unwrap_used, reason = "the bridge writes JSON, which is UTF-8")]
    Ok(String::from_utf8(requests).unwrap())
}

fn projection() -> Result<String, String> {
    let mut view = Vec::new();
    BRIDGE.view(&mut view).map_err(|err| err.to_string())?;
    #[allow(clippy::unwrap_used, reason = "the bridge writes JSON, which is UTF-8")]
    Ok(String::from_utf8(view).unwrap())
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    const RAIL: &str = "AQIAAAACAAAA9ihcj8J1K0CF61G4HoVJQHsUrkfheitAZmZmZmaGSUA=";

    fn assert_near(actual: &Value, expected: f64) {
        let actual = actual.as_f64().expect("a number");
        assert!(
            (actual - expected).abs() < 1e-3,
            "{actual} is not {expected}"
        );
    }

    #[test]
    fn a_page_is_asked_for_crossings_and_lines_and_answers_with_them() {
        assert_eq!(
            projection().unwrap(),
            r#"{"now":null,"crossings":0,"radius_metres":0.0,"pass_radius_metres":0.0,"here":null,"predicted":[],"lines":[],"trace_unwritten":null}"#
        );

        let requests = dispatch(r#""Reset""#).unwrap();
        assert_eq!(
            requests,
            r#"[{"id":0,"effect":{"Render":null}},{"id":1,"effect":{"Crossings":null}}]"#
        );

        let answered = dispatch(r#"{"Crossings":[[7,51.0403,13.7322],[8,51.0503,13.7322]]}"#);

        assert_eq!(answered.unwrap(), r#"[{"id":2,"effect":{"Render":null}}]"#);
        assert_eq!(
            projection().unwrap(),
            r#"{"now":null,"crossings":2,"radius_metres":5000.0,"pass_radius_metres":250.0,"here":null,"predicted":[],"lines":[],"trace_unwritten":null}"#
        );

        assert!(
            dispatch(r#""Reset""#)
                .unwrap()
                .contains(r#"{"Crossings":null}"#),
            "starting again asks for crossings afresh, as a kiosk does between journeys"
        );
        assert_eq!(
            projection().unwrap(),
            r#"{"now":null,"crossings":0,"radius_metres":0.0,"pass_radius_metres":0.0,"here":null,"predicted":[],"lines":[],"trace_unwritten":null}"#
        );

        dispatch(r#"{"Crossings":[[7,51.0403,13.7322]]}"#).unwrap();
        let requests: Value = serde_json::from_str(
            &dispatch(r#"{"Position":{"t":"2026-07-26T20:43:29Z","gps":{"lat":51.0403,"lon":13.7322,"alt":null,"acc":5.0,"speed":27.8,"heading":null}}}"#)
                .unwrap(),
        )
        .unwrap();
        let request = requests
            .as_array()
            .unwrap()
            .iter()
            .find_map(|request| request["effect"].get("Lines"))
            .expect("a fix asks for lines");
        let area = &request["area"];
        assert_near(&area["xmin"], 13.5177);
        assert_near(&area["ymin"], 50.9054);
        assert_near(&area["xmax"], 13.9467);
        assert_near(&area["ymax"], 51.1752);

        let id = &request["id"];
        let answered = dispatch(&format!(r#"{{"Lines":{{"id":{id},"lines":["{RAIL}"]}}}}"#));

        assert!(answered.unwrap().contains(r#"{"Render":null}"#));
        assert!(
            projection().unwrap().ends_with(&format!(
                r#""lines":[{{"category":"rail","wkb":"{RAIL}"}}],"trace_unwritten":null}}"#
            )),
            "the rail answering the request is drawn"
        );

        dispatch(r#"{"Position":{"t":"2026-07-26T20:44:05Z","gps":{"lat":51.0493,"lon":13.7322,"alt":null,"acc":5.0,"speed":27.8,"heading":null}}}"#).unwrap();

        let view: Value = serde_json::from_str(&projection().unwrap()).unwrap();
        let categories: Vec<&str> = view["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| line["category"].as_str().unwrap())
            .collect();
        assert_eq!(categories, vec!["rail", "trace"]);
    }

    #[test]
    fn an_unknown_event_is_refused() {
        assert!(dispatch(r#"{"Nudge":null}"#).is_err());
    }
}
