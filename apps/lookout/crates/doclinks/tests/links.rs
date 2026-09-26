use doclinks::{app_root, broken, markdown_under};

#[test]
fn every_link_between_the_docs_resolves() {
    let root = app_root();
    let markdown = markdown_under(&root);

    assert!(
        markdown.len() > 20,
        "found only {} markdown files under {}",
        markdown.len(),
        root.display()
    );

    let broken = broken(&markdown);

    assert!(
        broken.is_empty(),
        "{} broken link(s):\n{}",
        broken.len(),
        broken
            .iter()
            .map(|broken| format!("  {broken}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
