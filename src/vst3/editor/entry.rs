use crate::parameters::Spec;

pub struct Entry {
    pub id: usize,
    pub text: String,
    pub replace: bool,
    pub invalid: bool,
}

impl Entry {
    pub fn new(id: usize, value: f64) -> Self {
        let text = format!("{value:.4}");
        Self {
            id,
            text: text.trim_end_matches('0').trim_end_matches('.').to_owned(),
            replace: true,
            invalid: false,
        }
    }

    pub fn append(&mut self, text: &str) {
        if self.replace {
            self.text.clear();
            self.replace = false;
        }
        self.text.extend(
            text.chars()
                .take(32_usize.saturating_sub(self.text.chars().count())),
        );
        self.invalid = false;
    }
}

pub fn parse_plain(spec: Spec, input: &str) -> Option<f64> {
    let text = input.trim().to_ascii_lowercase().replace(',', ".");
    let special = match spec.unit {
        "Hz" => Some(("khz", 1000.0)),
        "ms" => Some(("s", 1000.0)),
        _ => None,
    };
    let unit = spec.unit.to_ascii_lowercase();
    let (number, multiplier) = if !unit.is_empty()
        && text.ends_with(&unit)
        && special.is_none_or(|(suffix, _)| suffix.len() <= unit.len() || !text.ends_with(suffix))
    {
        (text.strip_suffix(&unit)?, 1.0)
    } else if let Some((suffix, multiplier)) = special.filter(|(suffix, _)| text.ends_with(suffix))
    {
        (text.strip_suffix(suffix)?, multiplier)
    } else {
        (text.as_str(), 1.0)
    };
    let value = number.trim().parse::<f64>().ok()? * multiplier;
    value.is_finite().then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::{self, BandParam as P};

    #[test]
    fn value_entry_understands_display_units() {
        let edge = parameters::spec(P::Low.id(0));
        assert_eq!(parse_plain(edge, "1,5 kHz"), Some(1500.0));
        let release = parameters::spec(P::Release.id(0));
        assert_eq!(parse_plain(release, "120 ms"), Some(120.0));
        assert_eq!(parse_plain(release, "0.12 s"), Some(120.0));
        assert_eq!(parse_plain(release, "nan"), None);
        let range = parameters::spec(P::Range.id(0));
        assert_eq!(parse_plain(range, "-4 dB"), Some(-4.0));
    }
}
