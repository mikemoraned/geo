//! The core, as two functions over JSON strings.
//!
//! `BridgeWithSerializer` takes any serde serializer, so the shell speaks JSON and needs no
//! generated bindings to read it: an event is the serde tagging of [`web_core::Event`], and a
//! view is that of [`web_core::ViewModel`]. Nothing here knows what either holds.
//!
//! Each exported function wraps one that answers a plain `Result<String, String>`, since a
//! `JsError` can only be built on wasm and the JSON is what the tests are about.

use std::sync::LazyLock;

use crux_core::{Core, bridge::BridgeWithSerializer};
use wasm_bindgen::prelude::*;
use web_core::Counter;

/// One core per wasm instance, and one instance per page: the shell loads the module once and
/// every element on the page shares it.
static BRIDGE: LazyLock<BridgeWithSerializer<Counter>> =
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

    /// One core is shared by the whole process, so the round trip runs as a single test, in
    /// order.
    #[test]
    fn a_tick_renders_and_moves_the_count() {
        assert_eq!(projection().unwrap(), r#"{"count":"0"}"#);

        let requests = dispatch(r#"{"Tick":"2026-09-19T12:00:00Z"}"#).unwrap();

        assert_eq!(requests, r#"[{"id":0,"effect":{"Render":null}}]"#);
        assert_eq!(projection().unwrap(), r#"{"count":"1"}"#);

        // The same tick again: no request at all, which is what tells the shell not to repaint.
        assert_eq!(
            dispatch(r#"{"Tick":"2026-09-19T12:00:00Z"}"#).unwrap(),
            "[]"
        );
    }

    #[test]
    fn an_unknown_event_is_refused() {
        assert!(dispatch(r#"{"Nudge":null}"#).is_err());
    }
}
