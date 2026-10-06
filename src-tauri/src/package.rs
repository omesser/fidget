//! Where Character Packages live, and how their bytes are got off a disk.
//!
//! `character::load` is a pure function from bytes to either a validated
//! Character or a list of an author's mistakes. Something has to open a
//! directory or an archive and hand it those bytes, and that something performs
//! I/O, so it belongs in the Shell rather than in `fidget-core` — the same
//! split as `WindowSource`.
//!
//! The two ways a location can fail to yield bytes are kept apart, because
//! they are two different problems for whoever has to fix them:
//!
//! - **Not a package** — this location holds no Character Manifest. A user
//!   pointed at their Downloads folder.
//! - **Unreadable** — the bytes could not be got at all: permissions, a
//!   truncated archive, a path that is not there.
//!
//! Whether the bytes are a Character is `character::load`'s answer alone, and
//! its rejections are the author's list of things to fix.
//!
//! A Character Package is untrusted input, so the reader is bounded before it
//! is convenient: a package cannot make fidget read an unbounded number of
//! files, allocate an unbounded number of bytes, or walk an unbounded depth.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use fidget_core::character::{PackageBytes, CHARACTER_MANIFEST_FILE};

/// The file extension of a packaged Character. Zip because it is what a person
/// gets from Finder's "Compress", not because the format needs a container.
const ARCHIVE_EXTENSION: &str = "zip";

/// The most bytes a package may expand to, across every file in it.
///
/// Without this a few kilobytes of zip can ask for every byte of memory. Generous for a mascot.
const MAX_PACKAGE_BYTES: u64 = 64 * 1024 * 1024;

/// The most files a package may contain. Frames, a manifest, a prompt, and room
/// to spare for art nothing declares.
const MAX_PACKAGE_FILES: usize = 4096;

/// How many directories deep the reader will walk into a package, counting the
/// package root as the first. Anything nested deeper is refused rather than
/// walked.
const MAX_PACKAGE_DEPTH: usize = 8;

/// Directories added in front of the user directory and the shipped packages.
///
/// `:`-separated on macOS and Linux, `;`-separated on Windows. For development
/// and the verification script; a user never needs it.
pub const SEARCH_PATH_VAR: &str = "FIDGET_CHARACTERS";

/// The Character a new user meets, when nothing has chosen another.
///
/// Name order is not a decision: without this, adding a package that sorts earlier would silently replace the fidget everybody sees.
pub const DEFAULT_CHARACTER: &str = "buddy-bot";

/// The environment variable that starts one named Character rather than the first one found.
///
/// The search takes the first package that loads, so without this name order alone decides which Character a developer sees.
pub const CHARACTER_VAR: &str = "FIDGET_CHARACTER";

/// Why a location did not yield a package's bytes.
#[derive(Debug)]
pub enum ReadError {
    /// There is no Character Manifest here, so this was never a package.
    NotAPackage(PathBuf),
    /// The bytes could not be read at all.
    Unreadable { path: PathBuf, why: String },
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAPackage(path) => write!(
                f,
                "{} is not a Character Package: it contains no {CHARACTER_MANIFEST_FILE}",
                path.display()
            ),
            Self::Unreadable { path, why } => {
                write!(f, "{} could not be read: {why}", path.display())
            }
        }
    }
}

impl std::error::Error for ReadError {}

/// Read a Character Package's bytes from a directory or an archive.
///
/// Bytes, not a Character: whether they are one is `character::load`'s answer.
pub fn read(path: &Path) -> Result<PackageBytes, ReadError> {
    let unreadable = |why: String| ReadError::Unreadable {
        path: path.to_path_buf(),
        why,
    };

    let metadata = fs::metadata(path).map_err(|e| unreadable(e.to_string()))?;
    let files = if metadata.is_dir() {
        read_directory(path)
    } else {
        read_archive(path)
    }
    .map_err(unreadable)?;

    let files = strip_single_root(files);
    if !files.contains_key(CHARACTER_MANIFEST_FILE) {
        return Err(ReadError::NotAPackage(path.to_path_buf()));
    }

    Ok(files)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Desktop {
    // The other two are what `host` returns on that OS. On this one they are
    // constructed only from tests, which is what checks the list without that machine.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Macos,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Linux,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    Windows,
}

impl Desktop {
    fn host() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }

    /// A colon is a drive letter on Windows.
    fn separator(self) -> u8 {
        match self {
            Self::Windows => b';',
            Self::Macos | Self::Linux => b':',
        }
    }
}

struct PackageRoots<'a> {
    extra: Option<&'a OsStr>,
    data_dir: &'a Path,
    xdg_data_dirs: Option<&'a OsStr>,
    desktop: Desktop,
}

/// Unset or empty `$XDG_DATA_DIRS` means these two, per the XDG base spec.
const XDG_DATA_DIRS_DEFAULT: &[&str] = &["/usr/local/share", "/usr/share"];

/// Where a user adds a Character Package, beside settings and Memory.
pub fn user_characters_dir() -> PathBuf {
    fidget_core::memory::data_dir().join("characters")
}

/// Where fidget looks for Character Packages, in the order it looks.
///
/// `FIDGET_CHARACTERS` is added in front, the way `PATH` is. Earlier wins.
/// Replacing the other directories hides the user folder and the shipped packages.
pub fn search_paths(bundled: Option<PathBuf>) -> Vec<PathBuf> {
    let data_dir = fidget_core::memory::data_dir();
    let extra = std::env::var_os(SEARCH_PATH_VAR);
    let xdg_data_dirs = std::env::var_os("XDG_DATA_DIRS");
    search_paths_from(
        bundled,
        PackageRoots {
            extra: extra.as_deref(),
            data_dir: &data_dir,
            xdg_data_dirs: xdg_data_dirs.as_deref(),
            desktop: Desktop::host(),
        },
    )
}

fn search_paths_from(bundled: Option<PathBuf>, roots: PackageRoots<'_>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(extra) = roots.extra {
        paths.extend(split_list(extra, roots.desktop.separator()));
    }
    paths.push(roots.data_dir.join("characters"));
    if roots.desktop == Desktop::Linux {
        // An XDG base is `/usr/share`, not `.../fidget`, so both components are added.
        for base in xdg_bases(roots.xdg_data_dirs) {
            paths.push(base.join("fidget").join("characters"));
        }
    }
    paths.extend(bundled);
    paths
}

fn xdg_bases(raw: Option<&OsStr>) -> Vec<PathBuf> {
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return XDG_DATA_DIRS_DEFAULT
            .iter()
            .copied()
            .map(PathBuf::from)
            .collect();
    };
    // A relative entry is not a base. Windows does not call `/usr/share` absolute.
    split_list(raw, b':')
        .into_iter()
        .filter(|path| {
            path.is_absolute()
                || path
                    .as_os_str()
                    .to_str()
                    .is_some_and(|text| text.starts_with('/'))
        })
        .collect()
}

fn split_list(raw: &OsStr, separator: u8) -> Vec<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        raw.as_bytes()
            .split(|byte| *byte == separator)
            .filter(|part| !part.is_empty())
            .map(|part| PathBuf::from(OsStr::from_bytes(part)))
            .collect()
    }
    #[cfg(windows)]
    {
        use std::ffi::OsString;
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let sep = u16::from(separator);
        let wide: Vec<u16> = raw.encode_wide().collect();
        wide.split(|unit| *unit == sep)
            .filter(|part| !part.is_empty())
            .map(|part| PathBuf::from(OsString::from_wide(part)))
            .collect()
    }
}

/// The candidates named `wanted`, or all of them when nothing is named.
///
/// Naming a Character that is not installed leaves nothing rather than falling through: starting some other Character is a worse answer than saying so.
pub fn named(candidates: Vec<PathBuf>, wanted: Option<&OsStr>) -> Vec<PathBuf> {
    let Some(wanted) = wanted else {
        return candidates;
    };

    candidates
        .into_iter()
        .filter(|candidate| candidate.file_stem() == Some(wanted))
        .collect()
}

/// The candidates with `first` at the front, in the order given otherwise.
///
/// Ordering rather than filtering, so a broken default costs the Character they expected, not the app.
pub fn preferring(candidates: Vec<PathBuf>, first: &str) -> Vec<PathBuf> {
    let (preferred, rest): (Vec<PathBuf>, Vec<PathBuf>) = candidates
        .into_iter()
        .partition(|candidate| candidate.file_stem() == Some(OsStr::new(first)));

    preferred.into_iter().chain(rest).collect()
}

/// Every Character Package visible in `search_paths`, in the order found.
///
/// A candidate rather than a Character: this only says a directory or an archive is here.
pub fn installed(search_paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut packages = Vec::new();

    for directory in search_paths {
        let Ok(entries) = fs::read_dir(directory) else {
            continue; // a search path that does not exist is not an error
        };
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_dir()
                    || path
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case(ARCHIVE_EXTENSION))
            })
            .collect();
        found.sort();
        packages.extend(found);
    }

    packages
}

/// Every file under `root`, keyed by its path relative to `root` with `/`
/// separators — the names a Character Manifest writes.
fn read_directory(root: &Path) -> Result<PackageBytes, String> {
    let mut files = PackageBytes::new();
    let mut budget = Budget::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];

    while let Some((directory, depth)) = pending.pop() {
        if depth >= MAX_PACKAGE_DEPTH {
            return Err(format!(
                "the package nests deeper than {MAX_PACKAGE_DEPTH} directories"
            ));
        }

        let entries = fs::read_dir(&directory)
            .map_err(|e| format!("{} could not be listed: {e}", directory.display()))?;

        for entry in entries {
            let entry =
                entry.map_err(|e| format!("{} could not be read: {e}", directory.display()))?;
            let path = entry.path();

            // `symlink_metadata` rather than `metadata`: a link is described as
            // itself, so a package cannot reach outside its own directory by
            // pointing at somewhere else on the disk.
            let metadata = path
                .symlink_metadata()
                .map_err(|e| format!("{} could not be read: {e}", path.display()))?;

            if metadata.is_dir() {
                pending.push((path, depth + 1));
                continue;
            }
            if !metadata.is_file() {
                continue; // a symlink, a socket, a device: not art and not a manifest
            }

            budget.charge(metadata.len())?;

            let Some(name) = relative_name(root, &path) else {
                continue;
            };
            let bytes = fs::read(&path)
                .map_err(|e| format!("{} could not be read: {e}", path.display()))?;
            files.insert(name, bytes);
        }
    }

    Ok(files)
}

fn read_archive(path: &Path) -> Result<PackageBytes, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("not a readable archive: {e}"))?;

    let mut files = PackageBytes::new();
    let mut budget = Budget::new();

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("entry {index} could not be read: {e}"))?;
        if !entry.is_file() {
            continue;
        }

        // `enclosed_name` refuses absolute paths and `..`, so a hostile archive
        // cannot name a file outside the package. The names it yields are the
        // ones the manifest resolves against.
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let Some(name) = name.to_str().map(|name| name.replace('\\', "/")) else {
            continue;
        };

        if is_macos_litter(&name) {
            continue;
        }

        // Charged against the size the archive advertises, before a byte is
        // decompressed, so a small archive claiming a huge entry is refused
        // rather than allocated for.
        let allowed = budget.charge(entry.size())?;

        // Read under that same cap: the header is the archive's own claim.
        // An entry declaring one byte and inflating to a gigabyte is a zip bomb,
        // and `read_to_end` would follow it all the way down.
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(allowed + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("{name} could not be read: {e}"))?;
        if bytes.len() as u64 > allowed {
            return Err(format!(
                "{name} decompresses to more than the {allowed} bytes it declares"
            ));
        }
        files.insert(name, bytes);
    }

    Ok(files)
}

/// Whether an archive entry is macOS bookkeeping rather than package content.
///
/// Finder's `__MACOSX/` and `.DS_Store` describe the Mac, not the Character; dropping `__MACOSX/` also leaves one top-level directory for `strip_single_root`.
fn is_macos_litter(name: &str) -> bool {
    name.starts_with("__MACOSX/") || name == ".DS_Store" || name.ends_with("/.DS_Store")
}

/// What a package has left to spend. Each reader keeps its own.
///
/// One piece of arithmetic rather than one per reader, so the two bounds stay agreed.
struct Budget {
    bytes: u64,
    files: usize,
}

impl Budget {
    fn new() -> Self {
        Self {
            bytes: MAX_PACKAGE_BYTES,
            files: MAX_PACKAGE_FILES,
        }
    }

    /// Charge one file of `size` bytes, or refuse the package.
    ///
    /// Returns how many bytes remain so a reader that cannot trust an archive header can cap decompression.
    fn charge(&mut self, size: u64) -> Result<u64, String> {
        if self.files == 0 {
            return Err(format!(
                "the package holds more than {MAX_PACKAGE_FILES} files"
            ));
        }
        if size > self.bytes {
            return Err(format!(
                "the package expands to more than {} MiB",
                MAX_PACKAGE_BYTES / (1024 * 1024)
            ));
        }
        self.files -= 1;
        self.bytes -= size;
        Ok(size)
    }
}

fn relative_name(root: &Path, file: &Path) -> Option<String> {
    let relative = file.strip_prefix(root).ok()?;
    let parts: Vec<&str> = relative
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()?;
    Some(parts.join("/"))
}

/// Drop a single wrapping directory, if that is all the package has at its top.
///
/// `zip -r` and Finder's Compress wrap files under one folder; without this, directory and archive would not load identically.
fn strip_single_root(files: PackageBytes) -> PackageBytes {
    if files.contains_key(CHARACTER_MANIFEST_FILE) {
        return files;
    }

    let mut roots = files.keys().filter_map(|name| name.split_once('/'));
    let Some((root, _)) = roots.next() else {
        return files;
    };
    let root = root.to_string();
    if files
        .keys()
        .any(|name| name.split_once('/').map(|(r, _)| r) != Some(root.as_str()))
    {
        return files;
    }

    files
        .into_iter()
        .filter_map(|(name, bytes)| {
            name.strip_prefix(&format!("{root}/"))
                .map(|stripped| (stripped.to_string(), bytes))
        })
        .collect::<BTreeMap<_, _>>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::io::Write;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{Duration, UNIX_EPOCH};

    use fidget_core::character::{self, Character};
    use fidget_core::director::{Context, StaticDirector};
    use fidget_core::engine::{BehaviorProposal, Engine, Point, Rect, WorldSnapshot};
    use fidget_core::overlay::{AlphaMask, SpriteRect};
    use fidget_core::sensing::Activity;

    /// A 2x2 RGBA PNG, which is all the loader asks of a frame.
    const FRAME: &[u8] = include_bytes!("../../crates/core/tests/fixtures/alpha-2x2.png");

    /// A directory of our own under the system temp dir, removed when the test
    /// ends. A handful of lines rather than a dev-dependency, as in `memory`.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let unique = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join(format!("fidget-{label}-{}-{unique}", std::process::id()));
            fs::create_dir_all(&dir).expect("temp dir is creatable");
            Self(dir)
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Shipped Characters from the repo. Sample idles inside each `when` phase so a
    /// seam at a sample duration does not skip it. Read `weight` directly so a
    /// balance cannot go flat.
    #[test]
    fn every_shipped_character_loads_and_has_a_life() {
        let shipped = [
            ("black-mage", "Black Mage", "ponder", 40, "meditate"),
            ("bmo", "BMO", "report", 30, "patrol"),
            ("cat", "Cat", "inspect", 40, "nap"),
            ("jotaro-kujo", "Jotaro Kujo", "stand", 50, "rest"),
            ("nim", "Nim", "doze", 50, "doze"),
            ("buddy-bot", "Buddy Bot", "greet", 40, "nap"),
            ("timber-wolf", "Timber Wolf", "patrol", 30, "power_down"),
            ("trump", "Trump", "report", 40, "doze"),
        ];

        for (directory, name, behavior, weight, alone) in shipped {
            let character = shipped_character(directory);

            assert_eq!(character.name, name);
            assert_eq!(
                character
                    .behaviors
                    .get(behavior)
                    .map(|declared| declared.weight),
                Some(weight),
                "the declared balance is {name}'s, not the default one"
            );

            for idle in [0, 5, 15, 45, 90, 300, 3600].map(Duration::from_secs) {
                assert!(
                    !proposable(&character, idle).is_empty(),
                    "{name} has something to do {idle:?} after the user stopped typing"
                );
            }

            assert!(
                !proposable(&character, Duration::from_secs(0)).contains(alone),
                "{name} keeps {alone:?} for an empty desk rather than proposing it \
                 at somebody who is typing"
            );
            assert!(
                !proposable(&character, Duration::from_secs(1800)).contains("greet"),
                "{name} does not greet somebody who left half an hour ago"
            );
        }
    }

    /// Comparative claims a single weight cannot pin: Jotaro stands more than
    /// he speaks, Cat leads with curiosity.
    #[test]
    fn a_rewritten_balance_says_what_its_manifest_claims() {
        let jotaro = shipped_character("jotaro-kujo");
        let talking: u32 = ["greet", "mutter"]
            .iter()
            .map(|behavior| jotaro.behaviors[*behavior].weight)
            .sum();
        assert!(
            jotaro.behaviors["stand"].weight > talking,
            "Jotaro stands for longer than he opens his mouth"
        );

        let cat = shipped_character("cat");
        assert_eq!(
            ["inspect", "remark", "greet"]
                .iter()
                .max_by_key(|behavior| cat.behaviors[**behavior].weight),
            Some(&"inspect"),
            "curiosity outweighs the rest of what Cat does with somebody there"
        );
    }

    /// Every Behavior a Character will propose at one idle duration, drawn over
    /// enough wakes that a weighted one is not missed, with nothing remembered
    /// so that recency suppression hides none of them.
    fn proposable(character: &Character, idle: Duration) -> BTreeSet<String> {
        let mut director = StaticDirector::new(character.behaviors.clone(), 1);
        let moment = Context {
            activity: Activity {
                frontmost_application: Some("Terminal".to_string()),
                idle,
                ..Activity::quiet()
            },
            personality: character.personality.clone(),
            ..fidget_core::director::Context::quiet()
            happened: fidget_core::director::Happened::Proactive,
            standing: String::new(),
            front_title: None,
        };

        (0..64)
            .filter_map(|_| director.propose(&moment).map(|played| played.behavior))
            .collect()
    }

    /// The files of a minimal valid package: a manifest naming one frame per
    /// required Animation, the frames themselves under `frames/`, and a prompt.
    fn package_files() -> Vec<(String, Vec<u8>)> {
        let mut manifest = String::from("name = \"Blip\"\n");
        let mut files = Vec::new();

        for animation in character::REQUIRED_ANIMATIONS {
            manifest.push_str(&format!(
                "[animations.{animation}]\nframes = [\"frames/{animation}-0.png\"]\n"
            ));
            files.push((format!("frames/{animation}-0.png"), FRAME.to_vec()));
        }

        files.push((CHARACTER_MANIFEST_FILE.to_string(), manifest.into_bytes()));
        files.push((
            character::PERSONALITY_FILE.to_string(),
            b"Blip is cheerful.".to_vec(),
        ));
        files
    }

    fn write_package(root: &Path) {
        for (name, bytes) in package_files() {
            let path = root.join(&name);
            fs::create_dir_all(path.parent().expect("every file has a parent"))
                .expect("package directories are creatable");
            fs::write(&path, bytes).expect("package files are writable");
        }
    }

    /// Write the same package as an archive, wrapped in one directory the way
    /// `zip -r` and Finder's Compress both produce. `macos_litter` adds `__MACOSX/`
    /// and `.DS_Store`.
    fn write_archive(path: &Path, root_name: &str, macos_litter: bool) {
        let file = fs::File::create(path).expect("archive is creatable");
        let mut zip = zip::ZipWriter::new(file);
        let stored: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);

        let mut write = |name: String, bytes: &[u8]| {
            zip.start_file(name, stored)
                .expect("archive entry is startable");
            zip.write_all(bytes).expect("archive entry is writable");
        };

        for (name, bytes) in package_files() {
            write(format!("{root_name}/{name}"), &bytes);
            if macos_litter {
                let (directory, file) = match name.rsplit_once('/') {
                    Some((directory, file)) => (format!("{directory}/"), file),
                    None => (String::new(), name.as_str()),
                };
                write(
                    format!("__MACOSX/{root_name}/{directory}._{file}"),
                    b"extended attributes, not art",
                );
            }
        }
        if macos_litter {
            write(format!("{root_name}/.DS_Store"), b"folder bookkeeping");
        }
        zip.finish().expect("archive is finishable");
    }

    /// The rejection, or a failure naming the path that read instead.
    /// Never `Debug`-printed: the bytes carry every frame of the art.
    fn refusal(result: Result<PackageBytes, ReadError>) -> ReadError {
        match result {
            Ok(_) => panic!("expected a refusal, read a package"),
            Err(why) => why,
        }
    }

    #[test]
    fn a_package_directory_loads_into_a_validated_character() {
        let dir = TempDir::new("package-dir");
        let root = dir.join("blip");
        write_package(&root);

        let files = read(&root).expect("the package reads");
        let character = character::load(&files).expect("the package is valid");
        assert_eq!(character.name, "Blip");
        assert_eq!(character.personality, "Blip is cheerful.");
        assert_eq!(
            character.animations.len(),
            character::REQUIRED_ANIMATIONS.len()
        );
        assert!(
            character.animations["walk"].frames == vec!["frames/walk-0.png".to_string()],
            "frames keep the names the manifest wrote"
        );
    }

    #[test]
    fn the_same_package_as_an_archive_loads_identically() {
        let dir = TempDir::new("package-archive");
        let root = dir.join("blip");
        write_package(&root);
        let archive = dir.join("blip.zip");
        write_archive(&archive, "blip", false);

        let from_directory = read(&root).expect("the directory reads");
        let from_archive = read(&archive).expect("the archive reads");
        assert_eq!(from_directory, from_archive);
        assert_eq!(
            character::load(&from_directory).expect("the package is valid"),
            character::load(&from_archive).expect("the package is valid"),
        );
    }

    /// Finder's `__MACOSX/` is a second top-level directory; leaving it in place
    /// hides the Character Manifest one level down and the package is disowned.
    #[test]
    fn an_archive_carrying_finders_litter_loads_identically() {
        let dir = TempDir::new("finder-archive");
        let root = dir.join("blip");
        write_package(&root);
        let archive = dir.join("blip.zip");
        write_archive(&archive, "blip", true);

        let from_directory = read(&root).expect("the directory reads");
        let from_archive = read(&archive).expect("the archive reads");
        assert_eq!(
            from_directory, from_archive,
            "the litter is dropped rather than carried as package content"
        );
    }

    /// The depth bound is the one the constant names: the package root and
    /// seven directories under it are walked, and an eighth is refused.
    #[test]
    fn a_package_nested_past_the_depth_bound_is_refused() {
        let dir = TempDir::new("deep");
        let root = dir.join("blip");
        let deepest = root.join("1/2/3/4/5/6/7");
        fs::create_dir_all(&deepest).expect("directories are creatable");
        read_directory(&root).expect("the bound itself is walked");

        fs::create_dir_all(deepest.join("8")).expect("directories are creatable");
        let why = read_directory(&root).expect_err("one directory deeper is refused");
        assert!(why.contains("nests deeper"), "{why}");
    }

    #[test]
    fn a_directory_that_is_not_a_package_is_reported_as_such() {
        let dir = TempDir::new("not-a-package");
        let downloads = dir.join("Downloads");
        fs::create_dir_all(&downloads).expect("directory is creatable");
        fs::write(downloads.join("invoice.pdf"), b"not art").expect("file is writable");

        match refusal(read(&downloads)) {
            ReadError::NotAPackage(path) => assert_eq!(path, downloads),
            other => panic!("expected NotAPackage, got {other:?}"),
        }
    }

    /// The distinction the author cares about: a package with a mistake in it
    /// is a different problem from a directory that was never a package. The
    /// first reads and is rejected by the loader; only the second is disowned.
    #[test]
    fn a_package_missing_a_required_animation_is_rejected_rather_than_disowned() {
        let dir = TempDir::new("broken-package");
        let root = dir.join("blip");
        write_package(&root);
        fs::write(
            root.join(CHARACTER_MANIFEST_FILE),
            "name = \"Blip\"\n[animations.idle]\nframes = [\"frames/idle-0.png\"]\n",
        )
        .expect("manifest is writable");

        let files = read(&root).expect("a broken package still reads");
        let errors = character::load(&files).expect_err("and the loader rejects it");
        assert!(
            errors.iter().any(|error| error.contains("\"walk\"")),
            "the rejection names the missing animation: {errors:#?}"
        );
    }

    #[test]
    fn a_truncated_archive_is_reported_with_its_path() {
        let dir = TempDir::new("truncated");
        let archive = dir.join("blip.zip");
        fs::write(&archive, b"PK\x03\x04 and then nothing").expect("file is writable");

        match refusal(read(&archive)) {
            ReadError::Unreadable { path, why } => {
                assert_eq!(path, archive);
                assert!(!why.is_empty(), "the reason is carried, not dropped");
            }
            other => panic!("expected Unreadable, got {other:?}"),
        }
    }

    #[test]
    fn a_path_that_is_not_there_is_reported_with_its_path() {
        let dir = TempDir::new("absent");
        let missing = dir.join("nowhere");

        match refusal(read(&missing)) {
            ReadError::Unreadable { path, .. } => assert_eq!(path, missing),
            other => panic!("expected Unreadable, got {other:?}"),
        }
    }

    /// A zip bomb: an entry whose header declares almost nothing and whose
    /// stream inflates to far more. The header is the archive's claim about
    /// itself, so the reader has to cap the read rather than believe it.
    #[test]
    fn an_archive_entry_that_inflates_past_what_it_declares_is_refused() {
        let dir = TempDir::new("zip-bomb");
        let archive = dir.join("bomb.zip");

        {
            let file = fs::File::create(&archive).expect("archive is creatable");
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file(
                CHARACTER_MANIFEST_FILE,
                zip::write::FileOptions::<'_, ()>::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .expect("archive entry is startable");
            // Highly compressible, so the entry on disk is a few hundred bytes.
            zip.write_all(&vec![b'a'; 8 * 1024 * 1024])
                .expect("archive entry is writable");
            zip.finish().expect("archive is finishable");
        }

        // Rewrite the declared uncompressed size to 1 everywhere it appears:
        // the local header, the central directory, and any data descriptor.
        let mut bytes = fs::read(&archive).expect("archive is readable");
        let declared = (8u32 * 1024 * 1024).to_le_bytes();
        for at in 0..bytes.len().saturating_sub(4) {
            if bytes[at..at + 4] == declared {
                bytes[at..at + 4].copy_from_slice(&1u32.to_le_bytes());
            }
        }
        fs::write(&archive, bytes).expect("archive is writable");

        match refusal(read(&archive)) {
            ReadError::Unreadable { path, why } => {
                assert_eq!(path, archive);
                assert!(
                    why.contains("declares"),
                    "the refusal says the entry outgrew its header: {why}"
                );
            }
            other => panic!("expected Unreadable, got {other:?}"),
        }
    }

    /// Unix only: Windows has no equivalent of a file its owner cannot open, and
    /// a directory does not stand in because the walker recurses into it.
    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_be_opened_is_reported_with_its_path() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new("permissions");
        let root = dir.join("blip");
        write_package(&root);

        let unreadable = root.join("frames/idle-0.png");
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000))
            .expect("permissions are settable");

        let refused = refusal(read(&root));
        // Put it back before asserting, so a failure still leaves a removable
        // directory behind.
        let _ = fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o644));

        match refused {
            ReadError::Unreadable { path, why } => {
                assert_eq!(path, root, "the package is named");
                assert!(
                    why.contains("idle-0.png"),
                    "and so is the file inside it that could not be opened: {why}"
                );
            }
            other => panic!("expected Unreadable, got {other:?}"),
        }
    }

    /// Name order is not a decision. Without a default the Character a new user
    /// meets is whichever package sorts first, so adding one could silently
    /// replace the fidget everybody sees.
    #[test]
    fn the_default_character_is_met_first_and_is_not_the_only_one() {
        let candidates = vec![
            PathBuf::from("/characters/nim"),
            PathBuf::from("/characters/blip"),
            PathBuf::from("/characters/buddy-bot"),
        ];

        assert_eq!(
            preferring(candidates.clone(), "buddy-bot").first(),
            Some(&PathBuf::from("/characters/buddy-bot")),
            "the default is met first wherever it sorts"
        );
        assert_eq!(
            preferring(candidates.clone(), "buddy-bot").len(),
            candidates.len(),
            "and the rest stay behind it, so a default that will not load is not the end"
        );
        assert_eq!(
            preferring(candidates.clone(), "nobody"),
            candidates,
            "a default that is not installed leaves the search order alone"
        );
    }

    /// The search takes the first package that loads, so without this name
    /// order would make every other Character unreachable.
    #[test]
    fn a_named_character_is_the_only_candidate_left() {
        let candidates = vec![
            PathBuf::from("/characters/nim"),
            PathBuf::from("/characters/blip"),
            PathBuf::from("/characters/bmo.zip"),
        ];

        assert_eq!(
            named(candidates.clone(), Some(OsStr::new("bmo"))),
            vec![PathBuf::from("/characters/bmo.zip")],
            "named by its file name, archive or directory alike"
        );
        assert_eq!(
            named(candidates.clone(), None),
            candidates,
            "and naming nothing leaves the search as it was"
        );
        assert!(
            named(candidates, Some(OsStr::new("nobody"))).is_empty(),
            "a Character that is not there is no Character, not the next one along"
        );
    }

    #[test]
    fn installed_packages_are_the_directories_and_archives_in_the_search_paths() {
        let dir = TempDir::new("installed");
        let characters = dir.join("characters");
        fs::create_dir_all(characters.join("blip")).expect("directory is creatable");
        fs::write(characters.join("mochi.zip"), b"").expect("file is writable");
        fs::write(characters.join("README.md"), b"").expect("file is writable");

        let found = installed(&[characters.clone(), dir.join("no-such-directory")]);
        assert_eq!(
            found,
            vec![characters.join("blip"), characters.join("mochi.zip")],
            "a directory and an archive count; a loose file does not, \
             and a search path that is not there is not an error"
        );
    }

    fn roots<'a>(
        desktop: Desktop,
        data_dir: &'a Path,
        extra: Option<&'a OsStr>,
        xdg_data_dirs: Option<&'a OsStr>,
    ) -> PackageRoots<'a> {
        PackageRoots {
            extra,
            data_dir,
            xdg_data_dirs,
            desktop,
        }
    }

    #[test]
    fn macos_search_paths_are_the_user_directory_then_the_bundle() {
        let data_dir = Path::new("/Users/buddy/Library/Application Support/fidget");
        let bundled = PathBuf::from("/Applications/fidget.app/Contents/Resources/characters");
        assert_eq!(
            search_paths_from(
                Some(bundled.clone()),
                roots(
                    Desktop::Macos,
                    data_dir,
                    None,
                    Some(OsStr::new("/usr/share"))
                ),
            ),
            vec![data_dir.join("characters"), bundled]
        );
    }

    #[test]
    fn linux_search_paths_use_the_xdg_defaults_when_the_variable_is_unset() {
        let data_dir = Path::new("/home/buddy/.local/share/fidget");
        let bundled = PathBuf::from("/usr/lib/fidget/characters");
        assert_eq!(
            search_paths_from(
                Some(bundled.clone()),
                roots(Desktop::Linux, data_dir, None, None)
            ),
            vec![
                data_dir.join("characters"),
                PathBuf::from("/usr/local/share/fidget/characters"),
                PathBuf::from("/usr/share/fidget/characters"),
                bundled,
            ]
        );
    }

    #[test]
    fn linux_search_paths_follow_xdg_data_dirs_when_it_is_set() {
        let data_dir = Path::new("/srv/xdg/fidget");
        let bundled = PathBuf::from("/opt/fidget/characters");
        let set = search_paths_from(
            Some(bundled.clone()),
            roots(
                Desktop::Linux,
                data_dir,
                None,
                Some(OsStr::new("/opt/share:/usr/local/share")),
            ),
        );
        assert_eq!(
            set,
            vec![
                data_dir.join("characters"),
                PathBuf::from("/opt/share/fidget/characters"),
                PathBuf::from("/usr/local/share/fidget/characters"),
                bundled.clone(),
            ]
        );

        let empty = search_paths_from(
            Some(bundled.clone()),
            roots(Desktop::Linux, data_dir, None, Some(OsStr::new(""))),
        );
        assert_eq!(
            empty,
            vec![
                data_dir.join("characters"),
                PathBuf::from("/usr/local/share/fidget/characters"),
                PathBuf::from("/usr/share/fidget/characters"),
                bundled,
            ]
        );
    }

    #[test]
    fn linux_search_paths_skip_a_relative_xdg_data_dir() {
        let data_dir = Path::new("/home/buddy/.local/share/fidget");
        let paths = search_paths_from(
            None,
            roots(
                Desktop::Linux,
                data_dir,
                None,
                Some(OsStr::new("relative:/usr/share")),
            ),
        );
        assert_eq!(
            paths,
            vec![
                data_dir.join("characters"),
                PathBuf::from("/usr/share/fidget/characters"),
            ]
        );
    }

    #[test]
    fn fidget_characters_is_added_in_front_on_each_desktop() {
        let data_dir = Path::new("/Users/buddy/Library/Application Support/fidget");
        let bundled = PathBuf::from("/bundled/characters");
        let macos = search_paths_from(
            Some(bundled.clone()),
            roots(
                Desktop::Macos,
                data_dir,
                Some(OsStr::new("/tmp/one:/tmp/two")),
                None,
            ),
        );
        assert_eq!(
            macos,
            vec![
                PathBuf::from("/tmp/one"),
                PathBuf::from("/tmp/two"),
                data_dir.join("characters"),
                bundled.clone(),
            ]
        );

        let windows_data = Path::new(r"C:\Users\buddy\AppData\Roaming\fidget");
        let windows = search_paths_from(
            Some(bundled.clone()),
            roots(
                Desktop::Windows,
                windows_data,
                Some(OsStr::new(r"D:\one;E:\two")),
                Some(OsStr::new("/usr/share")),
            ),
        );
        assert_eq!(
            windows,
            vec![
                PathBuf::from(r"D:\one"),
                PathBuf::from(r"E:\two"),
                windows_data.join("characters"),
                bundled,
            ]
        );
    }

    #[test]
    fn windows_fidget_characters_does_not_split_on_colons() {
        let data_dir = Path::new(r"C:\Users\buddy\AppData\Roaming\fidget");
        let paths = search_paths_from(
            None,
            roots(
                Desktop::Windows,
                data_dir,
                Some(OsStr::new(r"D:\chars:still")),
                None,
            ),
        );
        assert_eq!(
            paths,
            vec![
                PathBuf::from(r"D:\chars:still"),
                data_dir.join("characters")
            ]
        );
    }

    #[test]
    fn windows_search_paths_keep_the_user_directory_without_home() {
        let data_dir = Path::new(r"C:\Users\buddy\AppData\Roaming\fidget");
        let bundled = PathBuf::from(r"C:\Program Files\fidget\characters");
        let paths = search_paths_from(
            Some(bundled.clone()),
            roots(Desktop::Windows, data_dir, None, None),
        );
        assert_eq!(paths, vec![data_dir.join("characters"), bundled]);
        assert!(
            paths.iter().all(|path| !path
                .components()
                .any(|component| component.as_os_str() == "Library")),
            "the macOS Library path is not a Windows search path: {paths:?}"
        );
    }

    #[test]
    fn an_empty_fidget_characters_does_not_hide_the_user_directory() {
        let data_dir = Path::new("/home/buddy/.local/share/fidget");
        let bundled = PathBuf::from("/bundled/characters");
        let unset = search_paths_from(
            Some(bundled.clone()),
            roots(
                Desktop::Linux,
                data_dir,
                None,
                Some(OsStr::new("/usr/share")),
            ),
        );
        let empty = search_paths_from(
            Some(bundled),
            roots(
                Desktop::Linux,
                data_dir,
                Some(OsStr::new("")),
                Some(OsStr::new("/usr/share")),
            ),
        );
        assert_eq!(empty, unset);
        assert_eq!(
            empty,
            vec![
                data_dir.join("characters"),
                PathBuf::from("/usr/share/fidget/characters"),
                PathBuf::from("/bundled/characters"),
            ]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn search_paths_on_linux_read_the_data_directory() {
        let user = fidget_core::memory::data_dir().join("characters");
        let bundled = PathBuf::from("/bundled/characters");
        let paths = search_paths(Some(bundled.clone()));
        assert!(
            paths.contains(&user),
            "the user directory is missing from {paths:?}"
        );
        assert_eq!(
            paths.last(),
            Some(&bundled),
            "the bundled directory stays last: {paths:?}"
        );
        assert!(
            paths.iter().all(|path| !path
                .components()
                .any(|component| component.as_os_str() == "Library")),
            "linux does not search the macOS Library directory: {paths:?}"
        );
    }

    /// A Character Package this repository ships, by its directory name.
    fn shipped(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../characters")
            .join(name)
    }

    fn shipped_character(name: &str) -> Character {
        let files = read(&shipped(name)).unwrap_or_else(|why| panic!("{why}"));
        character::load(&files).unwrap_or_else(|errors| panic!("{name}: {errors:#?}"))
    }

    /// Every Animation one of a Character's Behaviors puts on screen, played
    /// through the Engine that will play it for real rather than read off the
    /// manifest: what a Behavior does is what the Engine makes of it.
    fn played(character: &Character, behavior: &str) -> BTreeSet<&'static str> {
        let ground = WorldSnapshot {
            displays: vec![Rect {
                x: 0.0,
                y: 0.0,
                width: 1000.0,
                height: 800.0,
            }],
            elapsed_ms: 100,
            ..WorldSnapshot::default()
        };
        let mut engine =
            Engine::new(Point { x: 100.0, y: 0.0 }).with_behaviors(character.behaviors.clone());

        // On its feet first: every Primitive but expression is refused in
        // mid-air, so a sprite still falling would refuse the lot.
        for _ in 0..40 {
            engine.tick(&ground);
        }

        let proposed = WorldSnapshot {
            proposal: Some(BehaviorProposal {
                behavior: behavior.to_string(),
                dialogue: None,
            }),
            ..ground.clone()
        };
        let mut seen = BTreeSet::from([engine.tick(&proposed).animation]);
        // Six seconds: longer than the longest chain either Character declares,
        // and short of the walls a walk would otherwise reach.
        for _ in 0..60 {
            seen.insert(engine.tick(&ground).animation);
        }
        seen
    }

    /// Both packages must draw all nine required Animations. A Primitive plays
    /// one of those nine, so a missing frame or a named frame the package does
    /// not carry is what fails here.
    #[test]
    fn both_shipped_characters_load_through_the_same_loader() {
        for (directory, name) in [("bmo", "BMO"), ("nim", "Nim")] {
            let character = shipped_character(directory);
            assert_eq!(character.name, name);
            assert!(
                character.behaviors.contains_key("walk"),
                "{name} declares the one Behavior the Engine acts on itself, \
                 or no Director could ever set it walking"
            );

            for animation in character::REQUIRED_ANIMATIONS {
                assert!(
                    character.draw(animation, 0, 0, 1.0).is_some(),
                    "{name} draws its {animation:?} animation"
                );
            }
        }
    }

    /// Nothing a Character declares decides when it sits. BMO never settles;
    /// Nim does except on bare `walk`/`jump`, which are lone motion and leave
    /// it on its feet.
    #[test]
    fn switching_between_the_two_changes_the_idle_life_and_not_only_the_art() {
        let bmo = shipped_character("bmo");
        for behavior in bmo.behaviors.keys() {
            let seen = played(&bmo, behavior);
            assert!(
                !seen.contains("sit") && !seen.contains("sleep"),
                "BMO settles down in {behavior:?}: {seen:?}"
            );
        }

        let nim = shipped_character("nim");
        let bare_motion = ["walk", "jump"];
        for behavior in nim
            .behaviors
            .keys()
            .filter(|name| !bare_motion.contains(&name.as_str()))
        {
            let seen = played(&nim, behavior);
            assert!(
                seen.contains("sit") || seen.contains("sleep"),
                "Nim never comes to rest in {behavior:?}: {seen:?}"
            );
        }
    }

    /// One Character shipped twice with the palette swapped. BMO and Nim must
    /// not share a frame's geometry, and Nim still carries more frames overall.
    #[test]
    fn the_two_shipped_characters_are_not_one_character_twice() {
        let bmo = shipped_character("bmo");
        let nim = shipped_character("nim");

        // Width and height straight from the PNG header: the IHDR chunk's
        // first eight data bytes, big-endian, at a fixed offset.
        let size = |art: &character::Art| {
            (
                u32::from_be_bytes(art.png[16..20].try_into().unwrap()),
                u32::from_be_bytes(art.png[20..24].try_into().unwrap()),
            )
        };
        let bmo_grids: BTreeSet<_> = bmo.art.values().map(size).collect();
        let nim_grids: BTreeSet<_> = nim.art.values().map(size).collect();
        assert!(
            bmo_grids.is_disjoint(&nim_grids),
            "the two packages draw on the same grid: {bmo_grids:?} and {nim_grids:?}"
        );

        let frames = |c: &character::Character| -> usize {
            c.animations.values().map(|a| a.frames.len()).sum()
        };
        assert!(
            frames(&nim) > frames(&bmo),
            "Nim is the one that eases, and it has no more frames than BMO"
        );

        let bmo_art: BTreeSet<&Vec<u8>> = bmo.art.values().map(|art| &art.png).collect();
        assert!(
            nim.art.values().all(|art| !bmo_art.contains(&art.png)),
            "no frame is shipped in both packages"
        );
    }

    /// Whether a frame draws anything the hit-test cannot feel. Nim's contact
    /// shadow is its one translucent colour, so a pixel that is drawn at all
    /// but not drawn at `ALPHA_THRESHOLD` is shadow and can be nothing else.
    fn casts_a_shadow(frame: &[u8]) -> bool {
        let drawn = AlphaMask::from_png(frame, 1).expect("a shipped frame decodes");
        let solid = AlphaMask::from_png(frame, character::ALPHA_THRESHOLD)
            .expect("a shipped frame decodes");
        let (width, height) = drawn.size();
        let origin = SpriteRect {
            x: 0,
            y: 0,
            scale: 1,
        };

        (0..height).any(|y| {
            (0..width).any(|x| drawn.hit(&origin, x, y, false) && !solid.hit(&origin, x, y, false))
        })
    }

    /// `fall` is the Animation the Engine plays off the ground (throw, fall, and
    /// drag when there is no `grab`), so it is the one of Nim's with nothing under its feet.
    #[test]
    fn nim_casts_a_shadow_only_when_it_has_something_to_cast_it_on() {
        let nim = shipped_character("nim");

        for animation in character::REQUIRED_ANIMATIONS {
            for frame in &nim.animations[animation].frames {
                assert_eq!(
                    casts_a_shadow(&nim.art[frame].png),
                    animation != "fall",
                    "{frame}, a frame of {animation:?}"
                );
            }
        }
    }
}
