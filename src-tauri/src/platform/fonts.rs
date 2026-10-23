//! Font families installed on this computer, so the app can use any of them by name.
//! macOS asks the system font manager; Linux asks fontconfig; Windows reads the font registry (names only, so
//! style variants such as "Arial Bold" are folded into their family). Nothing here reads a font file.
use std::collections::BTreeSet;

const MAX_FAMILIES: usize = 3000;
const MAX_NAME: usize = 80;

/// A name that is safe to put inside a quoted CSS font-family: short, printable, no quotes, slashes or braces.
fn clean(name: &str) -> Option<String> {
    let name = name.trim();
    let ok = !name.is_empty()
        && name.chars().count() <= MAX_NAME
        && !name.starts_with('.')
        && name.chars().all(|c| {
            !c.is_control() && !matches!(c, '"' | '\'' | '\\' | ';' | '{' | '}' | '<' | '>' | ',')
        });
    ok.then(|| name.to_owned())
}

fn tidy(names: impl IntoIterator<Item = String>) -> Vec<String> {
    let set: BTreeSet<String> = names.into_iter().filter_map(|n| clean(&n)).collect();
    let mut out: Vec<String> = set.into_iter().collect();
    out.sort_by_key(|n| n.to_lowercase());
    out.truncate(MAX_FAMILIES);
    out
}

/// Installed families, sorted, without duplicates. Empty when the system cannot say.
/// On macOS this must run on the main thread (a synchronous Tauri command does).
pub fn installed_families() -> Vec<String> {
    tidy(platform_families())
}

#[cfg(target_os = "macos")]
fn platform_families() -> Vec<String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSFontManager;
    let Some(mtm) = MainThreadMarker::new() else {
        return Vec::new();
    };
    NSFontManager::sharedFontManager(mtm)
        .availableFontFamilies()
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[cfg(target_os = "linux")]
fn platform_families() -> Vec<String> {
    let Ok(out) = std::process::Command::new("fc-list")
        .args([":", "family"])
        .output()
    else {
        return Vec::new();
    };
    // One line per face; a line can list aliases separated by commas, the first is the family.
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split(',').next().map(str::to_owned))
        .collect()
}

#[cfg(target_os = "windows")]
fn platform_families() -> Vec<String> {
    let Ok(out) = std::process::Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts",
        ])
        .output()
    else {
        return Vec::new();
    };
    const STYLES: [&str; 14] = [
        "Bold",
        "Italic",
        "Oblique",
        "Regular",
        "Light",
        "Semilight",
        "Semibold",
        "Black",
        "Medium",
        "Thin",
        "Heavy",
        "Demi",
        "Narrow",
        "Condensed",
    ];
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            // "    Arial Bold Italic (TrueType)    REG_SZ    arialbi.ttf"
            let name = l.trim().split("    ").next()?;
            let name = name.split(" (").next()?;
            let mut words: Vec<&str> = name.split_whitespace().collect();
            while words.len() > 1 && STYLES.contains(words.last()?) {
                words.pop();
            }
            Some(words.join(" "))
        })
        .collect()
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn platform_families() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_are_cleaned_sorted_and_deduplicated() {
        let got = tidy(
            [
                "Zapfino",
                "arial",
                "Arial",
                " Menlo ",
                ".SF Hidden",
                "Bad\"Name",
                "Semi;colon",
                "",
                "Arial",
            ]
            .map(String::from),
        );
        assert_eq!(got, ["Arial", "arial", "Menlo", "Zapfino"]);
        assert!(tidy(["x".repeat(81)]).is_empty());
    }
}
