use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use walkdir::WalkDir;

const WORKSPACE_MANIFEST: &str = "Cargo.toml";
const WORKSPACE_SECTION: &str = "[workspace]";
const GENERATED: [&str; 2] = ["target", "node_modules"];

#[test]
fn every_link_between_the_docs_resolves() {
    let root = app_root();
    let docs = markdown_under(&root);
    assert!(
        docs.len() > 20,
        "found only {} markdown files under {}",
        docs.len(),
        root.display()
    );

    let headings: HashMap<PathBuf, Vec<String>> = docs
        .iter()
        .map(|doc| (canonical(doc), headings(&read(doc))))
        .collect();
    let broken: Vec<String> = docs
        .iter()
        .flat_map(|from| broken_links_in(from, &headings))
        .collect();

    assert!(
        broken.is_empty(),
        "{} broken link(s):\n  {}",
        broken.len(),
        broken.join("\n  ")
    );
}

fn broken_links_in(from: &Path, headings: &HashMap<PathBuf, Vec<String>>) -> Vec<String> {
    let here = from.parent().unwrap_or(Path::new("."));

    local_links(&read(from))
        .into_iter()
        .filter_map(|target| {
            let (path, heading) = match target.split_once('#') {
                Some((path, heading)) => (path, Some(heading)),
                None => (target.as_str(), None),
            };
            let file = match path.is_empty() {
                true => from.to_path_buf(),
                false => here.join(path),
            };
            let wrong = if !file.exists() {
                "no such file"
            } else if heading.is_some_and(|heading| {
                let known = headings
                    .get(&canonical(&file))
                    .cloned()
                    .unwrap_or_else(|| self::headings(&read(&file)));
                !known.iter().any(|slug| slug == heading)
            }) {
                "no such heading"
            } else {
                return None;
            };
            Some(format!("{}: {target} — {wrong}", from.display()))
        })
        .collect()
}

fn local_links(markdown: &str) -> Vec<String> {
    Parser::new(markdown)
        .filter_map(|event| match event {
            Event::Start(Tag::Link { dest_url, .. }) => Some(dest_url.to_string()),
            _ => None,
        })
        .filter(|target| !target.contains("://") && !target.starts_with("mailto:"))
        .collect()
}

fn headings(markdown: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut reading = None;

    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::Heading { .. }) => reading = Some(String::new()),
            Event::Text(text) | Event::Code(text) => {
                if let Some(heading) = &mut reading {
                    heading.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(heading) = reading.take() {
                    found.push(slug(&heading));
                }
            }
            _ => {}
        }
    }
    found
}

fn slug(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '-')
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

fn markdown_under(root: &Path) -> Vec<PathBuf> {
    let worth_walking = |entry: &walkdir::DirEntry| {
        let name = entry.file_name().to_string_lossy();
        !name.starts_with('.') && !GENERATED.contains(&name.as_ref())
    };

    let mut found: Vec<PathBuf> = WalkDir::new(root)
        .into_iter()
        .filter_entry(worth_walking)
        .flatten()
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|path| path.extension().is_some_and(|kind| kind == "md"))
        .collect();
    found.sort();
    found
}

fn app_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let declares_workspace = |dir: &Path| {
        fs::read_to_string(dir.join(WORKSPACE_MANIFEST))
            .is_ok_and(|manifest| manifest.contains(WORKSPACE_SECTION))
    };

    manifest
        .ancestors()
        .find(|dir| declares_workspace(dir))
        .expect("a Cargo.toml declaring a workspace at or above this crate")
        .to_path_buf()
}

fn canonical(doc: &Path) -> PathBuf {
    doc.canonicalize()
        .unwrap_or_else(|err| panic!("resolve {}: {err}", doc.display()))
}

fn read(doc: &Path) -> String {
    fs::read_to_string(doc).unwrap_or_else(|err| panic!("read {}: {err}", doc.display()))
}

#[test]
fn a_link_carries_its_path_and_the_heading_it_names() {
    assert_eq!(
        local_links("see [the store](../../docs/medallion.md#root) for that"),
        vec!["../../docs/medallion.md#root"]
    );
}

#[test]
fn a_link_within_a_page_carries_only_a_heading() {
    assert_eq!(
        local_links("[x](#what-fits-on-the-panel)"),
        vec!["#what-fits-on-the-panel"]
    );
}

#[test]
fn a_url_is_not_a_link_into_the_docs() {
    assert!(local_links("[rerun](https://rerun.io) and [x](http://a.b/c#d)").is_empty());
}

#[test]
fn a_link_broken_over_two_lines_is_still_one_link() {
    assert_eq!(
        local_links("see [the\npanel](../m5/README.md#what-fits) there"),
        vec!["../m5/README.md#what-fits"]
    );
}

#[test]
fn a_target_inside_a_code_span_or_block_is_not_a_link() {
    assert!(local_links("`[x](nope.md)` and\n\n```\n[y](nope.md)\n```\n").is_empty());
}

#[test]
fn a_heading_becomes_the_anchor_github_gives_it() {
    assert_eq!(
        headings("# The `store`\n\n## A rebuild, deleting what it produces\n"),
        vec!["the-store", "a-rebuild-deleting-what-it-produces"]
    );
}

#[test]
fn a_hash_inside_a_fenced_block_is_not_a_heading() {
    assert_eq!(
        headings("# real\n\n```\n# /// script\n```\n\n## also real\n"),
        vec!["real", "also-real"]
    );
}
