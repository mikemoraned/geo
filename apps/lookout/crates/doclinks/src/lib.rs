use std::fmt::{self, Display};
use std::fs;
use std::path::{Path, PathBuf};

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broken {
    pub from: PathBuf,
    pub target: String,
    pub reason: Reason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    NoSuchFile,
    NoSuchHeading,
}

impl Display for Broken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self.reason {
            Reason::NoSuchFile => "no such file",
            Reason::NoSuchHeading => "no such heading",
        };
        write!(f, "{}: {} — {reason}", self.from.display(), self.target)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub path: String,
    pub heading: Option<String>,
}

pub fn links(markdown: &str) -> Vec<Link> {
    Parser::new(markdown)
        .filter_map(|event| match event {
            Event::Start(Tag::Link { dest_url, .. }) => Some(dest_url),
            _ => None,
        })
        .filter(|target| !target.contains("://") && !target.starts_with("mailto:"))
        .map(|target| match target.split_once('#') {
            Some((path, heading)) => Link {
                path: path.to_string(),
                heading: Some(heading.to_string()),
            },
            None => Link {
                path: target.to_string(),
                heading: None,
            },
        })
        .collect()
}

pub fn headings(markdown: &str) -> Vec<String> {
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

pub fn broken(markdown_files: &[PathBuf]) -> Vec<Broken> {
    let mut broken = Vec::new();

    for from in markdown_files {
        let text = fs::read_to_string(from).unwrap_or_default();
        let here = from.parent().unwrap_or(Path::new("."));

        for link in links(&text) {
            let target = match link.path.is_empty() {
                true => from.clone(),
                false => here.join(&link.path),
            };
            let shown = match &link.heading {
                Some(heading) => format!("{}#{heading}", link.path),
                None => link.path.clone(),
            };

            if !target.exists() {
                broken.push(Broken {
                    from: from.clone(),
                    target: shown,
                    reason: Reason::NoSuchFile,
                });
                continue;
            }
            let Some(heading) = &link.heading else {
                continue;
            };
            let names_a_heading = target.extension().is_some_and(|kind| kind == "md")
                && headings(&fs::read_to_string(&target).unwrap_or_default())
                    .iter()
                    .any(|found| found == heading);
            if !names_a_heading {
                broken.push(Broken {
                    from: from.clone(),
                    target: shown,
                    reason: Reason::NoSuchHeading,
                });
            }
        }
    }
    broken
}

pub fn markdown_under(root: &Path) -> Vec<PathBuf> {
    const GENERATED: [&str; 4] = ["target", ".venv", "node_modules", "site-packages"];

    let mut found = Vec::new();
    let mut unvisited = vec![root.to_path_buf()];

    while let Some(dir) = unvisited.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();

            if path.is_dir() {
                if !name.starts_with('.') && !GENERATED.contains(&name.as_ref()) {
                    unvisited.push(path);
                }
            } else if path.extension().is_some_and(|kind| kind == "md") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

pub fn app_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    manifest
        .ancestors()
        .find(|dir| dir.join("crates").is_dir() && dir.join("docs").is_dir())
        .expect("the crate sits under the app it documents")
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_is_its_path_and_the_heading_it_names() {
        let found = links("see [the store](../../docs/medallion.md#root) for that");

        assert_eq!(
            found,
            vec![Link {
                path: "../../docs/medallion.md".to_string(),
                heading: Some("root".to_string()),
            }]
        );
    }

    #[test]
    fn a_link_to_a_file_names_no_heading() {
        assert_eq!(links("[x](README.md)")[0].heading, None);
    }

    #[test]
    fn a_link_within_a_page_has_no_path() {
        assert_eq!(
            links("[x](#what-fits-on-the-panel)"),
            vec![Link {
                path: String::new(),
                heading: Some("what-fits-on-the-panel".to_string()),
            }]
        );
    }

    #[test]
    fn a_url_is_not_a_link_into_the_docs() {
        assert!(links("[rerun](https://rerun.io) and [x](http://a.b/c#d)").is_empty());
    }

    #[test]
    fn a_link_broken_over_two_lines_is_still_one_link() {
        let found = links("see [the\npanel](../m5/README.md#what-fits-on-the-panel) there");

        assert_eq!(found[0].path, "../m5/README.md");
        assert_eq!(found[0].heading.as_deref(), Some("what-fits-on-the-panel"));
    }

    #[test]
    fn a_target_inside_a_code_span_or_block_is_not_a_link() {
        assert!(links("`[x](nope.md)` and\n\n```\n[y](nope.md)\n```\n").is_empty());
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
        let markdown = "# real\n\n```\n# /// script\n```\n\n## also real\n";

        assert_eq!(headings(markdown), vec!["real", "also-real"]);
    }
}
