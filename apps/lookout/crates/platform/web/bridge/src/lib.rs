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
    Ok(String::from_utf8(requests).expect("JSON is utf-8"))
}

fn projection() -> Result<String, String> {
    let mut view = Vec::new();
    BRIDGE.view(&mut view).map_err(|err| err.to_string())?;
    Ok(String::from_utf8(view).expect("JSON is utf-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_is_asked_for_crossings_and_answers_with_them() {
        assert_eq!(
            projection().unwrap(),
            r#"{"now":null,"crossings":0,"radius_metres":0.0,"here":null,"predicted":[]}"#
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
            r#"{"now":null,"crossings":2,"radius_metres":5000.0,"here":null,"predicted":[]}"#
        );

        assert!(
            dispatch(r#""Reset""#)
                .unwrap()
                .contains(r#"{"Crossings":null}"#),
            "starting again asks for crossings afresh, as a kiosk does between journeys"
        );
        assert_eq!(
            projection().unwrap(),
            r#"{"now":null,"crossings":0,"radius_metres":0.0,"here":null,"predicted":[]}"#
        );
    }

    #[test]
    fn an_unknown_event_is_refused() {
        assert!(dispatch(r#"{"Nudge":null}"#).is_err());
    }
}
