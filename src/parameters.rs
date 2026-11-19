//! Stable host parameter IDs and unit conversions.

pub mod detector;
pub mod layout;

pub const BANDS: usize = 6;
pub const MIX: usize = 0;
pub const OUTPUT: usize = 1;
pub const BYPASS: usize = 2;
const GLOBALS: usize = 3;
const ORIGINAL_BAND_FIELDS: usize = 17;
pub const LEGACY_COUNT: usize = GLOBALS + BANDS * ORIGINAL_BAND_FIELDS;
pub const LOOKAHEAD: usize = LEGACY_COUNT;
const NEW_BANDS: usize = LOOKAHEAD + 1;
const V2_COUNT: usize = NEW_BANDS + BANDS;
const SLOPES_START: usize = V2_COUNT;
const V3_COUNT: usize = SLOPES_START + BANDS * 2;
pub const PHASE: usize = V3_COUNT;
const V4_COUNT: usize = PHASE + 1;
const PRESENCE_START: usize = V4_COUNT;
const V6_COUNT: usize = PRESENCE_START + BANDS;
const AUDITION_START: usize = V6_COUNT;
pub const COUNT: usize = AUDITION_START + BANDS * 3;
pub const MAX_LOOKAHEAD_MS: f64 = 20.0;
pub type Values = [f64; COUNT];

#[repr(usize)]
#[derive(Clone, Copy)]
pub enum BandParam {
    Enabled,
    Solo,
    Low,
    High,
    Mode,
    Threshold,
    Ratio,
    Range,
    Knee,
    Attack,
    Release,
    Makeup,
    Route,
    Link,
    Key,
    KeyLow,
    KeyHigh,
    Lookahead,
    LowSlope,
    HighSlope,
    Present,
    Mute,
    Audition,
    KeyFilter,
}

impl BandParam {
    pub const ALL: [Self; 24] = [
        Self::Enabled,
        Self::Solo,
        Self::Low,
        Self::High,
        Self::Mode,
        Self::Threshold,
        Self::Ratio,
        Self::Range,
        Self::Knee,
        Self::Attack,
        Self::Release,
        Self::Makeup,
        Self::Route,
        Self::Link,
        Self::Key,
        Self::KeyLow,
        Self::KeyHigh,
        Self::Lookahead,
        Self::LowSlope,
        Self::HighSlope,
        Self::Present,
        Self::Mute,
        Self::Audition,
        Self::KeyFilter,
    ];

    pub const fn id(self, band: usize) -> usize {
        match self {
            Self::Lookahead => NEW_BANDS + band,
            Self::LowSlope => SLOPES_START + band * 2,
            Self::HighSlope => SLOPES_START + band * 2 + 1,
            Self::Present => PRESENCE_START + band,
            Self::Mute => AUDITION_START + band * 3,
            Self::Audition => AUDITION_START + band * 3 + 1,
            Self::KeyFilter => AUDITION_START + band * 3 + 2,
            _ => GLOBALS + band * ORIGINAL_BAND_FIELDS + self as usize,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Spec {
    pub name: &'static str,
    pub unit: &'static str,
    min: f64,
    max: f64,
    default: f64,
    log: bool,
    pub choices: &'static [&'static str],
}

const fn linear(name: &'static str, unit: &'static str, min: f64, max: f64, default: f64) -> Spec {
    Spec {
        name,
        unit,
        min,
        max,
        default,
        log: false,
        choices: &[],
    }
}

const fn log(name: &'static str, unit: &'static str, min: f64, max: f64, default: f64) -> Spec {
    Spec {
        log: true,
        ..linear(name, unit, min, max, default)
    }
}

const fn choice(name: &'static str, choices: &'static [&'static str], default: f64) -> Spec {
    Spec {
        choices,
        ..linear(name, "", 0.0, (choices.len() - 1) as f64, default)
    }
}

const GLOBAL_SPECS: [Spec; GLOBALS] = [
    linear("Mix", "%", 0.0, 200.0, 100.0),
    linear("Output", "dB", -24.0, 24.0, 0.0),
    choice("Bypass", &["Off", "On"], 0.0),
];

const BAND_SPECS: [Spec; ORIGINAL_BAND_FIELDS] = [
    choice("Band", &["Off", "On"], 0.0),
    choice("Solo", &["Off", "On"], 0.0),
    log("Low", "Hz", 20.0, 20000.0, 80.0),
    log("High", "Hz", 20.0, 20000.0, 240.0),
    choice(
        "Mode",
        &["Compress ↓", "Compress ↑", "Expand ↓", "Expand ↑"],
        0.0,
    ),
    linear("Threshold", "dB", -72.0, 0.0, -24.0),
    log("Ratio", ":1", 1.0, 30.0, 3.0),
    linear("Range", "dB", 0.0, 36.0, 12.0),
    linear("Knee", "dB", 0.0, 24.0, 6.0),
    log("Attack", "ms", 0.1, 200.0, 12.0),
    log("Release", "ms", 5.0, 2000.0, 120.0),
    linear("Makeup", "dB", -18.0, 18.0, 0.0),
    choice("Channels", &["Stereo", "Mid", "Side"], 0.0),
    linear("Link", "%", 0.0, 100.0, 100.0),
    choice("Key", &["Band", "Custom", "External"], 0.0),
    log("Key low", "Hz", 20.0, 20000.0, 20.0),
    log("Key high", "Hz", 20.0, 20000.0, 20000.0),
];

const LEGACY_EDGES: [f64; BANDS + 1] = [20.0, 90.0, 280.0, 900.0, 2800.0, 7200.0, 20000.0];
const DEFAULT_CROSSOVERS: [f64; 4] = [20.0, 120.0, 3000.0, 20000.0];
const DEFAULT_BANDS: usize = DEFAULT_CROSSOVERS.len() - 1;

pub fn spec(id: usize) -> Spec {
    if id < GLOBALS {
        return GLOBAL_SPECS[id];
    }
    if id == LOOKAHEAD {
        return choice("Lookahead engine", &["Off", "On"], 0.0);
    }
    if id >= AUDITION_START {
        return match (id - AUDITION_START) % 3 {
            0 => choice("Mute", &["Off", "On"], 0.0),
            1 => choice("Audition key", &["Off", "On"], 0.0),
            _ => choice("Key filter", &["Custom", "Band"], 0.0),
        };
    }
    if id >= PRESENCE_START {
        return choice(
            "Exists",
            &["No", "Yes"],
            f64::from(id - PRESENCE_START < DEFAULT_BANDS),
        );
    }
    if id == PHASE {
        return choice("Phase", &["Minimum", "Linear"], 0.0);
    }
    if id >= SLOPES_START {
        let name = if (id - SLOPES_START).is_multiple_of(2) {
            "Low slope"
        } else {
            "High slope"
        };
        return choice(
            name,
            &[
                "6 dB/oct",
                "12 dB/oct",
                "18 dB/oct",
                "24 dB/oct",
                "30 dB/oct",
                "36 dB/oct",
                "42 dB/oct",
                "48 dB/oct",
            ],
            1.0,
        );
    }
    if id >= NEW_BANDS {
        return linear("Lookahead", "ms", 0.0, MAX_LOOKAHEAD_MS, 0.0);
    }
    let band = (id - GLOBALS) / ORIGINAL_BAND_FIELDS;
    let field = (id - GLOBALS) % ORIGINAL_BAND_FIELDS;
    let mut spec = BAND_SPECS[field];
    if field == BandParam::Low as usize {
        spec.default = if band < DEFAULT_BANDS {
            DEFAULT_CROSSOVERS[band]
        } else {
            LEGACY_EDGES[band]
        };
    } else if field == BandParam::High as usize {
        spec.default = if band < DEFAULT_BANDS {
            DEFAULT_CROSSOVERS[band + 1]
        } else {
            LEGACY_EDGES[band + 1]
        };
    } else if field == BandParam::Enabled as usize && band < DEFAULT_BANDS {
        spec.default = 1.0;
    } else if (field == BandParam::Threshold as usize || field == BandParam::Knee as usize)
        && band < DEFAULT_BANDS
    {
        spec.default = 0.0;
    }
    spec
}

pub fn host_name(id: usize) -> String {
    let name = spec(id).name;
    match id {
        id if id < GLOBALS || configuration_param(id) => name.to_owned(),
        id if id >= AUDITION_START => format!("Band {} {name}", (id - AUDITION_START) / 3 + 1),
        id if id >= PRESENCE_START => format!("Band {} {name}", id - PRESENCE_START + 1),
        id if id >= SLOPES_START => format!("Band {} {name}", (id - SLOPES_START) / 2 + 1),
        id if id >= NEW_BANDS => format!("Band {} {name}", id - NEW_BANDS + 1),
        _ => format!("Band {} {name}", (id - GLOBALS) / ORIGINAL_BAND_FIELDS + 1),
    }
}

pub fn present(values: &Values, band: usize) -> bool {
    values[BandParam::Present.id(band)] > 0.5 || values[BandParam::Enabled.id(band)] > 0.5
}

pub fn presence_param(id: usize) -> bool {
    (PRESENCE_START..PRESENCE_START + BANDS).contains(&id)
}

pub fn configuration_param(id: usize) -> bool {
    matches!(id, LOOKAHEAD | PHASE)
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub struct Configuration(u8);

impl Configuration {
    pub fn read(values: &Values) -> Self {
        Self(u8::from(values[LOOKAHEAD] >= 0.5) | (u8::from(values[PHASE] >= 0.5) << 1))
    }

    pub fn write(self, values: &mut Values) {
        values[LOOKAHEAD] = f64::from(self.lookahead());
        values[PHASE] = f64::from(self.linear());
    }

    pub fn lookahead(self) -> bool {
        self.0 & 1 != 0
    }

    pub fn linear(self) -> bool {
        self.0 & 2 != 0
    }

    pub fn lookahead_samples(self, rate: f64) -> usize {
        if self.lookahead() {
            (rate * MAX_LOOKAHEAD_MS * 0.001).ceil() as usize
        } else {
            0
        }
    }

    #[cfg(all(feature = "plugin", target_os = "macos"))]
    pub(crate) fn with_param(self, id: usize, value: f64) -> Self {
        match id {
            LOOKAHEAD => Self((self.0 & !1) | u8::from(value >= 0.5)),
            PHASE => Self((self.0 & !2) | (u8::from(value >= 0.5) << 1)),
            _ => self,
        }
    }

    #[cfg(all(feature = "plugin", target_os = "macos"))]
    pub(crate) fn bits(self) -> u8 {
        self.0
    }
    #[cfg(all(feature = "plugin", target_os = "macos"))]
    pub(crate) fn from_bits(bits: u8) -> Self {
        Self(bits & 3)
    }
}

impl Spec {
    pub fn plain(self, normalized: f64) -> f64 {
        let normalized = normalized.clamp(0.0, 1.0);
        if self.log {
            self.min * (self.max / self.min).powf(normalized)
        } else {
            let value = self.min + (self.max - self.min) * normalized;
            if self.choices.is_empty() {
                value
            } else {
                value.round()
            }
        }
    }

    pub fn normalized(self, plain: f64) -> f64 {
        let plain = plain.clamp(self.min, self.max);
        if self.log {
            (plain / self.min).ln() / (self.max / self.min).ln()
        } else {
            (plain - self.min) / (self.max - self.min)
        }
    }

    pub fn default_normalized(self) -> f64 {
        self.normalized(self.default)
    }

    pub fn canonical(self, normalized: f64) -> f64 {
        if !normalized.is_finite() {
            return self.default_normalized();
        }
        if self.choices.is_empty() {
            normalized.clamp(0.0, 1.0)
        } else {
            self.normalized(self.plain(normalized))
        }
    }

    pub fn display(self, normalized: f64) -> String {
        let value = self.plain(normalized);
        if !self.choices.is_empty() {
            return self.choices[value as usize].to_owned();
        }
        match self.unit {
            "Hz" if value >= 1000.0 => format!("{:.2} kHz", value / 1000.0),
            "Hz" | "%" => format!("{value:.0} {}", self.unit),
            ":1" => format!("{value:.2}:1"),
            _ => format!("{value:.1} {}", self.unit),
        }
    }
}

pub fn defaults() -> Values {
    std::array::from_fn(|id| spec(id).default_normalized())
}

fn legacy_defaults() -> Values {
    let mut values = defaults();
    for band in 0..BANDS {
        let low = BandParam::Low.id(band);
        let high = BandParam::High.id(band);
        values[low] = spec(low).normalized(LEGACY_EDGES[band]);
        values[high] = spec(high).normalized(LEGACY_EDGES[band + 1]);
        for field in [
            BandParam::Enabled,
            BandParam::Present,
            BandParam::Threshold,
            BandParam::Knee,
        ] {
            let id = field.id(band);
            values[id] = match field {
                BandParam::Present => 0.0,
                _ => BAND_SPECS[field as usize].default_normalized(),
            };
        }
    }
    values
}

#[cfg(test)]
pub fn empty() -> Values {
    let mut values = defaults();
    for band in 0..BANDS {
        values[BandParam::Present.id(band)] = 0.0;
        values[BandParam::Enabled.id(band)] = 0.0;
    }
    values
}

pub fn encode(values: &Values) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

pub fn decode(bytes: &[u8], version: u32) -> Option<Values> {
    let fields = match version {
        1 => LEGACY_COUNT,
        2 => V2_COUNT,
        3 => V3_COUNT,
        4 => V4_COUNT,
        5 | 6 => V6_COUNT,
        7 => COUNT,
        _ => return None,
    };
    if bytes.len() != fields * size_of::<f64>() {
        return None;
    }
    let mut values = if version < 5 {
        legacy_defaults()
    } else {
        defaults()
    };
    for (id, bytes) in bytes.as_chunks::<8>().0.iter().enumerate() {
        let value = f64::from_le_bytes(*bytes);
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return None;
        }
        values[id] = spec(id).canonical(value);
    }
    if version < 5 {
        let baseline = legacy_defaults();
        for band in 0..BANDS {
            let touched = BandParam::ALL.iter().any(|field| {
                let id = field.id(band);
                values[id] != baseline[id]
            });
            values[BandParam::Present.id(band)] = f64::from(touched);
        }
    }
    Some(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_and_invalid_state_is_rejected() {
        let values = defaults();
        let mut bytes = encode(&values);
        assert_eq!(decode(&bytes, 7), Some(values));
        let mut old = legacy_defaults();
        old[BandParam::Threshold.id(4)] = 0.378;
        let old_payload = encode(&old);
        old[BandParam::Present.id(4)] = 1.0;
        assert_eq!(decode(&old_payload[..LEGACY_COUNT * 8], 1), Some(old));
        assert_eq!(BandParam::Threshold.id(4), 76);
        assert_eq!(decode(&bytes[1..], 7), None);
        bytes[..8].copy_from_slice(&f64::NAN.to_le_bytes());
        assert_eq!(decode(&bytes, 7), None);
    }
}
