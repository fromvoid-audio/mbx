use super::STATE_VERSION;
use crate::parameters::{self, Values};

pub const VERSION: u32 = 8;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Slot {
    #[default]
    A,
    B,
}

impl Slot {
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

#[derive(Default)]
pub struct Comparison {
    active: Slot,
    alternate: Option<Values>,
}

impl Comparison {
    pub fn active(&self) -> Slot {
        self.active
    }

    pub fn switch_to(&mut self, slot: Slot, current: Values) -> Option<Values> {
        if slot == self.active {
            return None;
        }
        self.active = slot;
        Some(self.alternate.replace(current).unwrap_or(current))
    }

    pub fn differs(&self, current: &Values) -> bool {
        self.alternate
            .as_ref()
            .is_some_and(|alternate| alternate != current)
    }

    pub fn copy(&mut self, current: Values) {
        self.alternate = Some(current);
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![
            self.active.index() as u8,
            u8::from(self.alternate.is_some()),
        ];
        if let Some(values) = &self.alternate {
            bytes.extend(parameters::encode(values));
        }
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let (&slot, rest) = bytes.split_first()?;
        let (&has_alternate, payload) = rest.split_first()?;
        let active = match slot {
            0 => Slot::A,
            1 => Slot::B,
            _ => return None,
        };
        let alternate = match has_alternate {
            0 if payload.is_empty() => None,
            1 => Some(parameters::decode(payload, STATE_VERSION)?),
            _ => return None,
        };
        Some(Self { active, alternate })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::{LOOKAHEAD, MIX, PHASE};

    #[test]
    fn compares_whole_settings_and_copies_the_active_slot() {
        let a = parameters::defaults();
        let mut b = a;
        b[MIX] = 0.2;
        b[PHASE] = 1.0;
        b[LOOKAHEAD] = 1.0;
        let mut comparison = Comparison::default();
        assert_eq!(comparison.switch_to(Slot::B, a), Some(a));
        assert!(comparison.differs(&b));
        assert_eq!(comparison.switch_to(Slot::A, b), Some(a));
        assert_eq!(comparison.switch_to(Slot::B, a), Some(b));
        comparison.copy(b);
        assert!(!comparison.differs(&b));
        assert_eq!(comparison.switch_to(Slot::A, b), Some(b));
    }

    #[test]
    fn alternate_survives_controller_state_round_trip() {
        let mut a = parameters::defaults();
        a[MIX] = 0.72;
        let mut comparison = Comparison::default();
        comparison.switch_to(Slot::B, a);
        let mut restored = Comparison::decode(&comparison.encode()).unwrap();
        assert_eq!(restored.active(), Slot::B);
        assert_eq!(restored.switch_to(Slot::A, parameters::defaults()), Some(a));
        assert!(Comparison::decode(&[3, 0]).is_none());
        assert!(Comparison::decode(&[0, 1]).is_none());
    }
}
