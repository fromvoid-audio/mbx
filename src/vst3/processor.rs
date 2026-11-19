use super::{controller::Controller, load, meters::Meters, save};
use crate::{
    dynamics::Engine,
    parameters::{self, Configuration, Values, BANDS, COUNT},
};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
    Arc, Mutex,
};
use toybox::vst3::prelude::{Steinberg::*, *};

const DEFAULT_RATE: f64 = 48_000.0;
const CONTROL_INTERVAL: usize = 16;

pub struct Processor {
    connection: InstanceConnection<Meters>,
    meters: Arc<Meters>,
    layout: AtomicU64,
    side_active: AtomicBool,
    rate: AtomicU64,
    runtime: Mutex<Runtime>,
    state: Mutex<SavedState>,
}

struct SavedState {
    values: Values,
    pending: bool,
}

struct Runtime {
    configuration: Configuration,
    engine: Engine,
    values: Values,
    applied: Values,
    phase: usize,
    meter_cursor: usize,
    dirty: bool,
}

impl Runtime {
    fn new(rate: f64, values: Values) -> Self {
        Self {
            configuration: Configuration::read(&values),
            engine: Engine::new(rate, &values),
            values,
            applied: values,
            phase: 0,
            meter_cursor: 0,
            dirty: false,
        }
    }

    fn update(&mut self) {
        if self.values != self.applied {
            self.engine.update(&self.values);
            self.applied = self.values;
        }
    }
}

impl Processor {
    pub const CID: TUID = uid(0xD2AE4195, 0x3CBD43A2, 0x9856D8EF, 0x9B0614A2);

    pub fn new() -> Self {
        let meters = Arc::new(Meters::new());
        let values = parameters::defaults();
        Self {
            connection: InstanceConnection::new(
                InstanceConnectionRole::Processor,
                Arc::clone(&meters),
            ),
            meters,
            layout: AtomicU64::new(SpeakerArr::kStereo | (SpeakerArr::kStereo << 32)),
            side_active: AtomicBool::new(false),
            rate: AtomicU64::new(DEFAULT_RATE.to_bits()),
            runtime: Mutex::new(Runtime::new(DEFAULT_RATE, values)),
            state: Mutex::new(SavedState {
                values,
                pending: false,
            }),
        }
    }

    fn configure(&self, rate: f64) -> tresult {
        let Ok(mut runtime) = self.runtime.try_lock() else {
            return kResultFalse;
        };
        let Ok(mut state) = self.state.lock() else {
            return kResultFalse;
        };
        let mut values = if state.pending {
            state.values
        } else {
            runtime.values
        };
        self.meters.configuration().write(&mut values);
        *runtime = Runtime::new(rate, values);
        *state = SavedState {
            values,
            pending: false,
        };
        self.rate.store(rate.to_bits(), Relaxed);
        self.meters
            .latency
            .store(runtime.engine.latency() as u32, Relaxed);
        self.meters.reset(rate);
        kResultOk
    }

    fn reset(&self) -> tresult {
        let Ok(mut runtime) = self.runtime.try_lock() else {
            return kResultFalse;
        };
        if let Ok(mut state) = self.state.try_lock() {
            if state.pending {
                runtime.values = state.values;
                state.pending = false;
            }
        }
        let mut values = runtime.values;
        runtime.configuration.write(&mut values);
        runtime.engine.reset(&values);
        runtime.values = values;
        runtime.applied = values;
        runtime.phase = 0;
        runtime.meter_cursor = 0;
        self.meters.reset(f64::from_bits(self.rate.load(Relaxed)));
        kResultOk
    }
}

impl Class for Processor {
    type Interfaces = (
        IComponent,
        IAudioProcessor,
        IConnectionPoint,
        IToyboxSharedState,
        IProcessContextRequirements,
    );
}

toybox::impl_vst3_instance_connection!(Processor, connection);

impl IPluginBaseTrait for Processor {
    unsafe fn initialize(&self, _: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for Processor {
    unsafe fn getControllerClassId(&self, id: *mut TUID) -> tresult {
        let Some(id) = (unsafe { id.as_mut() }) else {
            return kInvalidArgument;
        };
        *id = Controller::CID;
        kResultOk
    }

    unsafe fn setIoMode(&self, _: IoMode) -> tresult {
        kResultOk
    }

    unsafe fn getBusCount(&self, kind: MediaType, direction: BusDirection) -> i32 {
        if kind != MediaTypes_::kAudio as i32 {
            return 0;
        }
        match direction as BusDirections {
            BusDirections_::kInput => 2,
            BusDirections_::kOutput => 1,
            _ => 0,
        }
    }

    unsafe fn getBusInfo(
        &self,
        kind: MediaType,
        direction: BusDirection,
        index: i32,
        bus: *mut BusInfo,
    ) -> tresult {
        let Some(bus) = (unsafe { bus.as_mut() }) else {
            return kInvalidArgument;
        };
        let count = unsafe { self.getBusCount(kind, direction) };
        if !(0..count).contains(&index) {
            return kInvalidArgument;
        }
        let label = if index == 1 {
            "Sidechain"
        } else if direction == BusDirections_::kInput as i32 {
            "Input"
        } else {
            "Output"
        };
        let layout = self.layout.load(Relaxed);
        let arrangement = if index == 1 {
            layout >> 32
        } else {
            layout & u64::from(u32::MAX)
        };
        bus.mediaType = kind;
        bus.direction = direction;
        bus.channelCount = arrangement.count_ones() as i32;
        copy_wstring(label, &mut bus.name);
        bus.busType = if index == 1 {
            BusTypes_::kAux
        } else {
            BusTypes_::kMain
        } as BusType;
        bus.flags = if index == 1 {
            0
        } else {
            BusInfo_::BusFlags_::kDefaultActive
        };
        kResultOk
    }

    unsafe fn getRoutingInfo(&self, _: *mut RoutingInfo, _: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }

    unsafe fn activateBus(
        &self,
        kind: MediaType,
        direction: BusDirection,
        index: i32,
        active: TBool,
    ) -> tresult {
        if !(0..unsafe { self.getBusCount(kind, direction) }).contains(&index) {
            return kInvalidArgument;
        }
        if index == 1 {
            self.side_active.store(active != 0, Relaxed);
            self.meters.sidechain.store(u32::from(active != 0), Relaxed);
        }
        kResultOk
    }

    unsafe fn setActive(&self, active: TBool) -> tresult {
        if active == 0 {
            kResultOk
        } else {
            self.configure(f64::from_bits(self.rate.load(Relaxed)))
        }
    }

    unsafe fn setState(&self, stream: *mut IBStream) -> tresult {
        let Some(values) = (unsafe { load(stream) }) else {
            return kInvalidArgument;
        };
        let Ok(mut state) = self.state.lock() else {
            return kResultFalse;
        };
        self.meters
            .replace_configuration(Configuration::read(&values));
        *state = SavedState {
            values,
            pending: true,
        };
        kResultOk
    }

    unsafe fn getState(&self, stream: *mut IBStream) -> tresult {
        let mut values = match self.state.lock() {
            Ok(state) => state.values,
            Err(_) => return kResultFalse,
        };
        self.meters.configuration().write(&mut values);
        unsafe { save(stream, &values) }
    }
}

impl IProcessContextRequirementsTrait for Processor {
    unsafe fn getProcessContextRequirements(&self) -> u32 {
        0
    }
}

impl IAudioProcessorTrait for Processor {
    unsafe fn setBusArrangements(
        &self,
        inputs: *mut SpeakerArrangement,
        input_count: i32,
        outputs: *mut SpeakerArrangement,
        output_count: i32,
    ) -> tresult {
        if !(1..=2).contains(&input_count)
            || output_count != 1
            || inputs.is_null()
            || outputs.is_null()
        {
            return kResultFalse;
        }
        let main = unsafe { *inputs };
        let side = if input_count == 2 {
            unsafe { *inputs.add(1) }
        } else {
            SpeakerArr::kStereo
        };
        let supported =
            |arrangement| matches!(arrangement, SpeakerArr::kMono | SpeakerArr::kStereo);
        if main != unsafe { *outputs } || !supported(main) || !supported(side) {
            return kResultFalse;
        }
        self.layout.store(main | (side << 32), Relaxed);
        self.meters
            .output_channels
            .store(if main == SpeakerArr::kMono { 1 } else { 2 }, Relaxed);
        kResultTrue
    }

    unsafe fn getBusArrangement(
        &self,
        direction: BusDirection,
        index: i32,
        arrangement: *mut SpeakerArrangement,
    ) -> tresult {
        let Some(arrangement) = (unsafe { arrangement.as_mut() }) else {
            return kInvalidArgument;
        };
        let layout = self.layout.load(Relaxed);
        *arrangement = match (direction as BusDirections, index) {
            (BusDirections_::kInput | BusDirections_::kOutput, 0) => layout & u64::from(u32::MAX),
            (BusDirections_::kInput, 1) => layout >> 32,
            _ => return kInvalidArgument,
        };
        kResultOk
    }

    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        match size as SymbolicSampleSizes {
            SymbolicSampleSizes_::kSample32 | SymbolicSampleSizes_::kSample64 => kResultOk,
            _ => kResultFalse,
        }
    }

    unsafe fn getLatencySamples(&self) -> u32 {
        self.meters.latency.load(Relaxed)
    }

    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        let Some(setup) = (unsafe { setup.as_ref() }) else {
            return kInvalidArgument;
        };
        if !(100.0..=2_000_000.0).contains(&setup.sampleRate) {
            return kInvalidArgument;
        }
        self.configure(setup.sampleRate)
    }

    unsafe fn setProcessing(&self, active: TBool) -> tresult {
        if active == 0 {
            kResultOk
        } else {
            self.reset()
        }
    }

    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let Some(data) = (unsafe { data.as_ref() }) else {
            return kInvalidArgument;
        };
        if data.numSamples < 0 {
            return kInvalidArgument;
        }
        let Ok(mut runtime) = self.runtime.try_lock() else {
            return kResultFalse;
        };
        if let Ok(mut state) = self.state.try_lock() {
            if state.pending {
                runtime.values = state.values;
                runtime.update();
                runtime.dirty = false;
                state.pending = false;
            }
        }
        let result = match data.symbolicSampleSize as SymbolicSampleSizes {
            SymbolicSampleSizes_::kSample32 => unsafe { self.render::<f32>(data, &mut runtime) },
            SymbolicSampleSizes_::kSample64 => unsafe { self.render::<f64>(data, &mut runtime) },
            _ => kInvalidArgument,
        };
        if runtime.dirty {
            if let Ok(mut state) = self.state.try_lock() {
                if !state.pending {
                    state.values = runtime.values;
                    runtime.dirty = false;
                }
            }
        }
        result
    }

    unsafe fn getTailSamples(&self) -> u32 {
        f64::from_bits(self.rate.load(Relaxed)) as u32 + self.meters.latency.load(Relaxed)
    }
}

impl Processor {
    /// Host channel buffers may alias in-place. All channel reads at each frame
    /// are evaluated before any store, using raw pointers throughout.
    unsafe fn render<T: Sample>(&self, data: &ProcessData, runtime: &mut Runtime) -> tresult {
        let mut automation = unsafe { Queues::new(data, &runtime.values) };
        if data.numSamples == 0 {
            runtime.dirty |= automation.apply(0, &mut runtime.values).0;
            return kResultOk;
        }
        if !(1..=2).contains(&data.numInputs)
            || data.numOutputs != 1
            || data.inputs.is_null()
            || data.outputs.is_null()
        {
            return kInvalidArgument;
        }
        // A host may deliver parameter flush blocks without audio channel buffers.
        if unsafe { (*data.inputs).numChannels == 0 && (*data.outputs).numChannels == 0 } {
            runtime.dirty |= automation.apply(data.numSamples, &mut runtime.values).0;
            return kResultOk;
        }
        let Some(input) = (unsafe { Channels::<T>::new(data.inputs) }) else {
            return kInvalidArgument;
        };
        let Some(output) = (unsafe { Channels::<T>::new(data.outputs) }) else {
            return kInvalidArgument;
        };
        if input.count != output.count {
            return kInvalidArgument;
        }
        let key = if data.numInputs == 2 && self.side_active.load(Relaxed) {
            unsafe { Channels::<T>::new(data.inputs.add(1)) }
        } else {
            None
        };
        let silence = unsafe { (*data.inputs).silenceFlags };
        let key_silence = if key.is_some() {
            unsafe { (*data.inputs.add(1)).silenceFlags }
        } else {
            0
        };
        let capture = self.meters.wanted();
        let mut peak = [0.0_f64; 2];
        let mut activity = [0.0_f64; BANDS];
        let mut detector_db = [-90.0_f64; BANDS];
        let mut signal_flags = 0;
        for index in 0..data.numSamples {
            let (changed, discrete) = automation.apply(index, &mut runtime.values);
            runtime.dirty |= changed;
            if runtime.phase == 0 || discrete {
                runtime.update();
            }
            runtime.phase = (runtime.phase + 1) % CONTROL_INTERVAL;
            let input = unsafe { input.read(index as usize, silence) };
            let detector = key.as_ref().map_or([0.0; 2], |key| unsafe {
                key.read(index as usize, key_silence)
            });
            let frame = runtime.engine.tick(input, detector);
            signal_flags |= unsafe { output.write(index as usize, frame.output) };
            if capture {
                for (meter, sample) in peak.iter_mut().zip(frame.output).take(output.count) {
                    *meter = meter.max(sample.abs());
                }
                for (meter, sample) in activity.iter_mut().zip(frame.activity) {
                    if sample.abs() > meter.abs() {
                        *meter = sample;
                    }
                }
                for (meter, value) in detector_db.iter_mut().zip(frame.detector_db) {
                    *meter = meter.max(value);
                }
                self.meters
                    .sample(&mut runtime.meter_cursor, [input, frame.output, detector]);
            }
        }

        runtime.dirty |= automation.apply(data.numSamples, &mut runtime.values).0;
        unsafe { (*data.outputs).silenceFlags = !signal_flags & ((1 << output.count) - 1) };
        if capture {
            for (meter, value) in self.meters.gain.iter().zip(activity) {
                meter.store((value as f32).to_bits(), Relaxed);
            }
            for (meter, value) in self.meters.detector.iter().zip(detector_db) {
                meter.store((value as f32).to_bits(), Relaxed);
            }
            for (meter, value) in self.meters.peak.iter().zip(peak) {
                meter.fetch_max((value as f32).to_bits(), Relaxed);
            }
        }
        kResultOk
    }
}

trait Sample: Copy {
    unsafe fn array(bus: *const AudioBusBuffers) -> *mut *mut Self;
    fn from_audio(value: f64) -> Self;
    fn as_audio(self) -> f64;
}

impl Sample for f32 {
    unsafe fn array(bus: *const AudioBusBuffers) -> *mut *mut Self {
        unsafe { (*bus).__field0.channelBuffers32 }
    }
    fn from_audio(value: f64) -> Self {
        value as f32
    }
    fn as_audio(self) -> f64 {
        f64::from(self)
    }
}

impl Sample for f64 {
    unsafe fn array(bus: *const AudioBusBuffers) -> *mut *mut Self {
        unsafe { (*bus).__field0.channelBuffers64 }
    }
    fn from_audio(value: f64) -> Self {
        value
    }
    fn as_audio(self) -> f64 {
        self
    }
}

struct Channels<T> {
    count: usize,
    pointers: [*mut T; 2],
}

impl<T: Sample> Channels<T> {
    unsafe fn new(bus: *const AudioBusBuffers) -> Option<Self> {
        let count = unsafe { (*bus).numChannels };
        let array = unsafe { T::array(bus) };
        if !matches!(count, 1 | 2) || array.is_null() {
            return None;
        }
        let left = unsafe { *array };
        let right = if count == 2 {
            unsafe { *array.add(1) }
        } else {
            left
        };
        if left.is_null() || right.is_null() {
            return None;
        }
        Some(Self {
            count: count as usize,
            pointers: [left, right],
        })
    }

    unsafe fn read(&self, index: usize, silence: u64) -> [f64; 2] {
        let read = |channel: usize| {
            if silence & (1 << channel) != 0 {
                return 0.0;
            }
            let value = unsafe { (*self.pointers[channel].add(index)).as_audio() };
            if value.is_finite() {
                value
            } else {
                0.0
            }
        };
        let left = read(0);
        [left, if self.count == 2 { read(1) } else { left }]
    }

    unsafe fn write(&self, index: usize, values: [f64; 2]) -> u64 {
        let mut signal_flags = 0;
        for (channel, value) in values.into_iter().take(self.count).enumerate() {
            let sample = T::from_audio(value);
            signal_flags |= u64::from(sample.as_audio() != 0.0) << channel;
            unsafe { *self.pointers[channel].add(index) = sample };
        }
        signal_flags
    }
}

struct Cursor<'a> {
    queue: ComRef<'a, IParamValueQueue>,
    id: usize,
    index: i32,
    length: i32,
    previous: (i32, f64),
    next: Option<(i32, f64)>,
    discrete: bool,
}

impl Cursor<'_> {
    fn advance(&mut self, frames: i32) {
        self.next = None;
        while self.index < self.length {
            let (mut offset, mut value) = (0, 0.0);
            let result = unsafe { self.queue.getPoint(self.index, &mut offset, &mut value) };
            self.index += 1;
            if result == kResultOk && value.is_finite() {
                self.next = Some((
                    offset.clamp(self.previous.0, frames),
                    parameters::spec(self.id).canonical(value),
                ));
                return;
            }
        }
    }
}

struct Queues<'a> {
    active: [Option<Cursor<'a>>; COUNT],
    count: usize,
    frames: i32,
}

impl<'a> Queues<'a> {
    unsafe fn new(data: &'a ProcessData, values: &Values) -> Self {
        let mut queues = Self {
            active: [const { None }; COUNT],
            count: 0,
            frames: data.numSamples,
        };
        let Some(changes) = (unsafe { ComRef::from_raw(data.inputParameterChanges) }) else {
            return queues;
        };
        for index in 0..unsafe { changes.getParameterCount() }.clamp(0, COUNT as i32) {
            let Some(queue) = (unsafe { ComRef::from_raw(changes.getParameterData(index)) }) else {
                continue;
            };
            let id = unsafe { queue.getParameterId() } as usize;
            if id >= COUNT || parameters::configuration_param(id) {
                continue;
            }
            let mut cursor = Cursor {
                queue,
                id,
                index: 0,
                length: unsafe { queue.getPointCount() },
                previous: (0, values[id]),
                next: None,
                discrete: !parameters::spec(id).choices.is_empty(),
            };
            cursor.advance(queues.frames);
            if cursor.next.is_some() {
                queues.active[queues.count] = Some(cursor);
                queues.count += 1;
            }
        }
        queues
    }

    fn apply(&mut self, index: i32, values: &mut Values) -> (bool, bool) {
        let mut changed = false;
        let mut discrete = false;
        for cursor in self.active[..self.count].iter_mut().flatten() {
            while let Some(point) = cursor.next.filter(|point| point.0 <= index) {
                cursor.previous = point;
                cursor.advance(self.frames);
            }
            let (start, base) = cursor.previous;
            let value = match cursor.next {
                Some((end, value)) if !cursor.discrete && index >= start => {
                    base + (value - base) * f64::from(index - start) / f64::from(end - start)
                }
                _ => base,
            };
            if values[cursor.id] != value {
                values[cursor.id] = value;
                changed = true;
                discrete |= cursor.discrete;
            }
        }
        (changed, discrete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::BandParam as P;

    fn bus(buffers: &mut [*mut f64]) -> AudioBusBuffers {
        AudioBusBuffers {
            numChannels: buffers.len() as i32,
            silenceFlags: 0,
            __field0: AudioBusBuffers__type0 {
                channelBuffers64: buffers.as_mut_ptr(),
            },
        }
    }

    #[test]
    fn latency_changes_apply_at_host_activation() {
        let processor = Processor::new();
        assert!(processor.meters.request(parameters::LOOKAHEAD, 1.0));
        assert_eq!(unsafe { processor.getLatencySamples() }, 0);
        assert_eq!(unsafe { processor.setActive(1) }, kResultOk);
        assert_eq!(unsafe { processor.getLatencySamples() }, 960);
        assert_eq!(unsafe { processor.setProcessing(1) }, kResultOk);
        assert_eq!(processor.runtime.lock().unwrap().engine.latency(), 960);
        assert!(processor.meters.request(parameters::LOOKAHEAD, 0.0));
        assert_eq!(unsafe { processor.setActive(0) }, kResultOk);
        assert_eq!(unsafe { processor.setActive(1) }, kResultOk);
        assert_eq!(unsafe { processor.getLatencySamples() }, 0);
    }

    #[test]
    fn mono_f64_sidechain_triggers_dynamics_in_place() {
        const N: usize = 4096;
        const FREQUENCY: f64 = 800.0;
        let processor = Processor::new();
        let mut values = parameters::empty();
        for (param, value) in [
            (P::Enabled, 1.0),
            (P::Low, 100.0),
            (P::High, 7000.0),
            (P::Key, 2.0),
            (P::Threshold, -36.0),
            (P::Attack, 1.0),
            (P::Range, 18.0),
            (P::Ratio, 8.0),
        ] {
            let id = param.id(0);
            values[id] = parameters::spec(id).normalized(value);
        }
        *processor.state.lock().unwrap() = SavedState {
            values,
            pending: true,
        };

        let mut key = [0.0; N];
        for (n, sample) in key.iter_mut().enumerate() {
            *sample = (std::f64::consts::TAU * FREQUENCY / DEFAULT_RATE * n as f64).sin();
        }
        let original = key.map(|sample| sample * 0.2);
        let mut audio = original;
        let mut main_channels = [audio.as_mut_ptr()];
        let mut key_channels = [key.as_mut_ptr()];
        let mut output_channels = main_channels;
        let mut inputs = [bus(&mut main_channels), bus(&mut key_channels)];
        let mut output = bus(&mut output_channels);
        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime as i32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample64 as i32,
            numSamples: N as i32,
            numInputs: 2,
            numOutputs: 1,
            inputs: inputs.as_mut_ptr(),
            outputs: &mut output,
            inputParameterChanges: std::ptr::null_mut(),
            outputParameterChanges: std::ptr::null_mut(),
            inputEvents: std::ptr::null_mut(),
            outputEvents: std::ptr::null_mut(),
            processContext: std::ptr::null_mut(),
        };
        assert_eq!(unsafe { processor.process(&mut data) }, kResultOk);
        assert_eq!(audio, original);
        assert_eq!(
            unsafe {
                processor.activateBus(
                    MediaTypes_::kAudio as i32,
                    BusDirections_::kInput as i32,
                    1,
                    1,
                )
            },
            kResultOk
        );
        assert_eq!(unsafe { processor.process(&mut data) }, kResultOk);
        let peak = audio[N - 256..]
            .iter()
            .fold(0.0_f64, |peak, sample| peak.max(sample.abs()));
        assert!(peak < 0.035, "sidechain peak was {peak}");
    }
}
