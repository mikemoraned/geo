use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct StopPayload {
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub stop_hook_active: bool,
}

impl StopPayload {
    pub fn block_already_fired(&self) -> bool {
        self.stop_hook_active
    }
}

pub fn skills_invoked(transcript: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for line in transcript.lines() {
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            collect_invocations(&value, &mut found);
        }
    }
    found
}

fn collect_invocations(value: &Value, found: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if map.get("type") == Some(&Value::String("tool_use".into()))
                && map.get("name") == Some(&Value::String("Skill".into()))
                && let Some(Value::String(skill)) =
                    map.get("input").and_then(|input| input.get("skill"))
            {
                found.insert(skill.clone());
            }
            for nested in map.values() {
                collect_invocations(nested, found);
            }
        }
        Value::Array(items) => {
            for nested in items {
                collect_invocations(nested, found);
            }
        }
        _ => {}
    }
}

pub fn missing<'a>(required: &'a [String], invoked: &BTreeSet<String>) -> Vec<&'a str> {
    required
        .iter()
        .filter(|skill| !invoked.contains(*skill))
        .map(String::as_str)
        .collect()
}

pub fn block_reason(missing: &[&str], changed: &[String]) -> String {
    let list = |lines: &[&str]| {
        lines
            .iter()
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let changed: Vec<&str> = changed.iter().map(String::as_str).collect();
    format!(
        "Prose changed in this session, and these skills were never invoked:\n{}\n\n\
         Changed:\n{}\n\n\
         CLAUDE.md requires each of them before a first draft. Invoke what is missing with the \
         Skill tool and take the prose through its rules, or say plainly that the change carries \
         no prose — a ticked checkbox, a rename, a path.",
        list(missing),
        list(&changed),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_first_stop_of_a_turn_has_fired_no_block() {
        let payload: StopPayload =
            serde_json::from_str(r#"{"stop_hook_active": false}"#).expect("a payload");
        assert!(!payload.block_already_fired());
    }

    #[test]
    fn a_stop_that_follows_a_block_has_fired_one() {
        let payload: StopPayload =
            serde_json::from_str(r#"{"stop_hook_active": true}"#).expect("a payload");
        assert!(payload.block_already_fired());
    }

    #[test]
    fn a_payload_that_omits_the_field_has_fired_no_block() {
        let payload: StopPayload = serde_json::from_str("{}").expect("a payload");
        assert!(!payload.block_already_fired());
    }
}
