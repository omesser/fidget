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
        ContentBlock::Image(image) => format!("[image {}]", escaped(&image.mime_type)),
        ContentBlock::Audio(audio) => format!("[audio {}]", escaped(&audio.mime_type)),
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
/// would refuse stays text with its uri, so nothing is dropped.
fn link_mark(link: &ResourceLink) -> String {
    let name = if link.name.is_empty() {
        &link.uri
    } else {
        &link.name
    };
    match platform::openable(&link.uri) {
        Ok(target) => format!(
            "[{}]({})",
            escaped(name),
            target.replace('(', "%28").replace(')', "%29")
        ),
        Err(_) => format!("[link {} {}]", escaped(name), escaped(&link.uri)),
    }
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
        .map(|mime| format!(", {}", escaped(mime)))
        .unwrap_or_default();
    format!("[resource {}{mime}, {bytes} bytes]", escaped(uri))
}

/// The bytes a base64 string stands for, from its length alone.
fn decoded_len(base64: &str) -> usize {
    let digits = base64.trim_end_matches('=').len();
    digits * 3 / 4
}

/// One line of text that Markdown reads back as itself: a newline is a space,
/// and every character that means something to Markdown has its backslash.
fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' | '\r' => out.push(' '),
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
}
