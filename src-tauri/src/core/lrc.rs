//! Lyric Maker storage: user-authored `.lrc` files, one per YouTube id. Pure std, no Tauri types.
//! The frontend never sends a path; every path is built here from a validated id.
use super::playback::{
    extract::valid_id,
    lyrics::{TimedLine, parse_lrc_lines},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::UNIX_EPOCH,
};

pub const MAX_READ_BYTES: u64 = 256 * 1024;
pub const MAX_LIST_ITEMS: usize = 500;
const MAX_LINES: usize = 1_000;
const MAX_TEXT: usize = 300;
const MAX_TITLE: usize = 300;
const MAX_ARTISTS: usize = 8;
const MAX_ARTIST: usize = 200;
const MAX_MS: i64 = 86_400_000;
const NAME_PART: usize = 80;
// Directory entries examined per scan, so a hostile folder cannot stall a command.
const MAX_SCAN: usize = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "category", rename_all = "camelCase")]
pub enum LrcError {
    Validation,
    TooLarge,
    StorageUnavailable,
    NotFound,
    IoFailed,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineIn {
    pub start_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub title: String,
    pub artists: Vec<String>,
    pub duration_ms: u32,
    pub lines: Vec<TimedLine>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Loaded {
    pub path: PathBuf,
    pub lines: Vec<TimedLine>,
    pub title: Option<String>,
    pub artists: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Saved {
    pub path: Option<PathBuf>,
    pub count: usize,
    pub bytes: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ListItem {
    pub provider_track_id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub lines: usize,
    pub modified_ms: u64,
}

pub fn validate_id(id: &str) -> Result<(), LrcError> {
    valid_id(id).then_some(()).ok_or(LrcError::Validation)
}

/// Trim and collapse whitespace; None when it holds a control character or the length is out of range.
fn clean(value: &str, max: usize) -> Option<String> {
    if value.chars().any(char::is_control) {
        return None;
    }
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (1..=max).contains(&value.chars().count()).then_some(value)
}

pub fn prepare(
    title: &str,
    artists: &[String],
    duration_ms: i64,
    lines: &[LineIn],
) -> Result<Prepared, LrcError> {
    let v = LrcError::Validation;
    let title = clean(title, MAX_TITLE).ok_or(v)?;
    if artists.len() > MAX_ARTISTS
        || !(0..=MAX_MS).contains(&duration_ms)
        || lines.len() > MAX_LINES
    {
        return Err(v);
    }
    let artists = artists
        .iter()
        .map(|a| clean(a, MAX_ARTIST).ok_or(v))
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        if !(0..=MAX_MS).contains(&line.start_ms) {
            return Err(v);
        }
        out.push(TimedLine {
            start_ms: line.start_ms as u32,
            text: clean(&line.text, MAX_TEXT).ok_or(v)?,
            words: None,
        });
    }
    out.sort_by_key(|l| l.start_ms);
    let mut seen = HashSet::new();
    out.retain(|l| seen.insert((l.start_ms, l.text.clone())));
    Ok(Prepared {
        title,
        artists,
        duration_ms: duration_ms as u32,
        lines: out,
    })
}

fn tag(value: &str) -> String {
    value
        .chars()
        .filter(|c| !matches!(c, '[' | ']') && !c.is_control())
        .collect()
}

pub fn render(p: &Prepared) -> String {
    let mut out = format!("[ti:{}]\n", tag(&p.title));
    if !p.artists.is_empty() {
        out += &format!("[ar:{}]\n", tag(&p.artists.join(", ")));
    }
    if p.duration_ms > 0 {
        let s = p.duration_ms / 1000;
        out += &format!("[length:{:02}:{:02}]\n", s / 60, s % 60);
    }
    out += "[by:LUNOR-X Lyric Maker]\n[re:LUNOR-X]\n";
    for l in &p.lines {
        let (s, cs) = (l.start_ms / 1000, l.start_ms % 1000 / 10);
        out += &format!("[{:02}:{:02}.{:02}]{}\n", s / 60, s % 60, cs, l.text);
    }
    out
}

/// Tag value of the first `[name:...]` line, trimmed and bounded.
fn tag_value(text: &str, name: &str, max: usize) -> Option<String> {
    let prefix = format!("[{name}:");
    text.lines()
        .find_map(|l| l.trim().strip_prefix(&prefix)?.strip_suffix(']'))
        .map(|v| v.trim().chars().take(max).collect::<String>())
        .filter(|v| !v.is_empty())
}

type Parsed = (Vec<TimedLine>, Option<String>, Vec<String>);

pub fn parse(text: &str) -> Result<Parsed, LrcError> {
    let lines = parse_lrc_lines(text).ok_or(LrcError::TooLarge)?;
    let artists = tag_value(text, "ar", MAX_ARTISTS * (MAX_ARTIST + 2))
        .map(|a| {
            a.split(", ")
                .map(|p| p.trim().chars().take(MAX_ARTIST).collect::<String>())
                .filter(|p| !p.is_empty())
                .take(MAX_ARTISTS)
                .collect()
        })
        .unwrap_or_default();
    Ok((lines, tag_value(text, "ti", MAX_TITLE), artists))
}

fn name_part(value: &str) -> String {
    let replaced: String = value
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let joined = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = joined.chars().take(NAME_PART).collect();
    let cut = cut.trim_matches(|c: char| c == '.' || c == ' ');
    if cut.is_empty() { "Unknown" } else { cut }.to_string()
}

pub fn file_name(artist: Option<&str>, title: &str, id: &str) -> String {
    format!(
        "{} - {} [{id}].lrc",
        name_part(artist.unwrap_or("")),
        name_part(title)
    )
}

fn suffix(id: &str) -> String {
    format!(" [{id}].lrc")
}

fn id_of(name: &str) -> Option<&str> {
    let stem = name.strip_suffix("].lrc")?;
    let (head, id) = stem.split_at_checked(stem.len().checked_sub(11)?)?;
    (head.ends_with(" [") && valid_id(id)).then_some(id)
}

fn regular(path: &Path) -> Option<fs::Metadata> {
    let meta = fs::symlink_metadata(path).ok()?;
    meta.file_type().is_file().then_some(meta)
}

/// Regular (non-symlink) `.lrc` files whose name ends with ` [<id>].lrc`.
fn matches(dir: &Path, id: &str) -> Vec<PathBuf> {
    let want = suffix(id);
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.take(MAX_SCAN)
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.ends_with(&want)))
        .map(|e| e.path())
        .filter(|p| regular(p).is_some())
        .collect()
}

pub fn find(dir: &Path, id: &str) -> Result<Option<PathBuf>, LrcError> {
    validate_id(id)?;
    Ok(matches(dir, id).into_iter().next())
}

fn read_capped(path: &Path) -> Result<String, LrcError> {
    let meta = regular(path).ok_or(LrcError::IoFailed)?;
    if meta.len() > MAX_READ_BYTES {
        return Err(LrcError::TooLarge);
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|f| f.take(MAX_READ_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| LrcError::IoFailed)?;
    if bytes.len() as u64 > MAX_READ_BYTES {
        return Err(LrcError::TooLarge);
    }
    String::from_utf8(bytes).map_err(|_| LrcError::IoFailed)
}

pub fn load(dir: &Path, id: &str) -> Result<Option<Loaded>, LrcError> {
    let Some(path) = find(dir, id)? else {
        return Ok(None);
    };
    let (lines, title, artists) = parse(&read_capped(&path)?)?;
    Ok(Some(Loaded {
        path,
        lines,
        title,
        artists,
    }))
}

// ponytail: one global lock for all lyric writes; per-id locks only if contention ever shows.
static WRITES: Mutex<()> = Mutex::new(());

fn lock() -> Result<std::sync::MutexGuard<'static, ()>, LrcError> {
    WRITES.lock().map_err(|_| LrcError::IoFailed)
}

fn ensure_dir(dir: &Path) -> Result<(), LrcError> {
    if dir.is_dir() {
        return Ok(());
    }
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o755);
    }
    b.create(dir).map_err(|_| LrcError::StorageUnavailable)
}

fn write_atomic(dir: &Path, target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let part = dir.join(format!(".{:032x}.part", rand::random::<u128>()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o644);
        }
        let mut file = options.open(&part)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&part, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

/// Writes the file for `id`, replacing any file under an older name. Empty lines delete it instead.
pub fn save(dir: &Path, id: &str, p: &Prepared) -> Result<Saved, LrcError> {
    validate_id(id)?;
    let _guard = lock()?;
    let old = matches(dir, id);
    if p.lines.is_empty() {
        let path = old.first().cloned();
        for f in &old {
            fs::remove_file(f).map_err(|_| LrcError::IoFailed)?;
        }
        return Ok(Saved {
            path,
            count: 0,
            bytes: 0,
        });
    }
    let text = render(p);
    if text.len() as u64 > MAX_READ_BYTES {
        return Err(LrcError::TooLarge);
    }
    ensure_dir(dir)?;
    let target = dir.join(file_name(
        p.artists.first().map(String::as_str),
        &p.title,
        id,
    ));
    write_atomic(dir, &target, text.as_bytes()).map_err(|_| LrcError::IoFailed)?;
    for f in old.iter().filter(|f| **f != target) {
        let _ = fs::remove_file(f);
    }
    Ok(Saved {
        path: Some(target),
        count: p.lines.len(),
        bytes: text.len(),
    })
}

pub fn delete(dir: &Path, id: &str) -> Result<bool, LrcError> {
    validate_id(id)?;
    let _guard = lock()?;
    let old = matches(dir, id);
    for f in &old {
        fs::remove_file(f).map_err(|_| LrcError::IoFailed)?;
    }
    Ok(!old.is_empty())
}

pub fn list(dir: &Path) -> Result<Vec<ListItem>, LrcError> {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(LrcError::StorageUnavailable),
    };
    let mut items = Vec::new();
    for entry in rd.take(MAX_SCAN).filter_map(Result::ok) {
        let name = entry.file_name();
        let Some(id) = name.to_str().and_then(id_of) else {
            continue;
        };
        let path = entry.path();
        let Some(meta) = regular(&path).filter(|m| m.len() <= MAX_READ_BYTES) else {
            continue;
        };
        let Ok((lines, title, artists)) = read_capped(&path).and_then(|t| parse(&t)) else {
            continue;
        };
        let stem = name.to_str().unwrap_or_default();
        let fallback = stem.strip_suffix(&suffix(id)).unwrap_or(stem);
        items.push(ListItem {
            provider_track_id: id.to_string(),
            title: title.unwrap_or_else(|| fallback.to_string()),
            artists,
            lines: lines.len(),
            modified_ms: meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64),
        });
    }
    items.sort_by_key(|i| std::cmp::Reverse(i.modified_ms));
    items.truncate(MAX_LIST_ITEMS);
    Ok(items)
}

/// True when `path` is a regular file whose real location is inside `dir`.
pub fn inside(dir: &Path, path: &Path) -> bool {
    match (fs::canonicalize(dir), fs::canonicalize(path)) {
        (Ok(d), Ok(p)) => p.starts_with(d) && regular(&p).is_some(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "dQw4w9WgXcQ";

    fn tmp() -> PathBuf {
        std::env::temp_dir().join(format!("lunor-lrc-{:032x}", rand::random::<u128>()))
    }
    fn li(start_ms: i64, text: &str) -> LineIn {
        LineIn {
            start_ms,
            text: text.into(),
        }
    }
    fn prep(title: &str, lines: &[LineIn]) -> Prepared {
        prepare(title, &["Artist".to_string()], 200_000, lines).unwrap()
    }

    #[test]
    fn validation_matrix() {
        let ok = [li(0, "a")];
        let a = vec!["x".to_string()];
        assert!(prepare("T", &a, 0, &ok).is_ok());
        assert!(prepare("", &a, 0, &ok).is_err());
        assert!(prepare("  \t ", &a, 0, &ok).is_err());
        assert!(prepare(&"t".repeat(301), &a, 0, &ok).is_err());
        assert!(prepare(&"t".repeat(300), &a, 0, &ok).is_ok());
        assert!(prepare("a\nb", &a, 0, &ok).is_err());
        assert!(prepare("T", &vec!["x".to_string(); 9], 0, &ok).is_err());
        assert!(prepare("T", &["".to_string()], 0, &ok).is_err());
        assert!(prepare("T", &["x".repeat(201)], 0, &ok).is_err());
        assert!(prepare("T", &a, -1, &ok).is_err());
        assert!(prepare("T", &a, MAX_MS + 1, &ok).is_err());
        assert!(prepare("T", &a, MAX_MS, &ok).is_ok());
        let many: Vec<_> = (0..1001).map(|i| li(i, "a")).collect();
        assert!(prepare("T", &a, 0, &many).is_err());
        assert!(prepare("T", &a, 0, &many[..1000]).is_ok());
        for bad in [
            "",
            "   ",
            "line\nbreak",
            "cr\rx",
            "nul\0x",
            &"x".repeat(301),
        ] {
            assert!(prepare("T", &a, 0, &[li(0, bad)]).is_err(), "{bad:?}");
        }
        assert!(prepare("T", &a, 0, &[li(-1, "a")]).is_err());
        assert!(prepare("T", &a, 0, &[li(MAX_MS + 1, "a")]).is_err());
        assert!(prepare("T", &a, 0, &[li(MAX_MS, "a")]).is_ok());
        for id in [
            "",
            "short",
            "dQw4w9WgXcQ1",
            "dQw4w9WgX/Q",
            "dQw4w9WgX Q",
            "../../etc/p",
        ] {
            assert_eq!(validate_id(id), Err(LrcError::Validation), "{id}");
        }
        assert!(validate_id("a_B-9zZ0-_x").is_ok());
    }

    #[test]
    fn sorts_stably_and_drops_exact_duplicates() {
        let p = prep(
            "T",
            &[
                li(2000, "b"),
                li(1000, "x"),
                li(1000, "a"),
                li(1000, "x"),
                li(2000, "b"),
            ],
        );
        let got: Vec<_> = p
            .lines
            .iter()
            .map(|l| (l.start_ms, l.text.as_str()))
            .collect();
        assert_eq!(got, [(1000, "x"), (1000, "a"), (2000, "b")]);
    }

    #[test]
    fn golden_output() {
        let p = prepare(
            "My [Song]",
            &["A".to_string(), "B".to_string()],
            200_999,
            &[li(3_725_999, "late"), li(0, "first"), li(61_234, "second")],
        )
        .unwrap();
        assert_eq!(
            render(&p),
            "[ti:My Song]\n[ar:A, B]\n[length:03:20]\n[by:LUNOR-X Lyric Maker]\n[re:LUNOR-X]\n\
             [00:00.00]first\n[01:01.23]second\n[62:05.99]late\n"
        );
        let no_len = prepare("T", &[], 0, &[li(5, "x")]).unwrap();
        assert!(!render(&no_len).contains("[length:") && !render(&no_len).contains("[ar:"));
    }

    #[test]
    fn round_trips_through_the_parser() {
        let p = prepare(
            "ชื่อเพลง",
            &["ศิลปิน".to_string()],
            90_000,
            &[
                li(1_250, "สวัสดีครับ, โลก!"),
                li(65_000, "Hello - (world); \"quoted\" 100%"),
            ],
        )
        .unwrap();
        let (lines, title, artists) = parse(&render(&p)).unwrap();
        assert_eq!(
            lines,
            p.lines
                .iter()
                .map(|l| TimedLine {
                    start_ms: l.start_ms / 10 * 10,
                    text: l.text.clone(),
                    words: None,
                })
                .collect::<Vec<_>>()
        );
        assert_eq!(title.as_deref(), Some("ชื่อเพลง"));
        assert_eq!(artists, ["ศิลปิน"]);
    }

    #[test]
    fn filename_sanitization() {
        assert_eq!(
            file_name(Some("AC/DC"), "a\\b:c*d?e\"f<g>h|i", ID),
            format!("AC_DC - a_b_c_d_e_f_g_h_i [{ID}].lrc")
        );
        assert_eq!(
            file_name(None, "  ..  ", ID),
            format!("Unknown - Unknown [{ID}].lrc")
        );
        assert_eq!(
            file_name(Some("a\tb"), " x   y ", ID),
            format!("a_b - x y [{ID}].lrc")
        );
        let long = file_name(Some(&"ก".repeat(200)), &"t".repeat(200), ID);
        assert_eq!(
            long,
            format!("{} - {} [{ID}].lrc", "ก".repeat(80), "t".repeat(80))
        );
        assert_eq!(id_of(&long), Some(ID));
        assert_eq!(id_of("nope.lrc"), None);
        assert_eq!(id_of("x [short].lrc"), None);
        assert_eq!(id_of(&format!("x[{ID}].lrc")), None);
    }

    #[test]
    fn one_file_per_id_across_title_change_and_no_temp_left() {
        let dir = tmp().join("LUNOR-X Lyrics");
        let one = [li(1000, "a")];
        let first = save(&dir, ID, &prep("Old", &one)).unwrap();
        let second = save(&dir, ID, &prep("New", &one)).unwrap();
        assert_ne!(first.path, second.path);
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
        assert_eq!(
            names[0].to_str().unwrap(),
            format!("Artist - New [{ID}].lrc")
        );
        assert_eq!(second.count, 1);
        assert!(second.bytes > 0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(second.path.as_ref().unwrap()), 0o644);
            assert_eq!(mode(&dir), 0o755);
        }
        let loaded = load(&dir, ID).unwrap().unwrap();
        assert_eq!(loaded.title.as_deref(), Some("New"));
        assert_eq!(loaded.lines.len(), 1);
        assert!(load(&dir, "aaaaaaaaaaa").unwrap().is_none());
        assert_eq!(load(&dir, "bad").unwrap_err(), LrcError::Validation);
        fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn empty_lines_delete_and_delete_reports() {
        let dir = tmp();
        let saved = save(&dir, ID, &prep("T", &[li(0, "a")])).unwrap();
        let gone = save(&dir, ID, &prep("T", &[])).unwrap();
        assert_eq!((gone.count, gone.path.clone()), (0, saved.path.clone()));
        assert!(!saved.path.unwrap().exists());
        save(&dir, ID, &prep("T", &[li(0, "a")])).unwrap();
        assert!(delete(&dir, ID).unwrap());
        assert!(!delete(&dir, ID).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn oversize_save_is_rejected_and_leaves_nothing() {
        let dir = tmp();
        let big = "ก".repeat(300);
        let lines: Vec<_> = (0..1000).map(|i| li(i, &big)).collect();
        assert_eq!(save(&dir, ID, &prep("T", &lines)), Err(LrcError::TooLarge));
        assert!(!dir.exists());
    }

    #[test]
    fn oversize_and_symlink_on_load_and_list() {
        let dir = tmp();
        fs::create_dir_all(&dir).unwrap();
        let big = dir.join(format!("A - Big [{ID}].lrc"));
        fs::write(&big, vec![b'x'; MAX_READ_BYTES as usize + 1]).unwrap();
        assert_eq!(load(&dir, ID).unwrap_err(), LrcError::TooLarge);
        assert!(list(&dir).unwrap().is_empty());
        fs::remove_file(&big).unwrap();
        #[cfg(unix)]
        {
            let target = dir.join("real.txt");
            fs::write(&target, "[00:01.00]x\n").unwrap();
            let id2 = "bbbbbbbbbbb";
            std::os::unix::fs::symlink(&target, dir.join(format!("A - L [{id2}].lrc"))).unwrap();
            assert!(load(&dir, id2).unwrap().is_none());
            assert!(list(&dir).unwrap().is_empty());
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn list_orders_newest_first_ignores_strays_and_caps() {
        let dir = tmp();
        assert!(list(&dir).unwrap().is_empty());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("notes.txt"), "x").unwrap();
        fs::write(dir.join("A - B [short].lrc"), "[00:01.00]x").unwrap();
        for (i, id) in ["aaaaaaaaaaa", "bbbbbbbbbbb"].into_iter().enumerate() {
            save(
                &dir,
                id,
                &prep(&format!("T{i}"), &[li(0, "a"), li(1000, "b")]),
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let items = list(&dir).unwrap();
        let ids: Vec<_> = items.iter().map(|i| i.provider_track_id.as_str()).collect();
        assert_eq!(ids, ["bbbbbbbbbbb", "aaaaaaaaaaa"]);
        assert_eq!(
            (
                items[0].title.as_str(),
                items[0].lines,
                items[0].artists.clone()
            ),
            ("T1", 2, vec!["Artist".to_string()])
        );
        assert!(items[0].modified_ms >= items[1].modified_ms && items[0].modified_ms > 0);
        for n in 0..MAX_LIST_ITEMS + 5 {
            fs::write(
                dir.join(format!("A - T [c{n:010}].lrc")),
                "[ti:x]\n[00:01.00]x\n",
            )
            .unwrap();
        }
        assert_eq!(list(&dir).unwrap().len(), MAX_LIST_ITEMS);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn inside_rejects_outside_paths() {
        let dir = tmp();
        let other = tmp();
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(&other).unwrap();
        fs::write(dir.join("a.lrc"), "x").unwrap();
        fs::write(other.join("b.lrc"), "x").unwrap();
        assert!(inside(&dir, &dir.join("a.lrc")));
        assert!(!inside(&dir, &other.join("b.lrc")));
        assert!(!inside(&dir, &dir.join("missing.lrc")));
        assert!(!inside(&dir, &dir));
        fs::remove_dir_all(dir).unwrap();
        fs::remove_dir_all(other).unwrap();
    }

    #[test]
    fn error_serializes_as_category_only() {
        assert_eq!(
            serde_json::to_string(&LrcError::TooLarge).unwrap(),
            r#"{"category":"tooLarge"}"#
        );
        assert_eq!(
            serde_json::to_string(&LrcError::StorageUnavailable).unwrap(),
            r#"{"category":"storageUnavailable"}"#
        );
    }
}
