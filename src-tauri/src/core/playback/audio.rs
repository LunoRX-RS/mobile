//! Requested PCM format and negotiated audio API format are separate facts.
#[cfg(desktop)]
use libmpv2::Mpv;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AudioQuality {
    Low,
    Normal,
    #[default]
    High,
}
impl AudioQuality {
    pub fn bitrate_limit(self) -> u32 {
        match self {
            Self::Low => 64_000,
            Self::Normal => 128_000,
            Self::High => u32::MAX,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PcmFormat {
    Int16,
    Int32,
    #[default]
    Float32,
}
impl PcmFormat {
    #[cfg(desktop)]
    fn mpv(self) -> &'static str {
        match self {
            Self::Int16 => "s16",
            Self::Int32 => "s32",
            Self::Float32 => "float",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SampleRate {
    #[default]
    Auto,
    Hz44100,
    Hz48000,
    Hz96000,
    Hz192000,
}
impl SampleRate {
    #[cfg(desktop)]
    fn hz(self) -> i64 {
        match self {
            Self::Auto => 0,
            Self::Hz44100 => 44100,
            Self::Hz48000 => 48000,
            Self::Hz96000 => 96000,
            Self::Hz192000 => 192000,
        }
    }
}
/// Loudness is applied as an mpv `af` chain. mpv applies `volume-gain` after `af`, so a gain
/// placed there would land behind the limiter. The gain therefore lives inside the chain.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Loudness {
    Off,
    #[default]
    Balanced,
    Loud,
}
impl Loudness {
    /// dynaudnorm levels quiet and loud parts toward a common peak (0.9, max 18 dB gain).
    /// Loud adds +4 dB, then alimiter caps peaks at 0.89 (about -1 dBFS) with auto-level off.
    pub fn filter(self) -> &'static str {
        match self {
            Self::Off => "",
            Self::Balanced => "lavfi=[dynaudnorm=f=500:g=31:p=0.9:m=8:s=0]",
            Self::Loud => {
                "lavfi=[dynaudnorm=f=500:g=31:p=0.9:m=8:s=0,volume=4dB,alimiter=limit=0.89:level=0]"
            }
        }
    }
    #[cfg(desktop)]
    pub fn apply(self, mpv: &Mpv) -> libmpv2::Result<()> {
        mpv.set_property("af", self.filter())
    }
    /// The same filters as `filter`, one per entry, so the effects chain can put its own in front.
    fn parts(self) -> Vec<&'static str> {
        let f = self.filter();
        let inner = f.strip_prefix("lavfi=[").and_then(|f| f.strip_suffix(']'));
        match self {
            Self::Off => Vec::new(),
            Self::Balanced => vec![inner.unwrap_or_default()],
            Self::Loud => inner.unwrap_or_default().splitn(3, ',').collect(),
        }
    }
}

/// Centre frequencies of the ten equaliser bands, in hertz.
pub const BAND_HZ: [u32; 10] = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
const GAIN_DB: std::ops::RangeInclusive<i8> = -12..=12;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Reverb {
    #[default]
    Off,
    Room,
    Hall,
}
impl Reverb {
    fn filter(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::Room => Some("aecho=0.8:0.85:40|70:0.25|0.15"),
            Self::Hall => Some("aecho=0.8:0.88:60|120|180:0.3|0.2|0.12"),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Compressor {
    #[default]
    Off,
    Light,
    Medium,
    Heavy,
}
impl Compressor {
    fn filter(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::Light => {
                Some("acompressor=threshold=0.25:ratio=2:attack=20:release=250:makeup=1.5")
            }
            Self::Medium => {
                Some("acompressor=threshold=0.15:ratio=4:attack=10:release=200:makeup=2.5")
            }
            Self::Heavy => Some("acompressor=threshold=0.08:ratio=8:attack=5:release=150:makeup=4"),
        }
    }
}

/// Moving-sound effects. The sound travels around the head by swinging between the left and right ears, with a little
/// room echo so it feels like a space. They need headphones. `Eight` is a slow circle; `Four` is faster, wider and deeper.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum Spatial {
    #[default]
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "8d")]
    Eight,
    #[serde(rename = "4d")]
    Four,
}
impl Spatial {
    fn filter(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::Eight => Some(
                "apulsator=mode=sine:hz=0.125:amount=1:offset_l=0:offset_r=0.5,aecho=0.8:0.7:60:0.2",
            ),
            Self::Four => Some(
                "extrastereo=m=1.6,apulsator=mode=sine:hz=0.3:amount=0.85:offset_l=0:offset_r=0.5,aecho=0.8:0.85:50|90:0.25|0.15",
            ),
        }
    }
}

/// The user's sound shaping, all done by mpv's own filters. Whole numbers only, so a setting can never put text into a
/// filter string. `valid` is the range check the command runs before anything is stored.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct Effects {
    /// Master switch: off plays the track untouched without losing the settings.
    pub enabled: bool,
    /// Gain before everything else, -12 to 12 dB.
    pub preamp: i8,
    /// Ten-band equaliser, -12 to 12 dB per band (see `BAND_HZ`).
    pub bands: [i8; 10],
    pub bass: i8,
    pub treble: i8,
    /// 0 is mono, 100 leaves the stereo image alone, 200 doubles it.
    pub stereo_width: u8,
    /// Headphone crossfeed, 0 (off) to 100.
    pub crossfeed: u8,
    pub reverb: Reverb,
    pub compressor: Compressor,
    /// Playback speed in percent, 50 to 200.
    pub speed: u8,
    /// True keeps the pitch when the speed changes. False lets pitch follow speed, as in nightcore (faster and
    /// higher) and slowed (slower and lower).
    pub keep_pitch: bool,
    pub spatial: Spatial,
}
impl Default for Effects {
    fn default() -> Self {
        Self {
            enabled: true,
            preamp: 0,
            bands: [0; 10],
            bass: 0,
            treble: 0,
            stereo_width: 100,
            crossfeed: 0,
            reverb: Reverb::Off,
            compressor: Compressor::Off,
            speed: 100,
            keep_pitch: true,
            spatial: Spatial::Off,
        }
    }
}
impl Effects {
    pub fn valid(&self) -> bool {
        self.preamp_ok()
            && self.bands.iter().all(|g| GAIN_DB.contains(g))
            && GAIN_DB.contains(&self.bass)
            && GAIN_DB.contains(&self.treble)
            && self.stereo_width <= 200
            && self.crossfeed <= 100
            && (50..=200).contains(&self.speed)
    }
    fn preamp_ok(&self) -> bool {
        GAIN_DB.contains(&self.preamp)
    }
    /// With the switch off, or when asked to keep the pitch, mpv corrects the pitch as the speed changes.
    pub fn pitch_correction(&self) -> bool {
        !self.enabled || self.keep_pitch
    }
    /// The `speed` mpv should play at.
    pub fn speed_factor(&self) -> f64 {
        if self.enabled {
            f64::from(self.speed) / 100.0
        } else {
            1.0
        }
    }
    /// Filters in signal order. Empty when nothing is shaped or the master switch is off.
    fn parts(&self) -> Vec<String> {
        if !self.enabled {
            return Vec::new();
        }
        let mut p = Vec::new();
        if self.preamp != 0 {
            p.push(format!("volume={}dB", self.preamp));
        }
        for (hz, g) in BAND_HZ.iter().zip(self.bands) {
            if g != 0 {
                p.push(format!("equalizer=f={hz}:t=o:w=1:g={g}"));
            }
        }
        if self.bass != 0 {
            p.push(format!("bass=g={}", self.bass));
        }
        if self.treble != 0 {
            p.push(format!("treble=g={}", self.treble));
        }
        if self.stereo_width != 100 {
            p.push(format!(
                "extrastereo=m={:.2}",
                f64::from(self.stereo_width) / 100.0
            ));
        }
        if self.crossfeed != 0 {
            p.push(format!(
                "crossfeed=strength={:.2}",
                f64::from(self.crossfeed) / 100.0
            ));
        }
        p.extend(self.spatial.filter().map(str::to_owned));
        p.extend(self.reverb.filter().map(str::to_owned));
        p.extend(self.compressor.filter().map(str::to_owned));
        p
    }
    /// True when the settings can raise the level, so a limiter should guard the end of the chain.
    fn can_boost(&self) -> bool {
        self.enabled
            && (self.preamp > 0
                || self.bass > 0
                || self.treble > 0
                || self.bands.iter().any(|g| *g > 0)
                || self.compressor != Compressor::Off
                || self.stereo_width > 100
                || self.spatial == Spatial::Four)
    }
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioSettings {
    pub quality: AudioQuality,
    pub pcm_format: PcmFormat,
    pub sample_rate: SampleRate,
    #[serde(default)]
    pub loudness: Loudness,
    #[serde(default)]
    pub effects: Effects,
}
impl AudioSettings {
    /// The whole `af` value: the user's effects first, then loudness levelling, then a limiter when anything can boost.
    pub fn filter_chain(&self) -> String {
        let mut parts = self.effects.parts();
        let loud = self.loudness.parts();
        let limited = loud.iter().any(|f| f.starts_with("alimiter"));
        parts.extend(loud.into_iter().map(str::to_owned));
        if self.effects.can_boost() && !limited {
            parts.push("alimiter=limit=0.95:level=0".into());
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("lavfi=[{}]", parts.join(","))
        }
    }
    /// Filters, speed and whether speed keeps the pitch, applied to the running player.
    #[cfg(desktop)]
    pub fn apply_effects(&self, mpv: &Mpv) -> libmpv2::Result<()> {
        mpv.set_property("af", self.filter_chain())?;
        mpv.set_property("audio-pitch-correction", self.effects.pitch_correction())?;
        mpv.set_property("speed", self.effects.speed_factor())
    }
    #[cfg(desktop)]
    pub fn apply(self, mpv: &Mpv) -> libmpv2::Result<()> {
        mpv.set_property("options/audio-format", self.pcm_format.mpv())?;
        mpv.set_property("options/audio-samplerate", self.sample_rate.hz())
    }
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioOutput {
    pub driver: Option<String>,
    pub format: Option<String>,
    pub sample_rate: Option<i64>,
    pub source_format: Option<String>,
    pub source_sample_rate: Option<i64>,
}
#[derive(Clone, Default)]
pub struct AudioControl {
    pub settings: Arc<Mutex<AudioSettings>>,
    pub output: Arc<Mutex<AudioOutput>>,
}
impl AudioControl {
    pub fn settings(&self) -> AudioSettings {
        *self.settings.lock().unwrap_or_else(|e| e.into_inner())
    }
    #[cfg(desktop)]
    pub fn observe(&self, mpv: &Mpv) {
        *self.output.lock().unwrap_or_else(|e| e.into_inner()) = AudioOutput {
            driver: mpv.get_property("current-ao").ok(),
            format: mpv.get_property("audio-out-params/format").ok(),
            sample_rate: mpv.get_property("audio-out-params/samplerate").ok(),
            source_format: mpv.get_property("audio-params/format").ok(),
            source_sample_rate: mpv.get_property("audio-params/samplerate").ok(),
        };
    }
}
#[cfg(all(test, desktop))]
mod tests {
    use super::*;
    #[test]
    fn mpv_accepts_all_requested_pcm_formats_and_rates() {
        let mpv = Mpv::with_initializer(|init| {
            init.set_option("config", "no")?;
            init.set_option("ao", "null")
        })
        .unwrap();
        for pcm_format in [PcmFormat::Int16, PcmFormat::Int32, PcmFormat::Float32] {
            for sample_rate in [
                SampleRate::Auto,
                SampleRate::Hz44100,
                SampleRate::Hz48000,
                SampleRate::Hz96000,
                SampleRate::Hz192000,
            ] {
                AudioSettings {
                    quality: AudioQuality::High,
                    pcm_format,
                    sample_rate,
                    loudness: Loudness::Off,
                    effects: Effects::default(),
                }
                .apply(&mpv)
                .unwrap();
                assert_eq!(
                    mpv.get_property::<i64>("options/audio-samplerate").unwrap(),
                    sample_rate.hz()
                );
            }
        }
    }
    #[test]
    fn settings_accept_only_supported_formats_and_rates() {
        let mut value = serde_json::to_value(AudioSettings::default()).unwrap();
        assert_eq!(
            serde_json::from_value::<AudioSettings>(value.clone()).unwrap(),
            AudioSettings::default()
        );
        for bad in ["s24", "--script=evil", "int64"] {
            value["pcmFormat"] = bad.into();
            assert!(serde_json::from_value::<AudioSettings>(value.clone()).is_err());
        }
        value = serde_json::to_value(AudioSettings::default()).unwrap();
        value["sampleRate"] = "hz999999".into();
        assert!(serde_json::from_value::<AudioSettings>(value).is_err());
        assert_eq!(PcmFormat::Float32.mpv(), "float");
        assert_eq!(SampleRate::Hz192000.hz(), 192000);
    }
    #[test]
    fn loudness_defaults_to_balanced_and_old_payloads_still_parse() {
        let old = serde_json::json!({"quality":"high","pcmFormat":"float32","sampleRate":"auto"});
        let parsed: AudioSettings = serde_json::from_value(old).unwrap();
        assert_eq!(parsed.loudness, Loudness::Balanced);
        let json = serde_json::to_value(parsed).unwrap();
        assert_eq!(json["loudness"], "balanced");
        for bad in ["quiet", "lavfi=[evil]", "LOUD"] {
            let mut v = json.clone();
            v["loudness"] = bad.into();
            assert!(serde_json::from_value::<AudioSettings>(v).is_err());
        }
    }
    #[test]
    fn loudness_maps_to_filters_and_mpv_reads_them_back() {
        assert_eq!(Loudness::Off.filter(), "");
        assert!(Loudness::Balanced.filter().contains("dynaudnorm"));
        assert!(!Loudness::Balanced.filter().contains("volume="));
        let loud = Loudness::Loud.filter();
        assert!(loud.contains("dynaudnorm") && loud.contains("volume=4dB"));
        assert!(
            loud.find("volume=4dB") < loud.find("alimiter"),
            "limiter must follow the gain"
        );
        let mpv = Mpv::with_initializer(|init| {
            init.set_option("config", "no")?;
            init.set_option("ao", "null")
        })
        .unwrap();
        for l in [Loudness::Loud, Loudness::Balanced, Loudness::Off] {
            l.apply(&mpv).unwrap();
            let af: String = mpv.get_property("af").unwrap();
            assert_eq!(af.contains("dynaudnorm"), l != Loudness::Off, "{af}");
            assert_eq!(af.contains("alimiter"), l == Loudness::Loud, "{af}");
        }
    }
    fn settings(f: impl FnOnce(&mut AudioSettings)) -> AudioSettings {
        let mut s = AudioSettings::default();
        f(&mut s);
        s
    }
    #[test]
    fn no_effects_keep_the_old_loudness_chain_exactly() {
        for l in [Loudness::Off, Loudness::Balanced, Loudness::Loud] {
            let s = settings(|s| s.loudness = l);
            assert_eq!(s.filter_chain(), l.filter());
        }
    }
    #[test]
    fn effects_build_an_ordered_chain_with_a_limiter_when_they_can_boost() {
        let s = settings(|s| {
            s.loudness = Loudness::Off;
            s.effects.preamp = 3;
            s.effects.bands[0] = 4;
            s.effects.bands[9] = -2;
            s.effects.bass = 5;
            s.effects.stereo_width = 150;
            s.effects.reverb = Reverb::Room;
            s.effects.compressor = Compressor::Light;
        });
        let chain = s.filter_chain();
        let at = |needle: &str| {
            chain
                .find(needle)
                .unwrap_or_else(|| panic!("{needle} in {chain}"))
        };
        assert!(at("volume=3dB") < at("equalizer=f=31:t=o:w=1:g=4"));
        assert!(at("equalizer=f=16000") < at("bass=g=5"));
        assert!(at("bass=") < at("extrastereo=m=1.50"));
        assert!(at("extrastereo") < at("aecho"));
        assert!(at("aecho") < at("acompressor"));
        assert!(chain.ends_with("alimiter=limit=0.95:level=0]"), "{chain}");
        let loud = settings(|s| {
            s.loudness = Loudness::Loud;
            s.effects.bass = 3;
        })
        .filter_chain();
        assert_eq!(loud.matches("alimiter").count(), 1, "{loud}");
    }
    #[test]
    fn spatial_pitch_and_old_settings_files_behave() {
        let eight = settings(|s| {
            s.loudness = Loudness::Off;
            s.effects.spatial = Spatial::Eight;
        });
        let chain = eight.filter_chain();
        assert!(
            chain.contains("apulsator=mode=sine:hz=0.125") && chain.contains("aecho"),
            "{chain}"
        );
        assert!(
            !chain.contains("alimiter"),
            "8D only moves the sound between ears: {chain}"
        );
        let four = settings(|s| {
            s.loudness = Loudness::Off;
            s.effects.spatial = Spatial::Four;
        })
        .filter_chain();
        assert!(
            four.contains("extrastereo=m=1.6") && four.ends_with("alimiter=limit=0.95:level=0]"),
            "{four}"
        );
        // Moving-sound effects come before the room echo and the compressor.
        let both = settings(|s| {
            s.loudness = Loudness::Off;
            s.effects.spatial = Spatial::Eight;
            s.effects.compressor = Compressor::Light;
        })
        .filter_chain();
        assert!(both.find("apulsator").unwrap() < both.find("acompressor").unwrap());
        // Nightcore: faster and higher. The master switch always restores normal pitch handling.
        let night = settings(|s| {
            s.effects.speed = 125;
            s.effects.keep_pitch = false;
        });
        assert!(!night.effects.pitch_correction());
        assert_eq!(night.effects.speed_factor(), 1.25);
        let off = settings(|s| {
            s.effects.enabled = false;
            s.effects.keep_pitch = false;
        });
        assert!(off.effects.pitch_correction());
        // A settings file saved before these fields existed still loads, with them off.
        let old: Effects = serde_json::from_str(r#"{"enabled":true,"speed":110}"#).unwrap();
        assert_eq!(
            (old.spatial, old.keep_pitch, old.speed),
            (Spatial::Off, true, 110)
        );
        assert_eq!(serde_json::to_string(&Spatial::Eight).unwrap(), "\"8d\"");
        assert!(serde_json::from_str::<Spatial>("\"9d\"").is_err());
    }
    #[test]
    fn the_master_switch_and_cuts_only_need_no_limiter() {
        let off = settings(|s| {
            s.loudness = Loudness::Off;
            s.effects.enabled = false;
            s.effects.bass = 9;
            s.effects.speed = 150;
        });
        assert_eq!(off.filter_chain(), "");
        assert_eq!(off.effects.speed_factor(), 1.0);
        let cut = settings(|s| {
            s.loudness = Loudness::Off;
            s.effects.bands[3] = -6;
        });
        assert!(!cut.filter_chain().contains("alimiter"));
    }
    #[test]
    fn effect_ranges_are_enforced_and_unknown_fields_refused() {
        assert!(Effects::default().valid());
        for bad in [
            Effects {
                preamp: 13,
                ..Effects::default()
            },
            Effects {
                bands: [0, 0, 0, 0, 0, 0, 0, 0, 0, -13],
                ..Effects::default()
            },
            Effects {
                stereo_width: 201,
                ..Effects::default()
            },
            Effects {
                crossfeed: 101,
                ..Effects::default()
            },
            Effects {
                speed: 49,
                ..Effects::default()
            },
            Effects {
                speed: 201,
                ..Effects::default()
            },
        ] {
            assert!(!bad.valid(), "{bad:?}");
        }
        let mut v = serde_json::to_value(AudioSettings::default()).unwrap();
        v["effects"]["evil"] = "lavfi=[x]".into();
        assert!(serde_json::from_value::<AudioSettings>(v).is_err());
        let old = serde_json::json!({"quality":"high","pcmFormat":"float32","sampleRate":"auto","loudness":"off"});
        assert_eq!(
            serde_json::from_value::<AudioSettings>(old)
                .unwrap()
                .effects,
            Effects::default()
        );
    }
    #[test]
    fn mpv_accepts_the_built_chain_and_every_effect_filter() {
        let mpv = Mpv::with_initializer(|init| {
            init.set_option("config", "no")?;
            init.set_option("ao", "null")
        })
        .unwrap();
        let s = settings(|s| {
            s.loudness = Loudness::Loud;
            s.effects = Effects {
                preamp: -2,
                bands: [3, 2, 1, 0, -1, -2, 0, 1, 2, 3],
                bass: 4,
                treble: -3,
                stereo_width: 130,
                crossfeed: 40,
                reverb: Reverb::Hall,
                compressor: Compressor::Heavy,
                speed: 125,
                keep_pitch: false,
                spatial: Spatial::Four,
                ..Effects::default()
            };
        });
        s.apply_effects(&mpv).unwrap();
        assert!(!mpv.get_property::<bool>("audio-pitch-correction").unwrap());
        let af: String = mpv.get_property("af").unwrap();
        for f in [
            "equalizer",
            "bass",
            "treble",
            "extrastereo",
            "crossfeed",
            "apulsator",
            "aecho",
            "acompressor",
            "alimiter",
        ] {
            assert!(af.contains(f), "{f} missing from {af}");
        }
        assert!((mpv.get_property::<f64>("speed").unwrap() - 1.25).abs() < 1e-9);
        let eight = settings(|s| s.effects.spatial = Spatial::Eight);
        eight.apply_effects(&mpv).unwrap();
        assert!(
            mpv.get_property::<String>("af")
                .unwrap()
                .contains("apulsator")
        );
        AudioSettings::default().apply_effects(&mpv).unwrap();
        assert_eq!(mpv.get_property::<f64>("speed").unwrap(), 1.0);
        assert!(mpv.get_property::<bool>("audio-pitch-correction").unwrap());
    }
}
