//! A content block that is not text, as the line the transcript keeps of it.
//!
//! ADR-0028: Chat draws what ACP sends and invents nothing, so an image or a
//! resource arrives as a name and a kind, never as nothing. A mark is
//! Markdown, drawn by `src/markdown.js`, so every variable field is escaped
//! until it cannot close a label, open a link or forge a tag.

use agent_client_protocol::schema::v1::{ContentBlock, EmbeddedResourceResource, ResourceLink};

use crate::platform;

/// What one chunk adds to the text so far. Text is itself. Any other block is
/// its mark as a paragraph of its own: Markdown joins single lines, and two
/// marks, or a mark and the words around it, must not run together.
pub(crate) fn chunk_text(block: &ContentBlock, so_far: &str) -> String {
    let mark = match block {
        ContentBlock::Text(text) => return text.text.clone(),
        ContentBlock::Image(image) => format!("[image {}]", escaped(&image.mime_type, FIELD_LIMIT)),
        ContentBlock::Audio(audio) => format!("[audio {}]", escaped(&audio.mime_type, FIELD_LIMIT)),
        ContentBlock::ResourceLink(link) => link_mark(link),
        ContentBlock::Resource(embedded) => resource_mark(&embedded.resource),
        _ => "[content]".to_string(),
    };
    let lead = match so_far {
        "" => "",
        so_far if so_far.ends_with("\n\n") => "",
        so_far if so_far.ends_with('\n') => "\n",
        _ => "\n\n",
    };
    format!("{lead}{mark}\n\n")
}

/// A link Chat opens is a Markdown link, so it draws clickable. The target is
/// the one `open_link` would open, so what is drawn is what opens. A link it
/// would refuse stays text with its uri, so nothing is dropped. A target over
/// the limit is text too: cut short, it would open somewhere else.
fn link_mark(link: &ResourceLink) -> String {
    let name = if link.name.is_empty() {
        &link.uri
    } else {
        &link.name
    };
    let target = platform::openable(&link.uri)
        .ok()
        .filter(|target| target.chars().count() <= URI_LIMIT);
    let Some(target) = target else {
        return format!(
            "[link {} {}]",
            escaped(name, FIELD_LIMIT),
            escaped(&link.uri, URI_LIMIT)
        );
    };
    let drawn = format!(
        "[{}]({})",
        escaped(name, FIELD_LIMIT),
        target.replace('(', "%28").replace(')', "%29")
    );
    // A name that reads as another site must not be the only thing seen.
    if names_another_host(name, &target) {
        format!("{drawn} {}", escaped(&target, URI_LIMIT))
    } else {
        drawn
    }
}

/// Whether `name` is a URL, or a `www.` host, on a host other than `target`'s.
fn names_another_host(name: &str, target: &str) -> bool {
    let host = |url: &str| {
        url::Url::parse(url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_string))
    };
    let named = host(name).or_else(|| {
        name.get(..4)
            .filter(|start| start.eq_ignore_ascii_case("www."))
            .and_then(|_| host(&format!("https://{name}")))
    });
    named.is_some_and(|named| Some(named) != host(target))
}

fn resource_mark(resource: &EmbeddedResourceResource) -> String {
    let (uri, mime, bytes) = match resource {
        EmbeddedResourceResource::TextResourceContents(text) => {
            (&text.uri, &text.mime_type, text.text.len())
        }
        EmbeddedResourceResource::BlobResourceContents(blob) => {
            (&blob.uri, &blob.mime_type, decoded_len(&blob.blob))
        }
        _ => return "[resource]".to_string(),
    };
    let mime = mime
        .as_deref()
        .map(|mime| format!(", {}", escaped(mime, FIELD_LIMIT)))
        .unwrap_or_default();
    format!(
        "[resource {}{mime}, {bytes} bytes]",
        escaped(uri, URI_LIMIT)
    )
}

/// The bytes a base64 string stands for, from its digits alone: padding and
/// whitespace are not data.
fn decoded_len(base64: &str) -> usize {
    let digits = base64
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '=')
        .count();
    digits * 3 / 4
}

/// The most characters a name or a mime type keeps in a mark, and a uri. A
/// longer field is cut and ends in `…`, so one field cannot flood a row.
const FIELD_LIMIT: usize = 200;
const URI_LIMIT: usize = 2000;

/// One line of text that Markdown reads back as itself: a control character or
/// line separator is a space, every character that means something to Markdown
/// has its backslash, and the whole is cut at `limit` characters.
fn escaped(text: &str, limit: usize) -> String {
    let mut out = String::new();
    for (n, c) in text.chars().enumerate() {
        if n == limit {
            out.push('…');
            break;
        }
        match c {
            c if c.is_control() || c == '\u{2028}' || c == '\u{2029}' => out.push(' '),
            '\\' | '[' | ']' | '(' | ')' | '<' | '>' | '*' | '_' | '`' | '&' | '~' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{
        AudioContent, BlobResourceContents, EmbeddedResource, EmbeddedResourceResource,
        ImageContent, ResourceLink, TextContent, TextResourceContents,
    };

    fn mark(block: ContentBlock) -> String {
        chunk_text(&block, "")
    }

    fn link(name: &str, uri: &str) -> ContentBlock {
        ContentBlock::ResourceLink(ResourceLink::new(name, uri))
    }

    #[test]
    fn an_image_and_audio_are_named_by_their_kind() {
        assert_eq!(
            mark(ContentBlock::Image(ImageContent::new("AAAA", "image/png"))),
            "[image image/png]\n\n"
        );
        assert_eq!(
            mark(ContentBlock::Audio(AudioContent::new("AAAA", "audio/wav"))),
            "[audio audio/wav]\n\n"
        );
    }

    #[test]
    fn a_link_to_a_url_chat_opens_is_a_markdown_link() {
        assert_eq!(
            mark(link("spec", "https://example.com/spec.md")),
            "[spec](https://example.com/spec.md)\n\n"
        );
        assert_eq!(
            mark(link("mail", "mailto:oded@example.com")),
            "[mail](mailto:oded@example.com)\n\n"
        );
    }

    #[test]
    fn a_links_name_cannot_close_its_own_label() {
        assert_eq!(
            mark(link("a](https://evil.example) [b", "https://example.com/")),
            "[a\\]\\(https://evil.example\\) \\[b](https://example.com/)\n\n"
        );
        assert_eq!(
            mark(link("two\nlines", "https://example.com/")),
            "[two lines](https://example.com/)\n\n"
        );
    }

    #[test]
    fn a_links_url_cannot_close_its_own_destination() {
        assert_eq!(
            mark(link("x", "https://example.com/a(b)c d")),
            "[x](https://example.com/a%28b%29c%20d)\n\n"
        );
    }

    #[test]
    fn a_link_chat_will_not_open_stays_as_text_with_its_uri() {
        assert_eq!(
            mark(link("passwd", "file:///etc/passwd")),
            "[link passwd file:///etc/passwd]\n\n"
        );
        assert_eq!(
            mark(link("x", "javascript:alert(1)")),
            "[link x javascript:alert\\(1\\)]\n\n"
        );
        assert_eq!(
            mark(link("x", "no scheme here")),
            "[link x no scheme here]\n\n"
        );
    }

    #[test]
    fn a_nameless_link_is_named_by_its_uri() {
        assert_eq!(
            mark(link("", "https://example.com/")),
            "[https://example.com/](https://example.com/)\n\n"
        );
    }

    #[test]
    fn an_embedded_resource_says_its_uri_type_and_size() {
        let text = EmbeddedResourceResource::TextResourceContents(
            TextResourceContents::new("hello", "file:///notes.txt").mime_type("text/plain"),
        );
        assert_eq!(
            mark(ContentBlock::Resource(EmbeddedResource::new(text))),
            "[resource file:///notes.txt, text/plain, 5 bytes]\n\n"
        );
        let blob = EmbeddedResourceResource::BlobResourceContents(BlobResourceContents::new(
            "aGVsbG8=",
            "file:///a.bin",
        ));
        assert_eq!(
            mark(ContentBlock::Resource(EmbeddedResource::new(blob))),
            "[resource file:///a.bin, 5 bytes]\n\n"
        );
    }

    #[test]
    fn a_resource_uri_cannot_forge_a_link() {
        let hostile = EmbeddedResourceResource::TextResourceContents(TextResourceContents::new(
            "",
            "x](https://evil.example",
        ));
        assert_eq!(
            mark(ContentBlock::Resource(EmbeddedResource::new(hostile))),
            "[resource x\\]\\(https://evil.example, 0 bytes]\n\n"
        );
    }

    #[test]
    fn text_is_itself_and_a_mark_is_a_paragraph_of_its_own() {
        let text = ContentBlock::Text(TextContent::new("Here:"));
        assert_eq!(chunk_text(&text, "anything"), "Here:");
        let image = ContentBlock::Image(ImageContent::new("AAAA", "image/png"));
        assert_eq!(chunk_text(&image, "Here:"), "\n\n[image image/png]\n\n");
        assert_eq!(chunk_text(&image, "Here:\n"), "\n[image image/png]\n\n");
        assert_eq!(chunk_text(&image, "Here:\n\n"), "[image image/png]\n\n");
    }

    fn resource(uri: &str, mime: Option<&str>) -> ContentBlock {
        let text = TextResourceContents::new("", uri);
        let text = match mime {
            Some(mime) => text.mime_type(mime),
            None => text,
        };
        ContentBlock::Resource(EmbeddedResource::new(
            EmbeddedResourceResource::TextResourceContents(text),
        ))
    }

    fn blob(base64: &str) -> String {
        let blob = BlobResourceContents::new(base64, "file:///b");
        mark(ContentBlock::Resource(EmbeddedResource::new(
            EmbeddedResourceResource::BlobResourceContents(blob),
        )))
    }

    #[test]
    fn a_name_that_poses_as_another_host_is_followed_by_the_real_target() {
        assert_eq!(
            mark(link("https://paypal.com", "https://evil.example/x")),
            "[https://paypal.com](https://evil.example/x) https://evil.example/x\n\n"
        );
        assert_eq!(
            mark(link("www.paypal.com", "https://evil.example/x")),
            "[www.paypal.com](https://evil.example/x) https://evil.example/x\n\n"
        );
    }

    #[test]
    fn a_name_on_the_targets_own_host_or_no_host_stands_alone() {
        assert_eq!(
            mark(link("https://example.com/docs", "https://example.com/spec")),
            "[https://example.com/docs](https://example.com/spec)\n\n"
        );
        assert_eq!(
            mark(link("spec.md", "https://example.com/spec")),
            "[spec.md](https://example.com/spec)\n\n"
        );
    }

    /// A name or a mime type is cut at 200 characters and a uri at 2000, with
    /// a `\u{2026}` where the cut fell. A field exactly at its limit is whole.
    #[test]
    fn a_field_over_its_limit_is_cut_with_a_visible_mark() {
        let at = "n".repeat(200);
        assert_eq!(
            mark(link(&at, "https://example.com/")),
            format!("[{at}](https://example.com/)\n\n")
        );
        assert_eq!(
            mark(link(&format!("{at}n"), "https://example.com/")),
            format!("[{at}\u{2026}](https://example.com/)\n\n")
        );
        assert_eq!(
            mark(resource("file:///a", Some(&at))),
            format!("[resource file:///a, {at}, 0 bytes]\n\n")
        );
        assert_eq!(
            mark(resource("file:///a", Some(&format!("{at}m")))),
            format!("[resource file:///a, {at}\u{2026}, 0 bytes]\n\n")
        );
        let uri = format!("file:///{}", "u".repeat(1992));
        assert_eq!(uri.chars().count(), 2000);
        assert_eq!(
            mark(resource(&uri, None)),
            format!("[resource {uri}, 0 bytes]\n\n")
        );
        assert_eq!(
            mark(resource(&format!("{uri}u"), None)),
            format!("[resource {uri}\u{2026}, 0 bytes]\n\n")
        );
    }

    /// A target cut short would open somewhere else, so a link whose target is
    /// over the limit is text.
    #[test]
    fn a_link_whose_target_is_over_the_limit_is_text_not_a_cut_link() {
        let long = format!("https://example.com/{}", "p".repeat(2000));
        let cut: String = long.chars().take(2000).collect();
        assert_eq!(
            mark(link("long", &long)),
            format!("[link long {cut}\u{2026}]\n\n")
        );
    }

    #[test]
    fn a_blobs_size_is_its_decoded_length_padded_or_not_and_whitespace_aside() {
        assert_eq!(blob("aGVsbG8="), "[resource file:///b, 5 bytes]\n\n");
        assert_eq!(blob("aGVsbG8"), "[resource file:///b, 5 bytes]\n\n");
        assert_eq!(blob("aGVsbA=="), "[resource file:///b, 4 bytes]\n\n");
        assert_eq!(blob("aGVs\nbG8=\r\n "), "[resource file:///b, 5 bytes]\n\n");
        assert_eq!(blob(""), "[resource file:///b, 0 bytes]\n\n");
    }

    /// The `[content]` and `[resource]` arms cover a kind a later ACP adds.
    /// The schema's enums are non-exhaustive, so this build cannot build one,
    /// and a kind it does not know is refused before it reaches a mark.
    #[test]
    fn a_kind_this_build_does_not_know_is_refused_before_a_mark() {
        let unknown = serde_json::json!({"type": "hologram", "uri": "x"});
        assert!(serde_json::from_value::<ContentBlock>(unknown).is_err());
        let unknown = serde_json::json!({"type": "resource", "resource": {"hologram": 1}});
        assert!(serde_json::from_value::<ContentBlock>(unknown).is_err());
    }

    /// What Markdown, a browser or a line breaker could read as structure stays
    /// text, in a name, a uri and a mime type alike. The same strings go through
    /// `drawThought` in `tests/chat-markdown.test.js`.
    #[test]
    fn the_characters_a_mark_leaves_bare_stay_inert() {
        let nasty = "a\\b`c!d#e|f-g+h=i\u{2028}j\u{85}k\u{2029}l";
        let spaced = "a\\\\b\\`c!d#e|f-g+h=i j k l";
        assert_eq!(
            mark(link(nasty, "https://example.com/")),
            format!("[{spaced}](https://example.com/)\n\n")
        );
        assert_eq!(
            mark(link(nasty, "no scheme")),
            format!("[link {spaced} no scheme]\n\n")
        );
        assert_eq!(
            mark(resource(nasty, Some(nasty))),
            format!("[resource {spaced}, {spaced}, 0 bytes]\n\n")
        );
    }
}
