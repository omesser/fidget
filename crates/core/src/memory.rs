//! Memory: the one Markdown file recording what the characters know about the user.
//! Shared by every Character Instance and owned by the user, who can read, edit
//! and wipe it. Headings are advisory: unparsable content is carried across untouched.
//!
//! Memory is untrusted in both directions: a Harness writes it, the user can type
//! anything into it, and it reaches Harness prompts. Nobody can make the content
//! safe; this module keeps one fact on one line so it cannot forge structure.
//!
//! No size guard, deliberately: Memory is user-owned, so auto-deletion or a write
//! refusal would break that contract and break Harness sessions unpredictably.
//! If a guard ever becomes necessary it is an explicit setting, not a silent failure.
//!
//! ponytail: every read goes to the file rather than to a cached copy, which is
//! what makes an external edit visible with no watcher and no reload path.
//! Recall runs at Harness tool-call rate, never in the frame loop, so a read per
//! call is cheaper than a watcher and the cache invalidation it would exist to
//! drive. Add the watcher when something has to *react* to an edit rather than
//! merely see it.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// The folder Memory and settings share, so both are in one place the user owns.
///
/// ponytail: `dirs::data_dir` can be missing in a container or a test with no
/// home, and then we fall back to `/tmp`. A reboot wipes that copy. The
/// shipped app always has an Application Support directory; keep the fallback
/// only for those environments, and drop it if a launch without a data dir
/// should refuse instead.
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("fidget")
}

/// User home, distinct from `data_dir`. Empty attach cwd is `data_dir` (#782);
/// naming home explicitly is a different project.
pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

/// Debug IPC command path for test scripts to place commands.
pub fn debug_cmd_path() -> Option<PathBuf> {
    Some(home_dir()?.join(".fidget-debug-cmd"))
}

/// Debug IPC result path where fidget writes responses.
pub fn debug_result_path() -> Option<PathBuf> {
    Some(home_dir()?.join(".fidget-debug-result"))
}

/// The one file every Instance and every Harness shares, named here rather than
/// by each caller so it cannot quietly become two paths, which would look like
/// a character that forgot. `FIDGET_MEMORY` overrides it for tests and the probe.
pub fn shared_path() -> PathBuf {
    match std::env::var_os("FIDGET_MEMORY") {
        Some(path) => PathBuf::from(path),
        None => data_dir().join("memory.md"),
    }
}

/// Memory on disk, at a path the user owns.
pub struct MemoryManifest {
    path: PathBuf,
    /// Held across a read-modify-write, so two writers cannot each read the same
    /// document and have the later rename drop the earlier one's fact.
    ///
    /// ponytail: one lock per manifest, which covers the several-Instances-one-
    /// process case the spec describes. Cross-process file locking if a second
    /// fidget ever writes the same Memory.
    writing: Mutex<()>,
}

impl MemoryManifest {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            writing: Mutex::new(()),
        }
    }

    /// Everything Memory holds, as the user would see it in an editor. Memory
    /// the user has never written is empty, not missing: a character that has learned
    /// nothing yet is a normal state, not an error.
    pub fn recall(&self) -> io::Result<String> {
        match fs::read_to_string(&self.path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
            result => result,
        }
    }

    /// Record one fact under `heading`, and report the line recorded: the manifest
    /// rewrites a fact onto one line, and the user is owed what landed in their
    /// file. Both arguments are Harness input, checked before the lock is taken.
    pub fn remember(&self, heading: &str, fact: &str) -> io::Result<String> {
        let heading = non_empty("heading", heading)?;
        let recorded = format!("- {}", non_empty("fact", fact)?);
        let _writing = self.lock();
        self.write(with_fact(&self.recall()?, &heading, &recorded))?;
        Ok(recorded)
    }

    /// Empty Memory, keeping one backup beside it; `None` when there was nothing
    /// worth backing up. The backup is written first and its failure aborts the
    /// wipe, because a wipe the user did not mean cannot be undone.
    pub fn wipe(&self) -> io::Result<Option<PathBuf>> {
        let _writing = self.lock();
        let memory = self.recall()?;
        if memory.trim().is_empty() {
            self.write(String::new())?;
            return Ok(None);
        }

        let backup = backup_path(&self.path);
        fs::write(&backup, &memory)?;
        keep_permissions_of(&self.path, &backup)?;
        self.write(String::new())?;
        Ok(Some(backup))
    }

    /// Exclude every other writer for as long as the guard lives. A poisoned lock
    /// is taken anyway: it guards a file rather than an in-memory invariant, so
    /// one writer's panic must not wedge Memory for the rest of the session.
    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.writing.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Replace Memory's contents, creating its directory on first use. Written
    /// beside Memory and renamed over it, so Memory is the old file or the new
    /// one, never a half-written one; only a wipe leaves a backup to recover from.
    fn write(&self, contents: String) -> io::Result<()> {
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let scratch = scratch_path(&self.path);

        if let Err(e) = fs::write(&scratch, contents)
            .and_then(|()| keep_permissions_of(&self.path, &scratch))
            .and_then(|()| fs::rename(&scratch, &self.path))
        {
            let _ = fs::remove_file(&scratch);
            return Err(e);
        }
        Ok(())
    }
}

/// Where one write's scratch file lives: beside Memory, so the publishing rename
/// stays within one filesystem, and named per write rather than per process, or
/// one writer's rename publishes the partial bytes another is still writing.
fn scratch_path(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut name = path
        .file_name()
        .unwrap_or(OsStr::new("memory"))
        .to_os_string();
    name.push(format!(
        ".{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    path.with_file_name(name)
}

/// Give `fresh` the permissions `path` has, if `path` is there at all. A new
/// file arrives with whatever the umask says, and Memory holds what the characters
/// know about the user, so narrowing who reads it must survive a write and a wipe.
fn keep_permissions_of(path: &Path, fresh: &Path) -> io::Result<()> {
    match fs::metadata(path) {
        Ok(existing) => fs::set_permissions(fresh, existing.permissions()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Where the backup of `path` lives: beside Memory and with Memory's own
/// extension, so the user finds it in the same folder and it opens as Markdown.
///
/// ponytail: seconds since the epoch rather than a civil timestamp. It sorts
/// correctly and costs no date library; swap it for an ISO stamp if one ever
/// arrives for another reason. Two wipes in the same second share a name, and
/// the later one wins.
fn backup_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .unwrap_or(OsStr::new("memory"))
        .to_string_lossy();
    let stamp = epoch_seconds();
    let name = match path.extension() {
        Some(ext) => format!("{stem}-backup-{stamp}.{}", ext.to_string_lossy()),
        None => format!("{stem}-backup-{stamp}"),
    };
    path.with_file_name(name)
}

/// Seconds since the Unix epoch, or 0 if the system clock predates it.
fn epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The document that results from recording `fact` under `heading`. Pure, so
/// the Markdown handling is testable without a disk. Every other line is carried
/// across untouched: the file is the user's, and only they know what it means.
fn with_fact(document: &str, heading: &str, fact_line: &str) -> String {
    let heading_line = format!("## {}", one_line(heading));
    let mut lines: Vec<&str> = document.lines().collect();

    match lines.iter().position(|line| names(line, heading)) {
        // Append at the end of the section, past its blank tail, so the fact
        // lands under its own heading rather than at the end of the file.
        Some(start) => {
            let mut end = lines[start + 1..]
                .iter()
                .position(|line| is_heading(line))
                .map_or(lines.len(), |offset| start + 1 + offset);
            while end > start + 1 && lines[end - 1].trim().is_empty() {
                end -= 1;
            }
            lines.insert(end, fact_line);
        }
        None => {
            if !lines.is_empty() {
                lines.push("");
            }
            lines.push(&heading_line);
            lines.push("");
            lines.push(fact_line);
        }
    }

    // A file that does not end in a newline would otherwise glue the next fact
    // onto the user's last line.
    lines.join("\n") + "\n"
}

/// Whether `line` is an ATX heading of any level.
fn is_heading(line: &str) -> bool {
    line.starts_with('#')
}

/// Whether `line` is the heading called `heading`. Both sides are normalized the
/// way a heading is written, or a hand-typed `# facts` or a `Daily  Facts` a
/// Harness spaced its own way would quietly split the user's Memory in two.
fn names(line: &str, heading: &str) -> bool {
    is_heading(line)
        && one_line(line.trim_start_matches('#')).eq_ignore_ascii_case(&one_line(heading))
}

/// `text` collapsed onto one line, or `InvalidInput` if there is nothing in it:
/// a bare `- ` is litter in a file the user is meant to read, and a bare `## `
/// is a section nothing can name.
fn non_empty(label: &str, text: &str) -> io::Result<String> {
    match one_line(text) {
        text if text.is_empty() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("a {label} with nothing in it cannot be remembered"),
        )),
        text => Ok(text),
    }
}

/// Collapse `text` onto one line. A newline inside a fact would forge a heading
/// or a bullet of its own.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn memory_lives_in_the_user_data_dir() {
        assert!(
            data_dir().ends_with("fidget"),
            "Application Support/fidget, not a temp folder a reboot wipes"
        );
        assert_eq!(
            data_dir().join("memory.md").file_name().unwrap(),
            "memory.md"
        );
    }

    #[test]
    fn home_dir_is_not_the_data_folder() {
        let Some(home) = home_dir() else {
            return;
        };
        assert_ne!(
            home,
            data_dir(),
            "empty attach cwd is data_dir, not home (#782)"
        );
    }

    #[test]
    fn a_remembered_fact_round_trips_through_the_file() {
        let dir = tempfile::tempdir().expect("temp dir is creatable");
        let manifest = MemoryManifest::new(dir.path().join("memory.md"));

        manifest
            .remember("Facts", "Oded's cat is called Simba")
            .expect("remembering writes");

        let memory = manifest.recall().expect("recall reads back");
        assert!(
            memory.contains("Oded's cat is called Simba"),
            "the fact survives the file: {memory}"
        );
        assert!(
            memory.contains("## Facts"),
            "and sits under its heading: {memory}"
        );
    }

    /// The user edits Memory in their own editor while fidget is running. The
    /// same manifest, never re-created, has to see it.
    #[test]
    fn an_external_edit_is_picked_up_without_restarting() {
        let dir = tempfile::tempdir().expect("temp dir is creatable");
        let path = dir.path().join("memory.md");
        let manifest = MemoryManifest::new(&path);

        manifest
            .remember("Facts", "Oded lives in Tel Aviv")
            .expect("remembering writes");
        fs::write(&path, "## Facts\n\n- Oded lives in Haifa\n").expect("the user edits the file");

        let memory = manifest.recall().expect("recall reads back");
        assert!(
            memory.contains("Oded lives in Haifa"),
            "the correction is visible: {memory}"
        );
        assert!(
            !memory.contains("Tel Aviv"),
            "and what the user deleted is gone: {memory}"
        );
    }

    /// Nothing about the file may require fidget to have created it.
    #[test]
    fn a_hand_written_file_loads_and_is_appended_to_in_place() {
        let hand_written = "\
# What the characters know

Typed by me, before fidget ever ran.

## Facts

- Oded's cat is called Simba

## Preferences

- Dark mode, always
";
        let dir = tempfile::tempdir().expect("temp dir is creatable");
        let path = dir.path().join("memory.md");
        fs::write(&path, hand_written).expect("the user writes the file first");
        let manifest = MemoryManifest::new(&path);

        assert_eq!(
            manifest.recall().expect("recall reads back"),
            hand_written,
            "an untouched hand-written file reads back exactly as written"
        );

        manifest
            .remember("Facts", "Simba is ginger")
            .expect("remembering writes");

        assert_eq!(
            manifest.recall().expect("recall reads back"),
            "\
# What the characters know

Typed by me, before fidget ever ran.

## Facts

- Oded's cat is called Simba
- Simba is ginger

## Preferences

- Dark mode, always
",
            "the fact joins the user's own Facts section, and nothing else moves"
        );
    }

    /// A bad hand-edit has to degrade, not break: prose under no heading, a
    /// heading at the wrong level with the wrong case and no blank line after
    /// it, and no newline at the end of the file.
    #[test]
    fn malformed_content_still_loads_and_keeps_what_it_can() {
        let malformed = "\
Some notes I typed at the top, under no heading at all.
- Oded's cat is called Simba

###   preferences
- Dark mode, always";
        let dir = tempfile::tempdir().expect("temp dir is creatable");
        let path = dir.path().join("memory.md");
        fs::write(&path, malformed).expect("the user writes the file first");
        let manifest = MemoryManifest::new(&path);

        assert_eq!(
            manifest.recall().expect("recall reads back"),
            malformed,
            "malformed Memory still loads, exactly as it is"
        );

        manifest
            .remember("Preferences", "Uses a 2x display")
            .expect("remembering writes");

        assert_eq!(
            manifest.recall().expect("recall reads back"),
            "\
Some notes I typed at the top, under no heading at all.
- Oded's cat is called Simba

###   preferences
- Dark mode, always
- Uses a 2x display
",
            "every original line survives, and the section is found despite level and case"
        );
    }

    #[test]
    fn wipe_writes_a_backup_before_clearing() {
        let dir = tempfile::tempdir().expect("temp dir is creatable");
        let path = dir.path().join("memory.md");
        let manifest = MemoryManifest::new(&path);
        manifest
            .remember("Facts", "Oded's cat is called Simba")
            .expect("remembering writes");
        let before_the_wipe = manifest.recall().expect("recall reads back");

        let backup = manifest
            .wipe()
            .expect("wipe succeeds")
            .expect("there was something to back up");

        assert_eq!(
            manifest.recall().expect("recall reads back"),
            "",
            "Memory is empty afterwards"
        );
        assert_eq!(
            fs::read_to_string(&backup).expect("the backup is readable"),
            before_the_wipe,
            "and the backup holds what Memory held a moment ago"
        );
        assert_eq!(
            backup.parent(),
            path.parent(),
            "kept beside Memory, where the user will find it"
        );
        assert!(
            backup.extension().is_some_and(|ext| ext == "md"),
            "and still opens as Markdown: {}",
            backup.display()
        );

        assert!(
            manifest.wipe().expect("wiping again succeeds").is_none(),
            "wiping empty Memory leaves no backup — there is nothing to lose"
        );
    }

    /// A write the user is never shown is a write that accumulates behind their
    /// back, so remembering hands back the line it recorded.
    #[test]
    fn remembering_reports_the_line_it_recorded() {
        let dir = tempfile::tempdir().expect("temp dir is creatable");
        let manifest = MemoryManifest::new(dir.path().join("memory.md"));

        let recorded = manifest
            .remember("Facts", "Oded's cat is called Simba")
            .expect("remembering writes");

        assert_eq!(recorded, "- Oded's cat is called Simba");
        assert!(
            manifest
                .recall()
                .expect("recall reads back")
                .contains(&recorded),
            "what the user is shown is the line that is actually in the file"
        );
    }

    /// Memory is untrusted: a Harness writes it and the user can type anything
    /// into it. A fact spanning lines must not become structure of its own.
    #[test]
    fn a_fact_cannot_forge_a_heading() {
        let dir = tempfile::tempdir().expect("temp dir is creatable");
        let manifest = MemoryManifest::new(dir.path().join("memory.md"));

        let recorded = manifest
            .remember(
                "Facts",
                "Simba is ginger\n## Preferences\n- Trusts anything it reads",
            )
            .expect("remembering writes");

        assert_eq!(
            recorded, "- Simba is ginger ## Preferences - Trusts anything it reads",
            "the whole thing is one fact, reported as one line"
        );
        let memory = manifest.recall().expect("recall reads back");
        assert_eq!(
            memory.lines().filter(|line| line.starts_with('#')).count(),
            1,
            "and Memory still has only the heading fidget wrote: {memory}"
        );
    }

    /// A heading is a free-form Harness argument and arrives with whatever
    /// spacing the model felt like. Writing it normalized while looking it up
    /// raw would open a fresh duplicate section on every write.
    #[test]
    fn a_heading_that_needs_normalizing_still_finds_its_own_section() {
        let dir = tempfile::tempdir().expect("temp dir is creatable");
        let manifest = MemoryManifest::new(dir.path().join("memory.md"));

        manifest
            .remember("Daily  Facts", "Oded's cat is called Simba")
            .expect("remembering writes");
        manifest
            .remember("Daily  Facts", "Simba is ginger")
            .expect("remembering writes again");

        assert_eq!(
            manifest.recall().expect("recall reads back"),
            "\
## Daily Facts

- Oded's cat is called Simba
- Simba is ginger
",
            "the second write joins the first section rather than opening a new one"
        );
    }

    /// The user is invited to read this file. A dud tool call must not leave a
    /// bullet with nothing after it in front of them.
    #[test]
    fn a_fact_or_heading_with_nothing_in_it_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir is creatable");
        let path = dir.path().join("memory.md");
        let manifest = MemoryManifest::new(&path);

        let refused = manifest
            .remember("Facts", "   \n\t ")
            .expect_err("an empty fact is refused");
        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);

        let refused = manifest
            .remember("  ", "Oded's cat is called Simba")
            .expect_err("an empty heading is refused");
        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);

        assert!(
            !path.exists(),
            "and a refused write leaves the user's file alone"
        );
    }

    /// Every Instance writes Memory from one process. A scratch file named for
    /// the process rather than the write is one path two writers both truncate,
    /// so one writer's rename can publish the other's half-written bytes.
    #[test]
    fn two_writes_never_share_one_scratch_file() {
        let path = Path::new("/memories/memory.md");
        let first = scratch_path(path);
        let second = scratch_path(path);

        assert_ne!(
            first, second,
            "each write gets a scratch file of its own, so no writer can truncate another's"
        );
        for scratch in [&first, &second] {
            assert_eq!(
                scratch.parent(),
                path.parent(),
                "written beside Memory, so the rename stays within one filesystem"
            );
            assert_ne!(scratch, path, "and never over Memory itself");
        }
    }

    /// The user keeps Memory open in an editor, and a crash mid-write must not
    /// truncate it. Replacing the file by rename is what makes that impossible;
    /// a second name for the old file is how a test can tell which one happened.
    #[test]
    fn a_write_replaces_memory_rather_than_writing_over_it_in_place() {
        let dir = tempfile::tempdir().expect("temp dir is creatable");
        let path = dir.path().join("memory.md");
        let manifest = MemoryManifest::new(&path);
        manifest
            .remember("Facts", "Oded's cat is called Simba")
            .expect("remembering writes");
        let before = manifest.recall().expect("recall reads back");

        let same_file = dir.path().join("still-the-old-one.md");
        fs::hard_link(&path, &same_file).expect("a second name for the same file");
        manifest
            .remember("Facts", "Simba is ginger")
            .expect("remembering writes again");

        assert_eq!(
            fs::read_to_string(&same_file).expect("the old file is readable"),
            before,
            "the file that was there is left whole, never truncated and rewritten"
        );
        assert!(
            manifest
                .recall()
                .expect("recall reads back")
                .contains("Simba is ginger"),
            "while Memory itself has the new fact"
        );

        let mut left_behind: Vec<String> =
            fs::read_dir(path.parent().expect("Memory has a folder"))
                .expect("the folder is readable")
                .map(|entry| {
                    entry
                        .expect("a directory entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
        left_behind.sort();
        assert_eq!(
            left_behind,
            ["memory.md", "still-the-old-one.md"],
            "and no scratch file is left in the folder the user reads"
        );
    }

    /// If the user narrowed who can read Memory, replacing the file must not
    /// hand that back: a new file starts from the umask. Unix only, since the
    /// claim is about POSIX mode bits, which Windows has no equivalent for.
    #[cfg(unix)]
    #[test]
    fn a_write_keeps_the_permissions_the_user_set() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir is creatable");
        let path = dir.path().join("memory.md");
        let manifest = MemoryManifest::new(&path);
        manifest
            .remember("Facts", "Oded's cat is called Simba")
            .expect("remembering writes");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .expect("the user narrows who can read Memory");

        manifest
            .remember("Facts", "Simba is ginger")
            .expect("remembering writes again");

        assert_eq!(
            fs::metadata(&path)
                .expect("Memory is there")
                .permissions()
                .mode()
                & 0o777,
            0o600,
            "Memory is still readable only by the user who narrowed it"
        );
    }

    /// The backup holds exactly what Memory held, and if the user narrowed who
    /// can read Memory the copy beside it has to be just as narrow. Unix only,
    /// for the reason the test above gives.
    #[cfg(unix)]
    #[test]
    fn a_backup_keeps_the_permissions_the_user_set() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir is creatable");
        let path = dir.path().join("memory.md");
        let manifest = MemoryManifest::new(&path);
        manifest
            .remember("Facts", "Oded's cat is called Simba")
            .expect("remembering writes");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .expect("the user narrows who can read Memory");

        let backup = manifest
            .wipe()
            .expect("wipe succeeds")
            .expect("there was something to back up");

        assert_eq!(
            fs::metadata(&backup)
                .expect("the backup is there")
                .permissions()
                .mode()
                & 0o777,
            0o600,
            "the backup is as private as the Memory it copies"
        );
    }

    /// The user picks the path, so Memory need not be called memory.md or carry
    /// an extension at all. The backup still has to be findable beside it.
    #[test]
    fn a_backup_is_named_after_memory_even_without_an_extension() {
        let dir = tempfile::tempdir().expect("temp dir is creatable");
        let manifest = MemoryManifest::new(dir.path().join("notes"));
        manifest
            .remember("Facts", "Oded's cat is called Simba")
            .expect("remembering writes");

        let backup = manifest
            .wipe()
            .expect("wipe succeeds")
            .expect("there was something to back up");
        let name = backup
            .file_name()
            .expect("the backup has a name")
            .to_string_lossy()
            .into_owned();

        let stamp = name
            .strip_prefix("notes-backup-")
            .unwrap_or_else(|| panic!("named after Memory: {name}"));
        assert!(
            !stamp.is_empty() && stamp.chars().all(|c| c.is_ascii_digit()),
            "and stamped with when it was taken: {name}"
        );
    }

    /// Memory is shared by every Character Instance and they write it from one
    /// process. Two characters recording at the same moment must both land: a write
    /// `remember` reported and the file does not hold is a lie to the user.
    #[test]
    fn concurrent_remembers_all_land() {
        let dir = tempfile::tempdir().expect("temp dir is creatable");
        let manifest = &MemoryManifest::new(dir.path().join("memory.md"));

        std::thread::scope(|scope| {
            for writer in 0..4 {
                scope.spawn(move || {
                    for i in 0..25 {
                        manifest
                            .remember("Facts", &format!("fact {writer}-{i}"))
                            .expect("remembering writes");
                    }
                });
            }
        });

        let memory = manifest.recall().expect("recall reads back");
        assert_eq!(
            memory
                .lines()
                .filter(|line| line.starts_with("- fact"))
                .count(),
            100,
            "every fact remember reported is in the file: {memory}"
        );
    }
}
