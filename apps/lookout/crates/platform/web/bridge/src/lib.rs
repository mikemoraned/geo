//! The core, as two functions over JSON strings.
//!
//! `BridgeWithSerializer` takes any serde serializer, so the shell speaks JSON and needs no
//! generated bindings to read it: an event is the serde tagging of [`platform_core::Event`],
//! and a view is that of [`web_core::ViewModel`]. Nothing here knows what either holds.
//!
//! Each exported function wraps one that answers a plain `Result<String, String>`, since a
//! `JsError` can only be built on wasm and the JSON is what the tests are about.

use std::sync::LazyLock;

use crux_core::{Core, bridge::BridgeWithSerializer};
use platform_core::Lookout;
use wasm_bindgen::prelude::*;
use web_core::Browser;

/// One core per wasm instance, and one instance per page: the shell loads the module once and
/// every element on the page shares it.
static BRIDGE: LazyLock<BridgeWithSerializer<Lookout<Browser>>> =
    LazyLock::new(|| BridgeWithSerializer::new(Core::new()));

/// Applies one event, answering the requests it raised as a JSON array. An event that moved
/// nothing answers `[]`, which is the shell's signal to leave the canvas alone.
///
/// # Errors
///
/// Returns an error where `event` is not an event this core knows.
#[wasm_bindgen]
pub fn process_event(event: &str) -> Result<String, JsError> {
    dispatch(event).map_err(|err| JsError::new(&err))
}

/// The current view, as JSON.
///
/// # Errors
///
/// Returns an error where the view could not be serialized.
#[wasm_bindgen]
pub fn view() -> Result<String, JsError> {
    projection().map_err(|err| JsError::new(&err))
}

fn dispatch(event: &str) -> Result<String, String> {
    let mut requests = Vec::new();
    let mut deserializer = serde_json::Deserializer::from_str(event);
    let mut serializer = serde_json::Serializer::new(&mut requests);
    BRIDGE
        .process_event(&mut deserializer, &mut serializer)
        .map_err(|err| err.to_string())?;
    Ok(String::from_utf8(requests).expect("serde_json writes utf-8"))
}

fn projection() -> Result<String, String> {
    let mut view = Vec::new();
    let mut serializer = serde_json::Serializer::new(&mut view);
    BRIDGE
        .view(&mut serializer)
        .map_err(|err| err.to_string())?;
    Ok(String::from_utf8(view).expect("serde_json writes utf-8"))
}

/// The JSON these tests assert is the shell's whole contract, so a change to an event or to
/// the view fails here rather than in a browser.
#[cfg(test)]
mod tests {
    use super::*;

    /// One core is shared by the whole process, so the sequence runs as a single test, in
    /// order: the page starts, is asked for crossings, answers, and starts again.
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

        // Starting again drops them and asks afresh, which is how a kiosk begins a journey
        // that happened before the one it just showed.
        assert!(
            dispatch(r#""Reset""#)
                .unwrap()
                .contains(r#"{"Crossings":null}"#)
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
