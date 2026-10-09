//! What a tool call carried, as the pieces Chat draws in its replayed row.
//!
//! ADR-0028: every field the wire sends reaches the reader in some form. A
//! diff is a path and line counts, never a viewer. A terminal is its id.

use agent_client_protocol::schema::v1::{ContentBlock, ToolCallContent, ToolCallLocation};
use serde::Serialize;
use similar::{ChangeTag, TextDiff};

use crate::content_mark;

/// One thing a tool call put out. `Text` is drawn as written. `Mark` is
/// Markdown from `content_mark`, so its links are live.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolPiece {
    Text {
        text: String,
    },
    Mark {
        markdown: String,
    },
    Diff {
        path: String,
        added: usize,
        removed: usize,
    },
    Terminal {
        id: String,
    },
}

/// A file a tool call touched, and the line when the Harness sent one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Place {
    path: String,
    line: Option<u32>,
}

pub(crate) fn pieces(content: &[ToolCallContent]) -> Vec<ToolPiece> {
    content
        .iter()
        .map(|piece| match piece {
            ToolCallContent::Content(block) => match &block.content {
                ContentBlock::Text(text) => ToolPiece::Text {
                    text: text.text.clone(),
                },
                other => ToolPiece::Mark {
                    markdown: content_mark::chunk_text(other, "").trim_end().to_string(),
                },
            },
            ToolCallContent::Diff(diff) => {
                let (added, removed) = line_counts(diff.old_text.as_deref(), &diff.new_text);
                ToolPiece::Diff {
                    path: diff.path.display().to_string(),
                    added,
                    removed,
                }
            }
            ToolCallContent::Terminal(terminal) => ToolPiece::Terminal {
                id: terminal.terminal_id.0.to_string(),
            },
            _ => ToolPiece::Mark {
                markdown: "[content]".to_string(),
            },
        })
        .collect()
}

pub(crate) fn places(locations: &[ToolCallLocation]) -> Vec<Place> {
    locations
        .iter()
        .map(|at| Place {
            path: at.path.display().to_string(),
            line: at.line,
        })
        .collect()
}

/// Lines added and removed going from `old` to `new`, as `git diff --numstat`
/// counts them. A file with no `old` text is all added.
fn line_counts(old: Option<&str>, new: &str) -> (usize, usize) {
    let diff = TextDiff::from_lines(old.unwrap_or(""), new);
    diff.iter_all_changes()
        .fold((0, 0), |(added, removed), change| match change.tag() {
            ChangeTag::Insert => (added + 1, removed),
            ChangeTag::Delete => (added, removed + 1),
            ChangeTag::Equal => (added, removed),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_file_counts_the_lines_it_added_and_removed() {
        assert_eq!(
            line_counts(Some("a\nb\nc\nd\n"), "a\nB\nc\nd\ne\nf\n"),
            (3, 1)
        );
    }

    #[test]
    fn a_new_file_is_all_added_and_an_emptied_one_all_removed() {
        assert_eq!(line_counts(None, "x\ny\nz\n"), (3, 0));
        assert_eq!(line_counts(Some("x\ny\n"), ""), (0, 2));
    }

    #[test]
    fn an_unchanged_file_adds_and_removes_nothing() {
        assert_eq!(line_counts(Some("a\nb\n"), "a\nb\n"), (0, 0));
        assert_eq!(line_counts(None, ""), (0, 0));
    }

    #[test]
    fn a_moved_line_counts_as_one_removed_and_one_added() {
        assert_eq!(line_counts(Some("a\nb\nc\n"), "b\nc\na\n"), (1, 1));
    }
}
