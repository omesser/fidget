//! A content block that is not text, as the line the transcript keeps of it.
//!
//! ADR-0028: Chat draws what ACP sends and invents nothing, so an image or a
//! resource arrives as a name and a kind, never as nothing. A mark is
//! Markdown, drawn by `src/markdown.js`, so every variable field is escaped
//! until it cannot close a label, open a link or forge a tag.

use agent_client_protocol::schema::v1::{ContentBlock, EmbeddedResourceResource, ResourceLink};

use crate::platform;

/// The mark for a block a reader has no other way to see, as the mark's own
/// line. Text is itself.
pub(crate) const UNKNOWN: &str = "[content]";

pub(crate) fn mark(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text(text) => text.text.clone(),
        ContentBlock::Image(image) => format!("[image {}]", escaped(&image.mime_type)),
        ContentBlock::Audio(audio) => format!("[audio {}]", escaped(&audio.mime_type)),
        ContentBlock::ResourceLink(link) => link_mark(link),
        ContentBlock::Resource(embedded) => resource_mark(&embedded.resource),
        _ => UNKNOWN.to_string(),
    }
}

/// What one chunk adds to the text so far. Text is itself. Any other block is
/// its mark as a paragraph of its own: Markdown joins single lines, and two
/// marks, or a mark and the words around it, must not run together.
pub(crate) fn chunk_text(block: &ContentBlock, so_far: &str) -> String {
    if let ContentBlock::Text(text) = block {
        return text.text.clone();
    }
    let lead = match so_far {
        "" => "",
        so_far if so_far.ends_with("\n\n") => "",
        so_far if so_far.ends_with('\n') => "\n",
        _ => "\n\n",
    };
    format!("{lead}{}\n\n", mark(block))
}

/// A link Chat opens is a Markdown link, so it draws clickable. The target is
/// the one `open_link` would open, so what is drawn is what opens. A link it
/// would refuse stays text with its uri, so nothing is dropped.
fn link_mark(link: &ResourceLink) -> String {
    let name = if link.name.is_empty() {
        &link.uri
    } else {
        &link.name
    };
    let Ok(target) = platform::openable(&link.uri) else {
        return format!("[link {} {}]", escaped(name), escaped(&link.uri));
    };
    let drawn = format!(
        "[{}]({})",
        escaped(name),
        target.replace('(', "%28").replace(')', "%29")
    );
    // A name that reads as another site must not be the only thing seen.
    if names_another_host(name, &target) {
        format!("{drawn} {}", escaped(&target))
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
    let (uri, mime, size) = match resource {
        EmbeddedResourceResource::TextResourceContents(text) => {
            (&text.uri, &text.mime_type, format!("{}", text.text.len()))
        }
        // Three bytes to every four base64 characters, near enough to say.
        EmbeddedResourceResource::BlobResourceContents(blob) => (
            &blob.uri,
            &blob.mime_type,
            format!("~{}", blob.blob.len() * 3 / 4),
        ),
        _ => return "[resource]".to_string(),
    };
    let mime = mime
        .as_deref()
        .map(|mime| format!(", {}", escaped(mime)))
        .unwrap_or_default();
    format!("[resource {}{mime}, {size} bytes]", escaped(uri))
}

/// One line of text that Markdown reads back as itself: a control character is
/// a space, and every character that means something to Markdown has its
/// backslash.
fn escaped(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            c if c.is_control() => out.push(' '),
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
    fn a_marks_own_line_has_no_paragraph_around_it() {
        let image = ContentBlock::Image(ImageContent::new("AAAA", "image/png"));
        assert_eq!(super::mark(&image), "[image image/png]");
        let text = ContentBlock::Text(TextContent::new("words"));
        assert_eq!(super::mark(&text), "words");
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
            "[resource file:///a.bin, ~6 bytes]\n\n"
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

    /// A blob's size is an estimate from its base64 length, and says so.
    #[test]
    fn a_blobs_size_is_an_estimate() {
        assert_eq!(blob("aGVsbG8="), "[resource file:///b, ~6 bytes]\n\n");
        assert_eq!(blob(""), "[resource file:///b, ~0 bytes]\n\n");
    }

    /// What Markdown or a browser could read as structure stays
    /// text, in a name, a uri and a mime type alike. The same strings go through
    /// `drawThought` in `tests/chat-markdown.test.js`.
    #[test]
    fn the_characters_a_mark_leaves_bare_stay_inert() {
        let nasty = "a\\b`c!d#e|f-g+h=i\u{85}j\u{7}k\u{1b}l";
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
