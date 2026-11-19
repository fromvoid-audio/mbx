use super::{STATE_MAGIC, STATE_VERSION, STATE_VERSIONS};
use crate::parameters::{self, BandParam as P, Values};
use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
use toybox::vst3::prelude::{decode_versioned_payload, try_encode_versioned_payload};

const MAX_PRESET_BYTES: u64 = 16 * 1024;
pub const EXTENSION: &str = "mbx";

#[derive(Clone, Copy)]
pub struct FactoryPreset {
    pub name: &'static str,
    pub description: &'static str,
    adjust: fn(&mut Values),
}

pub const FACTORY: &[FactoryPreset] = &[
    FactoryPreset {
        name: "Default",
        description: "Neutral three-band starting point",
        adjust: |_| {},
    },
    FactoryPreset {
        name: "Gentle Mix Bus",
        description: "Small, slow gain changes across the mix",
        adjust: |values| {
            for (band, threshold) in [-23.0, -23.0, -26.0].into_iter().enumerate() {
                compression(values, band, [threshold, 1.5, 2.5, 30.0, 160.0]);
            }
        },
    },
    FactoryPreset {
        name: "Low-End Control",
        description: "Contain bass peaks while leaving mids and highs open",
        adjust: |values| compression(values, 0, [-23.0, 3.0, 6.0, 14.0, 125.0]),
    },
    FactoryPreset {
        name: "Vocal Presence",
        description: "Tame upper-mid peaks and soften sibilance",
        adjust: |values| {
            crossover(values, 0, 260.0);
            crossover(values, 1, 5300.0);
            compression(values, 1, [-27.0, 2.0, 4.0, 10.0, 95.0]);
            compression(values, 2, [-32.0, 3.0, 5.0, 1.5, 75.0]);
        },
    },
    FactoryPreset {
        name: "De-Ess Highs",
        description: "Fast compression of sharp high-frequency peaks",
        adjust: |values| {
            crossover(values, 1, 5400.0);
            compression(values, 2, [-32.0, 4.0, 8.0, 1.0, 65.0]);
        },
    },
    FactoryPreset {
        name: "Percussion Punch",
        description: "Expand low and high transients with capped gain",
        adjust: |values| {
            for band in [0, 2] {
                compression(values, band, [-24.0, 1.8, 3.0, 1.5, 75.0]);
                set(values, P::Mode.id(band), 3.0);
            }
        },
    },
];

impl FactoryPreset {
    pub fn values(self) -> Values {
        let mut values = parameters::defaults();
        (self.adjust)(&mut values);
        values
    }
}

fn set(values: &mut Values, id: usize, plain: f64) {
    values[id] = parameters::spec(id).normalized(plain);
}

fn crossover(values: &mut Values, left: usize, hz: f64) {
    set(values, P::High.id(left), hz);
    set(values, P::Low.id(left + 1), hz);
}

fn compression(
    values: &mut Values,
    band: usize,
    [threshold, ratio, range, attack, release]: [f64; 5],
) {
    for (field, plain) in [
        (P::Threshold, threshold),
        (P::Ratio, ratio),
        (P::Range, range),
        (P::Attack, attack),
        (P::Release, release),
        (P::Knee, 6.0),
    ] {
        set(values, field.id(band), plain);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Source {
    Factory(usize),
    User(PathBuf),
}

impl Source {
    pub fn name(&self) -> &str {
        match self {
            Self::Factory(index) => FACTORY[*index].name,
            Self::User(path) => path
                .file_stem()
                .and_then(OsStr::to_str)
                .unwrap_or("User preset"),
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Factory(index) => FACTORY[*index].description,
            Self::User(_) => "",
        }
    }

    pub fn values(&self) -> io::Result<Values> {
        match self {
            Self::Factory(index) => Ok(FACTORY[*index].values()),
            Self::User(path) => load_file(path),
        }
    }
}

pub fn default_folder() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|path| !path.is_empty())
        .map(|home| PathBuf::from(home).join("Library/Application Support/MBX/Presets"))
}

pub fn catalog(folder: Option<&Path>) -> io::Result<Vec<Source>> {
    let mut sources = (0..FACTORY.len()).map(Source::Factory).collect::<Vec<_>>();
    let Some(folder) = folder else {
        return Ok(sources);
    };
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(sources),
        Err(error) => return Err(error),
    };
    let mut user = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_file()
            && path.extension() == Some(OsStr::new(EXTENSION))
            && path.file_stem().and_then(OsStr::to_str).is_some()
        {
            user.push(Source::User(path));
        }
    }
    user.sort_by(|a, b| {
        a.name()
            .to_lowercase()
            .cmp(&b.name().to_lowercase())
            .then_with(|| a.name().cmp(b.name()))
    });
    sources.extend(user);
    Ok(sources)
}

fn load_file(path: &Path) -> io::Result<Values> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_PRESET_BYTES + 1)
        .read_to_end(&mut bytes)?;
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Unsupported or damaged MBX preset",
        )
    };
    if bytes.len() as u64 > MAX_PRESET_BYTES {
        return Err(invalid());
    }
    let payload =
        decode_versioned_payload(&bytes, STATE_MAGIC, STATE_VERSIONS).map_err(|_| invalid())?;
    parameters::decode(&payload.payload, payload.version).ok_or_else(invalid)
}

pub fn save_file(folder: &Path, name: &str, values: &Values) -> io::Result<Source> {
    let name = name.trim();
    if name.is_empty()
        || name.starts_with('.')
        || name.chars().count() > 60
        || name
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '/' | '\\' | ':'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Enter a valid preset name",
        ));
    }

    fs::create_dir_all(folder)?;
    let path = folder.join(format!("{name}.{EXTENSION}"));
    let bytes =
        try_encode_versioned_payload(STATE_MAGIC, STATE_VERSION, &parameters::encode(values))
            .map_err(|error| io::Error::other(format!("Could not encode preset: {error:?}")))?;
    let mut staging = tempfile::NamedTempFile::new_in(folder)?;
    staging.write_all(&bytes)?;
    staging
        .persist_noclobber(&path)
        .map_err(|error| error.error)?;
    Ok(Source::User(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::layout::Layout;

    #[test]
    fn factory_settings_keep_three_adjacent_bands_and_default_is_exact() {
        assert_eq!(FACTORY[0].values(), parameters::defaults());
        for preset in FACTORY {
            let values = preset.values();
            let layout = Layout::new(&values);
            assert_eq!(layout.order(), &[0, 1, 2], "{}", preset.name);
            assert_eq!(layout.edges[0][0], 0.0);
            assert_eq!(layout.edges[2][1], 1.0);
            assert_eq!(layout.edges[0][1], layout.edges[1][0]);
            assert_eq!(layout.edges[1][1], layout.edges[2][0]);
        }
    }

    #[test]
    fn user_library_round_trips_and_never_overwrites() {
        let folder = tempfile::tempdir().unwrap();
        let a = FACTORY[1].values();
        let b = FACTORY[2].values();
        let saved = save_file(folder.path(), "Bass", &a).unwrap();
        assert_eq!(saved.values().unwrap(), a);
        assert_eq!(
            save_file(folder.path(), "Bass", &b).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(saved.values().unwrap(), a);
        assert!(save_file(folder.path(), "../outside", &a).is_err());
        fs::write(folder.path().join("Broken.mbx"), b"bad").unwrap();
        let visible = catalog(Some(folder.path())).unwrap();
        assert_eq!(visible[FACTORY.len()], saved);
        assert_eq!(
            visible.last().unwrap().values().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
