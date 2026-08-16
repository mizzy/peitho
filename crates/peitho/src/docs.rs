use std::{fmt::Write as _, sync::LazyLock};

use peitho_core::error::{BuildError, ErrorKind};

// These paths reach outside the crate, so `cargo package`/`publish` would break; peitho ships as a repo-built Homebrew binary, not a crates.io crate.
const GUIDE_SOURCES: &[(&str, &str)] = &[
    ("cli", include_str!("../../../site/content/guide/cli.md")),
    (
        "frontmatter",
        include_str!("../../../site/content/guide/frontmatter.md"),
    ),
    (
        "getting-started",
        include_str!("../../../site/content/guide/getting-started.md"),
    ),
    (
        "layouts",
        include_str!("../../../site/content/guide/layouts.md"),
    ),
    (
        "writing-decks",
        include_str!("../../../site/content/guide/writing-decks.md"),
    ),
];

#[derive(Debug)]
pub(crate) struct Topic {
    pub(crate) slug: &'static str,
    pub(crate) title: &'static str,
    pub(crate) description: &'static str,
    pub(crate) body: &'static str,
    weight: i64,
}

pub(crate) static TOPICS: LazyLock<Vec<Topic>> = LazyLock::new(|| {
    let mut topics = GUIDE_SOURCES
        .iter()
        .map(|&(slug, source)| parse_topic(slug, source))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|err| panic!("failed to parse embedded guide: {err}"));
    topics.sort_by_key(|topic| topic.weight);
    topics
});

fn parse_topic(slug: &'static str, source: &'static str) -> Result<Topic, BuildError> {
    let source = source
        .strip_prefix("+++\n")
        .ok_or_else(|| invalid_frontmatter(slug, "the opening `+++` delimiter is missing"))?;
    let (frontmatter, body) = source
        .split_once("\n+++\n")
        .ok_or_else(|| invalid_frontmatter(slug, "the closing `+++` delimiter is missing"))?;
    let body = body.strip_prefix('\n').unwrap_or(body);

    let mut title = None;
    let mut description = None;
    let mut weight = None;
    for line in frontmatter.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            let key = line.split_whitespace().next().unwrap_or("<missing>");
            return Err(invalid_frontmatter(
                slug,
                format!("`{key}` must use `key = <value>` syntax"),
            ));
        };
        let key = key.trim();
        match key {
            "title" => title = Some(parse_string(slug, "title", value)?),
            "description" => description = Some(parse_string(slug, "description", value)?),
            "weight" => {
                weight = Some(
                    value
                        .trim()
                        .parse::<i64>()
                        .map_err(|_| invalid_frontmatter(slug, "`weight` must be an integer"))?,
                )
            }
            "template" | "insert_anchor_links" => {
                parse_quoted_string(slug, key, value)?;
            }
            _ => {
                return Err(invalid_frontmatter(
                    slug,
                    format!(
                        "unknown key `{key}`; expected one of `title`, `description`, `weight`, `template`, or `insert_anchor_links`"
                    ),
                ));
            }
        }
    }

    let title = title.ok_or_else(|| invalid_frontmatter(slug, "`title` is missing"))?;
    let description =
        description.ok_or_else(|| invalid_frontmatter(slug, "`description` is missing"))?;
    let weight = weight.ok_or_else(|| invalid_frontmatter(slug, "`weight` is missing"))?;

    Ok(Topic {
        slug,
        title,
        description,
        body,
        weight,
    })
}

fn parse_string(slug: &str, key: &str, value: &'static str) -> Result<&'static str, BuildError> {
    let parsed = parse_quoted_string(slug, key, value)?;
    if parsed.is_empty() {
        return Err(invalid_frontmatter(
            slug,
            format!("`{key}` must be a non-empty quoted string"),
        ));
    }
    Ok(parsed)
}

fn parse_quoted_string(
    slug: &str,
    key: &str,
    value: &'static str,
) -> Result<&'static str, BuildError> {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or_else(|| invalid_frontmatter(slug, format!("`{key}` must be a quoted string")))
}

fn invalid_frontmatter(slug: &str, detail: impl Into<String>) -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        None,
        format!(
            "invalid embedded guide frontmatter for topic '{slug}': {}",
            detail.into()
        ),
        "guide pages must start with Zola TOML frontmatter containing title, weight, and description",
    )
}

pub(crate) fn render(topic: Option<&str>, all: bool) -> Result<String, BuildError> {
    if all {
        return Ok(render_all());
    }
    match topic {
        Some(slug) => TOPICS
            .iter()
            .find(|topic| topic.slug == slug)
            .map(render_topic)
            .ok_or_else(|| unknown_topic(slug)),
        None => Ok(render_list()),
    }
}

fn render_list() -> String {
    let width = TOPICS
        .iter()
        .map(|topic| topic.slug.len())
        .max()
        .unwrap_or(0);
    let mut output = String::new();
    for topic in TOPICS.iter() {
        writeln!(output, "{:<width$}  {}", topic.slug, topic.description)
            .expect("writing to a string cannot fail");
    }
    writeln!(
        output,
        "\nRun `peitho docs <topic>` for one topic or `peitho docs --all` for the complete guide."
    )
    .expect("writing to a string cannot fail");
    output
}

fn render_all() -> String {
    let mut output = String::new();
    for (index, topic) in TOPICS.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        output.push_str(&render_topic(topic));
    }
    output
}

fn render_topic(topic: &Topic) -> String {
    let mut output = String::new();
    writeln!(output, "# {}\n", topic.title).expect("writing to a string cannot fail");
    output.push_str(&rewrite_internal_links(topic.body));
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output
}

fn rewrite_internal_links(body: &str) -> String {
    const LINK_MARKER: &str = "](";

    let mut output = String::with_capacity(body.len());
    let mut remaining = body;
    while let Some(label_end) = remaining.find(LINK_MARKER) {
        let destination_start = label_end + LINK_MARKER.len();
        let destination = &remaining[destination_start..];
        let is_zola_link = destination.starts_with("@/");
        let is_root_relative_link = destination.starts_with('/') && !destination.starts_with("//");
        if !is_zola_link && !is_root_relative_link {
            output.push_str(&remaining[..destination_start]);
            remaining = destination;
            continue;
        }

        let Some(destination_end_offset) = remaining[destination_start..].find(')') else {
            break;
        };
        let destination_end = destination_start + destination_end_offset;
        let Some(label_start) = remaining[..label_end].rfind('[') else {
            output.push_str(&remaining[..=destination_end]);
            remaining = &remaining[destination_end + 1..];
            continue;
        };

        output.push_str(&remaining[..label_start]);
        let label = &remaining[label_start + 1..label_end];
        let destination = &remaining[destination_start..destination_end];
        write_rewritten_link(&mut output, label, destination);
        remaining = &remaining[destination_end + 1..];
    }
    output.push_str(remaining);
    output
}

fn write_rewritten_link(output: &mut String, label: &str, destination: &str) {
    if destination.starts_with('/') {
        write!(output, "[{label}](https://peitho.gosu.ke{destination})")
            .expect("writing to a string cannot fail");
        return;
    }

    if let Some(guide_path) = destination.strip_prefix("@/guide/") {
        let page = guide_path
            .split_once('#')
            .map_or(guide_path, |(page, _)| page);
        if let Some(slug) = page.strip_suffix(".md") {
            write!(output, "{label} (see `peitho docs {slug}`)")
                .expect("writing to a string cannot fail");
            return;
        }
    }

    let path_with_anchor = destination
        .strip_prefix("@/")
        .expect("only Zola-internal links reach this function");
    let (path, anchor) = path_with_anchor
        .split_once('#')
        .map_or((path_with_anchor, None), |(path, anchor)| {
            (path, Some(anchor))
        });
    let path = path.strip_suffix(".md").unwrap_or(path);

    write!(output, "[{label}](https://peitho.gosu.ke/{path}")
        .expect("writing to a string cannot fail");
    if !path.ends_with('/') {
        output.push('/');
    }
    if let Some(anchor) = anchor {
        write!(output, "#{anchor}").expect("writing to a string cannot fail");
    }
    output.push(')');
}

fn unknown_topic(slug: &str) -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        None,
        format!("unknown docs topic '{slug}'"),
        format!(
            "valid topics: {}",
            TOPICS
                .iter()
                .map(|topic| topic.slug)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    )
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs, path::Path};

    use assert_cmd::Command as AssertCommand;

    use super::*;

    #[test]
    fn topic_table_parses_every_embedded_page() {
        for &(slug, source) in GUIDE_SOURCES {
            let topic = parse_topic(slug, source)
                .unwrap_or_else(|err| panic!("failed to parse {slug}: {err}"));

            assert!(!topic.title.is_empty(), "missing title for {slug}");
            assert!(
                !topic.description.is_empty(),
                "missing description for {slug}"
            );
            assert!(!topic.body.trim_start().starts_with("+++"));
        }

        assert!(TOPICS
            .windows(2)
            .all(|pair| pair[0].weight <= pair[1].weight));
    }

    #[test]
    fn strict_frontmatter_accepts_supported_ignored_keys_and_blank_lines() {
        let topic = parse_topic(
            "test",
            "+++\ntitle = \"Test\"\n\ndescription = \"Description\"\nweight = -1\ntemplate = \"guide-page.html\"\ninsert_anchor_links = \"right\"\n+++\n\nBody\n",
        )
        .unwrap();

        assert_eq!(topic.title, "Test");
        assert_eq!(topic.weight, -1);
        assert_eq!(topic.body, "Body\n");
    }

    #[test]
    fn strict_frontmatter_rejects_unknown_structure_and_malformed_values() {
        let cases = [
            (
                "+++\ntitle \"Test\"\n+++\n\nBody\n",
                "`title`",
                "key = <value>",
            ),
            (
                "+++\ntitle = \"Test\"\ndescription = \"Description\"\nweight = 1\ntemplate = guide-page.html\n+++\n\nBody\n",
                "`template`",
                "quoted string",
            ),
            (
                "+++\ntitle = \"Test\"\ndescription = \"Description\"\nweight = 1\ninsert_anchor_links = false\n+++\n\nBody\n",
                "`insert_anchor_links`",
                "quoted string",
            ),
            (
                "+++\ntitle = \"Test\"\ndescription = \"Description\"\nweight = heavy\n+++\n\nBody\n",
                "`weight`",
                "integer",
            ),
            (
                "+++\ntitle = \"Test\"\ndescription = \"Description\"\nweight = 1\nextra = \"value\"\n+++\n\nBody\n",
                "`extra`",
                "expected one of",
            ),
        ];

        for (source, key, expectation) in cases {
            let err = parse_topic("broken", source).unwrap_err();
            assert!(err.message.contains("topic 'broken'"), "{err}");
            assert!(err.message.contains(key), "{err}");
            assert!(err.message.contains(expectation), "{err}");
        }
    }

    #[test]
    fn embedded_topics_match_guide_pages() {
        let guide_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../site/content/guide");
        let expected = fs::read_dir(guide_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("md"))
            .filter_map(|path| guide_slug(&path))
            .collect::<BTreeSet<_>>();
        let actual = TOPICS
            .iter()
            .map(|topic| topic.slug.to_owned())
            .collect::<BTreeSet<_>>();

        assert_eq!(actual, expected);
    }

    #[test]
    fn docs_list() {
        let assert = AssertCommand::cargo_bin("peitho")
            .unwrap()
            .arg("docs")
            .assert()
            .success();
        let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

        for topic in TOPICS.iter() {
            assert!(stdout.contains(topic.slug), "actual stdout: {stdout}");
            assert!(
                stdout.contains(topic.description),
                "actual stdout: {stdout}"
            );
        }
        assert!(stdout.contains("peitho docs <topic>"));
        assert!(stdout.contains("peitho docs --all"));
    }

    #[test]
    fn docs_topic() {
        let topic = TOPICS.first().expect("embedded topics");
        let assert = AssertCommand::cargo_bin("peitho")
            .unwrap()
            .args(["docs", topic.slug])
            .assert()
            .success();
        let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();

        assert_eq!(stdout, render_topic(topic));
        assert!(stdout.starts_with(&format!("# {}\n\n", topic.title)));
        assert!(!stdout.trim_start().starts_with("+++"));
    }

    #[test]
    fn docs_all() {
        let assert = AssertCommand::cargo_bin("peitho")
            .unwrap()
            .args(["docs", "--all"])
            .assert()
            .success();
        let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
        let mut previous_heading = None;

        for topic in TOPICS.iter() {
            let heading = format!("# {}\n", topic.title);
            let position = stdout
                .find(&heading)
                .unwrap_or_else(|| panic!("missing {heading:?} in stdout"));
            if let Some(previous) = previous_heading {
                assert!(previous < position, "topics are not in weight order");
            }
            previous_heading = Some(position);
            assert!(stdout.contains(&render_topic(topic)));
        }
    }

    #[test]
    fn guide_links_without_anchors_become_command_references() {
        assert_eq!(
            rewrite_internal_links("Read [Writing Decks](@/guide/writing-decks.md)."),
            "Read Writing Decks (see `peitho docs writing-decks`)."
        );
    }

    #[test]
    fn guide_links_with_anchors_become_command_references() {
        assert_eq!(
            rewrite_internal_links("Read [`peitho present`](@/guide/cli.md#peitho-present)."),
            "Read `peitho present` (see `peitho docs cli`)."
        );
    }

    #[test]
    fn other_zola_links_become_absolute_docs_site_links_and_keep_anchors() {
        assert_eq!(
            rewrite_internal_links(
                "[Example](@/examples/code-images.md) [Section](@/examples/code-images.md#mermaid)"
            ),
            "[Example](https://peitho.gosu.ke/examples/code-images/) [Section](https://peitho.gosu.ke/examples/code-images/#mermaid)"
        );
    }

    #[test]
    fn root_relative_image_links_become_absolute_and_keep_queries_and_anchors() {
        assert_eq!(
            rewrite_internal_links(
                "![Preview](/guide-shots/preview-single.png) [Raw](/guide-shots/preview-single.png?raw=1#preview)"
            ),
            "![Preview](https://peitho.gosu.ke/guide-shots/preview-single.png) [Raw](https://peitho.gosu.ke/guide-shots/preview-single.png?raw=1#preview)"
        );
    }

    #[test]
    fn protocol_relative_and_external_links_are_untouched() {
        let body = "[Protocol relative](//example.com/path) [HTTPS](https://example.com/path) [HTTP](http://example.com) [relative](../guide.md#topic)";

        assert_eq!(rewrite_internal_links(body), body);
    }

    #[test]
    fn rendered_topic_bodies_have_no_zola_internal_links() {
        for topic in TOPICS.iter() {
            let rendered = render(Some(topic.slug), false).unwrap();
            assert!(
                !rendered.contains("@/"),
                "rendered topic '{}' still contains a Zola link:\n{rendered}",
                topic.slug
            );
            assert!(
                !rendered.contains("](/"),
                "rendered topic '{}' still contains a root-relative link:\n{rendered}",
                topic.slug
            );
        }
    }

    #[test]
    fn docs_unknown_topic_names_valid_topics() {
        let assert = AssertCommand::cargo_bin("peitho")
            .unwrap()
            .args(["docs", "missing"])
            .assert()
            .failure();
        let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();

        assert!(
            stderr.contains("unknown docs topic 'missing'"),
            "actual stderr: {stderr}"
        );
        for topic in TOPICS.iter() {
            assert!(stderr.contains(topic.slug), "actual stderr: {stderr}");
        }
    }

    fn guide_slug(path: &Path) -> Option<String> {
        let stem = path.file_stem()?.to_str()?;
        (stem != "_index").then(|| stem.to_owned())
    }
}
