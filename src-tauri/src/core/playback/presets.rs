//! Sound presets: one list for the settings panel and the macOS Sound menu. A preset is a full set of effects, so
//! choosing one replaces whatever was set before (the master switch is turned on).
use super::audio::{Compressor, Effects, Reverb, Spatial};
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub effects: Effects,
}

/// Groups in the order they are shown.
pub const GROUPS: [&str; 5] = [
    "Everyday",
    "Genres",
    "Fun",
    "Moving sound",
    "Where you listen",
];

fn p(
    id: &'static str,
    label: &'static str,
    group: &'static str,
    set: impl FnOnce(&mut Effects),
) -> Preset {
    let mut effects = Effects::default();
    set(&mut effects);
    Preset {
        id,
        label,
        group,
        effects,
    }
}

/// Speed with pitch following, as in nightcore and slowed.
fn pitched(e: &mut Effects, speed: u8) {
    e.speed = speed;
    e.keep_pitch = false;
}

pub fn all() -> Vec<Preset> {
    const E: &str = "Everyday";
    const G: &str = "Genres";
    const F: &str = "Fun";
    const M: &str = "Moving sound";
    const W: &str = "Where you listen";
    vec![
        // Everyday
        p("flat", "Flat", E, |_| {}),
        p("bass", "Bass boost", E, |e| {
            e.bands = [5, 4, 3, 1, 0, 0, 0, 0, 0, 0];
            e.preamp = -3;
        }),
        p("bass-max", "Bass boost (extra)", E, |e| {
            e.bands = [8, 7, 5, 2, 0, 0, 0, 0, 0, 0];
            e.preamp = -6;
        }),
        p("treble", "Treble boost", E, |e| {
            e.bands = [0, 0, 0, 0, 0, 1, 2, 3, 4, 5];
            e.preamp = -3;
        }),
        p("vocal", "Vocal", E, |e| {
            e.bands = [-2, -2, -1, 1, 3, 3, 2, 1, 0, -1]
        }),
        p("warm", "Warm", E, |e| {
            e.bands = [2, 2, 1, 1, 0, 0, -1, -2, -3, -3];
        }),
        p("clear", "Clear and open", E, |e| {
            e.bands = [-1, -1, 0, 0, 0, 1, 2, 3, 3, 2];
            e.preamp = -2;
        }),
        p("loudness", "Loudness (smile)", E, |e| {
            e.bands = [5, 4, 2, 0, -1, -1, 0, 2, 4, 5];
            e.preamp = -4;
        }),
        p("soft", "Soft and gentle", E, |e| {
            e.bands = [1, 1, 0, 0, 0, -1, -2, -3, -3, -4];
            e.compressor = Compressor::Light;
        }),
        p("late", "Late night", E, |e| {
            e.bands = [2, 1, 0, 0, 0, 0, 0, 0, 0, 0];
            e.compressor = Compressor::Heavy;
        }),
        p("focus", "Focus and study", E, |e| {
            e.bands = [-3, -2, -1, 0, 1, 1, 0, -1, -2, -3];
            e.compressor = Compressor::Medium;
        }),
        // Genres
        p("pop", "Pop", G, |e| {
            e.bands = [-1, 1, 3, 4, 2, 0, -1, -1, -1, -1]
        }),
        p("rock", "Rock", G, |e| {
            e.bands = [4, 3, 1, -1, -2, -1, 1, 3, 4, 4];
            e.preamp = -3;
        }),
        p("hiphop", "Hip-hop", G, |e| {
            e.bands = [6, 5, 3, 1, -1, -1, 1, 0, 1, 2];
            e.preamp = -4;
        }),
        p("rnb", "R&B and soul", G, |e| {
            e.bands = [4, 3, 2, 1, -1, -1, 1, 1, 2, 2];
            e.preamp = -3;
        }),
        p("jazz", "Jazz", G, |e| {
            e.bands = [3, 2, 1, 2, -1, -1, 0, 1, 2, 3]
        }),
        p("classical", "Classical", G, |e| {
            e.bands = [3, 2, 1, 1, -1, -1, 0, 2, 2, 3];
        }),
        p("piano", "Piano and acoustic", G, |e| {
            e.bands = [0, 1, 2, 2, 1, 1, 2, 2, 3, 2];
        }),
        p("electronic", "Electronic", G, |e| {
            e.bands = [5, 4, 1, 0, -2, 1, 1, 2, 4, 5];
            e.preamp = -3;
        }),
        p("edm", "Dance and EDM", G, |e| {
            e.bands = [6, 5, 2, 0, 0, 1, 2, 3, 4, 3];
            e.preamp = -4;
            e.stereo_width = 120;
        }),
        p("metal", "Metal", G, |e| {
            e.bands = [4, 3, 0, -2, -1, 2, 4, 4, 3, 2];
            e.preamp = -3;
        }),
        p("country", "Country and folk", G, |e| {
            e.bands = [2, 1, 1, 2, 2, 1, 2, 3, 3, 2];
        }),
        p("reggae", "Reggae", G, |e| {
            e.bands = [4, 4, 1, -2, -2, 0, 1, 1, 2, 2]
        }),
        p("latin", "Latin", G, |e| {
            e.bands = [3, 2, 1, 0, 1, 2, 3, 3, 2, 2]
        }),
        p("lofi", "Lo-fi", G, |e| {
            e.bands = [3, 3, 2, 1, 0, -1, -3, -5, -8, -10];
            e.compressor = Compressor::Light;
            e.reverb = Reverb::Room;
        }),
        p("speech", "Podcast and speech", G, |e| {
            e.bands = [-6, -5, -3, 0, 3, 4, 3, 1, -1, -3];
            e.compressor = Compressor::Medium;
        }),
        // Fun
        p("night", "Nightcore", F, |e| {
            pitched(e, 125);
            e.treble = 2;
        }),
        p("sped", "Sped up", F, |e| pitched(e, 115)),
        p("slow", "Slowed + reverb", F, |e| {
            pitched(e, 85);
            e.reverb = Reverb::Hall;
            e.bass = 2;
        }),
        p("slowed", "Slowed", F, |e| pitched(e, 80)),
        p("vapor", "Vaporwave", F, |e| {
            pitched(e, 75);
            e.reverb = Reverb::Hall;
            e.bands = [3, 2, 1, 0, 0, 0, -1, -2, -3, -3];
        }),
        p("chipmunk", "Chipmunk", F, |e| pitched(e, 160)),
        p("deep", "Deep voice", F, |e| pitched(e, 70)),
        p("underwater", "Underwater", F, |e| {
            e.bands = [4, 3, 1, -2, -5, -8, -11, -12, -12, -12];
            e.reverb = Reverb::Hall;
        }),
        p("telephone", "Telephone", F, |e| {
            e.bands = [-12, -12, -12, -6, 3, 5, 3, -6, -12, -12];
            e.stereo_width = 0;
        }),
        p("megaphone", "Megaphone", F, |e| {
            e.bands = [-12, -12, -10, -4, 4, 8, 6, 0, -8, -12];
            e.stereo_width = 0;
            e.compressor = Compressor::Heavy;
        }),
        p("radio", "Old radio", F, |e| {
            e.bands = [-12, -10, -6, 0, 3, 3, 2, -2, -8, -12];
            e.stereo_width = 0;
            e.compressor = Compressor::Medium;
        }),
        // Moving sound (headphones)
        p("8d", "8D audio", M, |e| e.spatial = Spatial::Eight),
        p("4d", "4D audio", M, |e| {
            e.spatial = Spatial::Four;
            e.bass = 2;
        }),
        p("8d-hall", "8D concert hall", M, |e| {
            e.spatial = Spatial::Eight;
            e.reverb = Reverb::Hall;
        }),
        p("8d-bass", "8D bass", M, |e| {
            e.spatial = Spatial::Eight;
            e.bands = [5, 4, 2, 0, 0, 0, 0, 0, 0, 0];
            e.preamp = -3;
        }),
        p("4d-bass", "4D bass", M, |e| {
            e.spatial = Spatial::Four;
            e.bands = [6, 5, 3, 1, 0, 0, 0, 0, 0, 0];
            e.preamp = -4;
        }),
        p("8d-slow", "8D slowed", M, |e| {
            e.spatial = Spatial::Eight;
            pitched(e, 90);
            e.reverb = Reverb::Room;
        }),
        p("8d-night", "8D nightcore", M, |e| {
            e.spatial = Spatial::Eight;
            pitched(e, 120);
        }),
        // Where you listen
        p("headphones", "Headphones", W, |e| {
            e.crossfeed = 35;
            e.bands = [1, 1, 0, 0, 0, 0, 0, 1, 1, 0];
        }),
        p("earbuds", "Earbuds", W, |e| {
            e.bands = [4, 3, 2, 0, 0, 0, 1, 1, 0, 0];
            e.preamp = -3;
        }),
        p("laptop", "Laptop speakers", W, |e| {
            e.bands = [-6, -4, 0, 3, 4, 3, 2, 1, 0, 0];
            e.preamp = -2;
        }),
        p("car", "Car", W, |e| {
            e.bands = [5, 4, 2, 0, -1, 0, 1, 3, 4, 3];
            e.preamp = -4;
        }),
        p("tv", "TV and soundbar", W, |e| {
            e.bands = [2, 1, 0, 0, 2, 3, 3, 1, 0, 0];
            e.compressor = Compressor::Medium;
        }),
        p("small", "Small speaker", W, |e| {
            e.bands = [-8, -6, -2, 3, 4, 3, 2, 1, 0, 0];
            e.stereo_width = 130;
        }),
        p("room", "Live room", W, |e| {
            e.reverb = Reverb::Room;
            e.stereo_width = 130;
        }),
        p("hall", "Concert hall", W, |e| {
            e.reverb = Reverb::Hall;
            e.stereo_width = 120;
        }),
        p("cathedral", "Cathedral", W, |e| {
            e.reverb = Reverb::Hall;
            e.stereo_width = 150;
            e.bands = [2, 1, 0, 0, 0, 0, 0, -1, -2, -2];
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn presets_are_unique_valid_grouped_and_flat_means_untouched() {
        let list = all();
        assert!(list.len() >= 50, "{} presets", list.len());
        let mut ids = HashSet::new();
        for preset in &list {
            assert!(ids.insert(preset.id), "duplicate id {}", preset.id);
            assert!(!preset.label.is_empty());
            assert!(GROUPS.contains(&preset.group), "{} group", preset.id);
            assert!(preset.effects.valid(), "{} is out of range", preset.id);
            assert!(
                preset.effects.enabled,
                "{} must switch shaping on",
                preset.id
            );
        }
        for group in GROUPS {
            assert!(list.iter().any(|p| p.group == group), "{group} is empty");
        }
        assert_eq!(list[0].id, "flat");
        assert_eq!(list[0].effects, Effects::default());
        // Every preset except flat changes something.
        assert!(list[1..].iter().all(|p| p.effects != Effects::default()));
    }
}
