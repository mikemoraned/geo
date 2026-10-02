use prose_gate::{missing, skills_invoked};
use serde_json::{Value, json};

const WRITING: &str = "softaworks-agent-toolkit-writing-clearly-and-concisely";
const TECHNICAL: &str = "technical-writing:technical-writing";

fn required() -> Vec<String> {
    vec![WRITING.to_string(), TECHNICAL.to_string()]
}

fn line(value: Value) -> String {
    value.to_string()
}

/// The shape a transcript records a call in, nested as Claude Code writes it.
fn invocation(skill: &str) -> String {
    line(json!({
        "type": "assistant",
        "message": {
            "role": "assistant",
            "content": [{
                "type": "tool_use",
                "id": "toolu_01",
                "name": "Skill",
                "input": { "skill": skill },
                "caller": { "type": "direct" }
            }]
        }
    }))
}

#[test]
fn an_invocation_is_found_however_deeply_it_nests() {
    let found = skills_invoked(&invocation(WRITING));
    assert_eq!(
        found.iter().map(String::as_str).collect::<Vec<_>>(),
        [WRITING]
    );
}

#[test]
fn every_invocation_of_a_session_is_found() {
    let transcript = format!("{}\n{}\n", invocation(WRITING), invocation(TECHNICAL));
    assert!(missing(&required(), &skills_invoked(&transcript)).is_empty());
}

#[test]
fn a_skill_never_invoked_is_reported_alone() {
    let found = skills_invoked(&invocation(WRITING));
    assert_eq!(missing(&required(), &found), [TECHNICAL]);
}

#[test]
fn the_definition_of_the_skill_tool_is_not_an_invocation() {
    let definition = line(json!({
        "type": "attachment",
        "attachment": {
            "tools": [{
                "name": "Skill",
                "description": "Invoke a skill.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "skill": { "type": "string", "description": "The name of a skill" }
                    }
                }
            }]
        }
    }));
    assert!(skills_invoked(&definition).is_empty());
}

#[test]
fn naming_a_skill_in_prose_is_not_an_invocation() {
    let read_of_claude_md = line(json!({
        "type": "user",
        "message": {
            "content": [{
                "type": "tool_result",
                "content": format!("- `{WRITING}` carries the general principles")
            }]
        }
    }));
    assert!(skills_invoked(&read_of_claude_md).is_empty());
}

#[test]
fn an_unparseable_line_leaves_the_rest_readable() {
    let transcript = format!("not json\n{}\n{{\"type\":\"assis", invocation(TECHNICAL));
    assert_eq!(
        skills_invoked(&transcript)
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [TECHNICAL]
    );
}

#[test]
fn an_empty_transcript_records_no_invocation() {
    assert!(skills_invoked("").is_empty());
}

#[test]
fn a_tool_that_merely_mentions_a_skill_is_not_an_invocation() {
    let bash = line(json!({
        "type": "tool_use",
        "name": "Bash",
        "input": { "command": format!("echo {WRITING}") }
    }));
    assert!(skills_invoked(&bash).is_empty());
}
