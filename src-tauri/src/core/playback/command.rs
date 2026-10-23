use crate::core::domain::TrackReference;
use crate::core::errors::PlaybackErrorKind;

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackCommand {
    Load(TrackReference),
    Play,
    Pause,
    Resume,
    Stop,
    Seek { position_ms: u64 },
    Next,
    Previous,
    SetVolume(Volume),
    GetState,
}

// Private field: the only way to build a Volume is new(), so SetVolume cannot carry an out-of-range value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Volume(u8);

impl Volume {
    pub fn new(value: u8) -> Result<Volume, PlaybackErrorKind> {
        if value <= 100 {
            Ok(Volume(value))
        } else {
            Err(PlaybackErrorKind::Validation("volume out of range"))
        }
    }

    pub fn value(&self) -> u8 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_bounds_zero_and_100_are_ok() {
        assert_eq!(Volume::new(0).map(|v| v.value()), Ok(0));
        assert_eq!(Volume::new(100).map(|v| v.value()), Ok(100));
    }

    #[test]
    fn volume_above_100_is_validation() {
        let err = PlaybackErrorKind::Validation("volume out of range");
        assert_eq!(Volume::new(101), Err(err));
        assert_eq!(Volume::new(255), Err(err));
    }

    #[test]
    fn volume_value_round_trips() {
        assert_eq!(Volume::new(42).unwrap().value(), 42);
    }
}
