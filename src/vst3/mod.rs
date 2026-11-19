//! Toybox VST3 adapter and native editor.

mod comparison;
mod controller;
mod editor;
mod factory;
mod meters;
mod presets;
mod processor;
mod view;

use crate::parameters::{self, Values};
use std::sync::atomic::{AtomicU64, Ordering};
use toybox::vst3::prelude::Steinberg::{kResultFalse, kResultOk, tresult, IBStream};
use toybox::vst3::prelude::{read_versioned_payload, write_versioned_payload};

const NAME: &str = "MBX";
const STATE_MAGIC: u32 = u32::from_le_bytes(*b"MBX1");
const STATE_VERSION: u32 = 7;
const STATE_VERSIONS: &[u32] = &[1, 2, 3, 4, 5, 6, STATE_VERSION];

struct Parameters([AtomicU64; parameters::COUNT]);

impl Parameters {
    fn new() -> Self {
        Self(parameters::defaults().map(|value| AtomicU64::new(value.to_bits())))
    }

    fn get(&self, id: usize) -> f64 {
        f64::from_bits(self.0[id].load(Ordering::Relaxed))
    }

    fn set(&self, id: usize, value: f64) {
        let value = parameters::spec(id).canonical(value);
        self.0[id].store(value.to_bits(), Ordering::Relaxed);
    }

    fn values(&self) -> Values {
        std::array::from_fn(|id| self.get(id))
    }

    fn replace(&self, values: &Values) {
        for (id, &value) in values.iter().enumerate() {
            self.set(id, value);
        }
    }
}

unsafe fn load(stream: *mut IBStream) -> Option<Values> {
    let data = unsafe { read_versioned_payload(stream, STATE_MAGIC, STATE_VERSIONS) }.ok()?;
    parameters::decode(&data.payload, data.version)
}

unsafe fn save(stream: *mut IBStream, values: &Values) -> tresult {
    match unsafe {
        write_versioned_payload(
            stream,
            STATE_MAGIC,
            STATE_VERSION,
            &parameters::encode(values),
        )
    } {
        Ok(()) => kResultOk,
        Err(_) => kResultFalse,
    }
}

toybox::vst3_plugin_entry!(factory::Factory);
