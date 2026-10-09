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
        approximate: bool,
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
                    markdown: content_mark::mark(other),
                },
            },
            ToolCallContent::Diff(diff) => {
                let counts = line_counts(diff.old_text.as_deref(), &diff.new_text);
                ToolPiece::Diff {
                    path: diff.path.display().to_string(),
                    added: counts.added,
                    removed: counts.removed,
                    approximate: counts.approximate,
                }
            }
            ToolCallContent::Terminal(terminal) => ToolPiece::Terminal {
                id: terminal.terminal_id.0.to_string(),
            },
            _ => ToolPiece::Mark {
                markdown: content_mark::UNKNOWN.to_string(),
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

/// Lines a diff added and removed. `approximate` is set when the edit was too
/// big to diff, and the counts are then the whole of each changed side.
#[derive(Debug, PartialEq, Eq)]
struct Counts {
    added: usize,
    removed: usize,
    approximate: bool,
}

/// The most lines on either side of an edit that Fidget diffs. Past it, a
/// diff costs more time than a count in a folded row is worth.
const MAX_DIFF_LINES: usize = 10_000;

/// Lines added and removed going from `old` to `new`, as `git diff --numstat`
/// counts them: only `\n` ends a line. A file with no `old` text is all added.
/// Lines the two share at the start and the end are skipped before the cap
/// applies, so a small edit in a big file stays exact.
fn line_counts(old: Option<&str>, new: &str) -> Counts {
    let old: Vec<&str> = old.unwrap_or("").split_inclusive('\n').collect();
    let new: Vec<&str> = new.split_inclusive('\n').collect();
    let head = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old = &old[head..old.len() - tail];
    let new = &new[head..new.len() - tail];
    if old.len() > MAX_DIFF_LINES || new.len() > MAX_DIFF_LINES {
        return Counts {
            added: new.len(),
            removed: old.len(),
            approximate: true,
        };
    }
    let (added, removed) = TextDiff::from_slices(old, new).iter_all_changes().fold(
        (0, 0),
        |(added, removed), change| match change.tag() {
            ChangeTag::Insert => (added + 1, removed),
            ChangeTag::Delete => (added, removed + 1),
            ChangeTag::Equal => (added, removed),
        },
    );
    Counts {
        added,
        removed,
        approximate: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::{Duration, Instant};

    /// Counts that are exact, as a pair.
    fn counts(old: Option<&str>, new: &str) -> (usize, usize) {
        let counted = line_counts(old, new);
        assert!(!counted.approximate, "{old:?} to {new:?} hit the cap");
        (counted.added, counted.removed)
    }

    #[test]
    fn a_changed_file_counts_the_lines_it_added_and_removed() {
        assert_eq!(counts(Some("a\nb\nc\nd\n"), "a\nB\nc\nd\ne\nf\n"), (3, 1));
    }

    #[test]
    fn a_new_file_is_all_added_and_an_emptied_one_all_removed() {
        assert_eq!(counts(None, "x\ny\nz\n"), (3, 0));
        assert_eq!(counts(Some("x\ny\n"), ""), (0, 2));
    }

    #[test]
    fn an_unchanged_file_adds_and_removes_nothing() {
        assert_eq!(counts(Some("a\nb\n"), "a\nb\n"), (0, 0));
        assert_eq!(counts(None, ""), (0, 0));
    }

    #[test]
    fn a_moved_line_counts_as_one_removed_and_one_added() {
        assert_eq!(counts(Some("a\nb\nc\n"), "b\nc\na\n"), (1, 1));
    }

    #[test]
    fn only_a_newline_ends_a_line() {
        assert_eq!(counts(Some("a\r\nb\r\n"), "a\r\nB\r\n"), (1, 1));
        assert_eq!(counts(Some("a\r\nb\n"), "a\nb\n"), (1, 1));
        assert_eq!(counts(Some("a\rb\n"), "a\nb\n"), (2, 1));
    }

    #[test]
    fn a_missing_final_newline_makes_the_last_line_a_different_line() {
        assert_eq!(counts(Some("a\nb"), "a\nb\n"), (1, 1));
        assert_eq!(counts(Some("a\nb\n"), "a\nb"), (1, 1));
        assert_eq!(counts(Some("a\nb"), "a\nb"), (0, 0));
    }

    const PAIRS: [(&str, &str); 10] = [
        ("a\nb\nc\nd\n", "a\nB\nc\nd\ne\nf\n"),
        ("a\nb\nc\n", "b\nc\na\n"),
        ("a\r\nb\r\n", "a\r\nB\r\n"),
        ("a\r\nb\n", "a\nb\n"),
        ("a\rb\n", "a\nb\n"),
        ("a\nb", "a\nb\n"),
        ("a\nb\n", "a\nb"),
        ("a\nb", "a\nB"),
        ("x\ny\n", ""),
        ("", "x\ny\nz\n"),
    ];

    /// The numbers must be the ones `gh` shows, which come from git. Git is
    /// on every runner, so a missing git fails this test.
    #[test]
    fn the_counts_are_the_ones_git_reports() {
        let dir = std::env::temp_dir().join(format!("fidget-numstat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (old, new) in PAIRS {
            std::fs::write(dir.join("old"), old).unwrap();
            std::fs::write(dir.join("new"), new).unwrap();
            let run = Command::new("git")
                .args([
                    "-c",
                    "core.autocrlf=false",
                    "diff",
                    "--no-index",
                    "--numstat",
                    "old",
                    "new",
                ])
                .current_dir(&dir)
                .output()
                .expect("git must be installed to run this test");
            let line = String::from_utf8(run.stdout).unwrap();
            let mut fields = line.split('\t');
            let from_git = match (fields.next(), fields.next()) {
                (Some(added), Some(removed)) => (added.parse().unwrap(), removed.parse().unwrap()),
                _ => (0, 0),
            };
            assert_eq!(counts(Some(old), new), from_git, "{old:?} to {new:?}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Past the cap the counts are the whole of each side and say so, and the
    /// answer comes without running the diff.
    #[test]
    fn a_rewrite_past_the_cap_is_counted_by_its_size_and_marked_approximate() {
        let old: String = (0..100_000).map(|n| format!("old {n}\n")).collect();
        let new: String = (0..100_000).map(|n| format!("new {n}\n")).collect();
        let started = Instant::now();
        let counted = line_counts(Some(&old), &new);
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(
            counted,
            Counts {
                added: 100_000,
                removed: 100_000,
                approximate: true
            }
        );
    }

    /// A big file with a small edit stays exact: the lines both versions
    /// share at the start and the end are not diffed.
    #[test]
    fn a_small_edit_in_a_big_file_is_exact() {
        let old: String = (0..100_000).map(|n| format!("line {n}\n")).collect();
        let new = old.replacen("line 50000\n", "edited\nline 50000\n", 1);
        assert_eq!(counts(Some(&old), &new), (1, 0));
    }

    /// The largest side `MAX_DIFF_LINES` lets through is diffed, so the
    /// counts are exact there and approximate one line over.
    #[test]
    fn the_cap_is_inclusive() {
        let side =
            |lead: &str, n: usize| -> String { (0..n).map(|i| format!("{lead} {i}\n")).collect() };
        let at = line_counts(
            Some(&side("old", MAX_DIFF_LINES)),
            &side("new", MAX_DIFF_LINES),
        );
        assert!(!at.approximate);
        let over = line_counts(Some(&side("old", MAX_DIFF_LINES + 1)), &side("new", 1));
        assert_eq!(
            over,
            Counts {
                added: 1,
                removed: MAX_DIFF_LINES + 1,
                approximate: true
            }
        );
    }
}
