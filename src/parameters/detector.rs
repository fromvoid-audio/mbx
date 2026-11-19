//! Detector routing shared by audio processing and the editor.

use super::{spec, BandParam as P, Values};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Band,
    Custom,
    External,
}

impl Source {
    pub fn read(values: &Values, band: usize) -> Self {
        let id = P::Key.id(band);
        match spec(id).plain(values[id]) as u8 {
            1 => Self::Custom,
            2 => Self::External,
            _ => Self::Band,
        }
    }

    pub fn follows_band(self, values: &Values, band: usize) -> bool {
        let filter = P::KeyFilter.id(band);
        self == Self::Band || (self == Self::External && spec(filter).plain(values[filter]) > 0.0)
    }

    #[cfg(feature = "plugin")]
    pub fn title(self) -> &'static str {
        match self {
            Self::Band => "Internal band",
            Self::Custom => "Internal custom",
            Self::External => "External",
        }
    }
}

pub fn custom_range(values: &Values, band: usize) -> [f64; 2] {
    let [low, high] = [P::KeyLow, P::KeyHigh].map(|field| values[field.id(band)]);
    [low.min(high), low.max(high)]
}
