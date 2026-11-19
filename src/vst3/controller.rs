use super::{
    comparison::{self, Comparison},
    editor, load,
    meters::Meters,
    view::{EditorView, Presentation},
    Parameters, STATE_MAGIC,
};
use crate::parameters::{self, COUNT};
use std::{cell::RefCell, ffi::CStr, ptr, rc::Rc, sync::Arc};
use toybox::vst3::prelude::{Steinberg::*, *};

pub(crate) type Handler = Rc<RefCell<Option<ComPtr<IComponentHandler>>>>;

pub(crate) fn restart_for_latency(handler: &Handler) {
    let handler = handler.borrow().clone();
    if let Some(handler) = handler {
        unsafe { handler.restartComponent(RestartFlags_::kLatencyChanged) };
    }
}

pub(crate) fn mark_dirty(handler: &Handler) {
    let handler = handler.borrow().clone();
    if let Some(extended) = handler.and_then(|handler| handler.cast::<IComponentHandler2>()) {
        unsafe { extended.setDirty(1) };
    }
}

pub(crate) fn with_group_edit(handler: &Handler, grouped: bool, edit: impl FnOnce()) {
    let extended = if grouped {
        let handler = handler.borrow().clone();
        handler
            .and_then(|handler| handler.cast::<IComponentHandler2>())
            .filter(|handler| unsafe { handler.startGroupEdit() == kResultOk })
    } else {
        None
    };
    edit();
    if let Some(extended) = extended {
        unsafe { extended.finishGroupEdit() };
    }
}

pub(crate) struct Controller {
    params: Arc<Parameters>,
    handler: Handler,
    connection: InstanceConnection<Meters>,
    comparison: Rc<RefCell<Comparison>>,
}
impl Controller {
    pub(crate) const CID: TUID = uid(0xE420378A, 0x52BA4E2E, 0xB42D45EF, 0xC8798409);
    pub(crate) fn new() -> Self {
        Self {
            params: Arc::new(Parameters::new()),
            handler: Rc::new(RefCell::new(None)),
            comparison: Rc::new(RefCell::new(Comparison::default())),
            connection: InstanceConnection::new(
                InstanceConnectionRole::Controller,
                Arc::new(Meters::new()),
            ),
        }
    }
}
impl Class for Controller {
    type Interfaces = (IEditController, IConnectionPoint, IToyboxSharedState);
}
toybox::impl_vst3_instance_connection!(Controller, connection);

impl IPluginBaseTrait for Controller {
    unsafe fn initialize(&self, _: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}
impl IEditControllerTrait for Controller {
    unsafe fn setComponentState(&self, stream: *mut IBStream) -> tresult {
        let Some(values) = (unsafe { load(stream) }) else {
            return kInvalidArgument;
        };
        self.params.replace(&values);
        kResultOk
    }
    unsafe fn setState(&self, stream: *mut IBStream) -> tresult {
        let Ok(data) = (unsafe {
            read_versioned_payload(
                stream,
                STATE_MAGIC,
                &[1, 2, 3, 4, 5, 6, 7, comparison::VERSION],
            )
        }) else {
            return kInvalidArgument;
        };
        if data.version == comparison::VERSION {
            let Some(comparison) = Comparison::decode(&data.payload) else {
                return kInvalidArgument;
            };
            self.comparison.replace(comparison);
        } else {
            let Some(values) = parameters::decode(&data.payload, data.version) else {
                return kInvalidArgument;
            };
            self.params.replace(&values);
            self.comparison.replace(Comparison::default());
        }
        kResultOk
    }
    unsafe fn getState(&self, stream: *mut IBStream) -> tresult {
        let payload = self.comparison.borrow().encode();
        match unsafe { write_versioned_payload(stream, STATE_MAGIC, comparison::VERSION, &payload) }
        {
            Ok(()) => kResultOk,
            Err(_) => kResultFalse,
        }
    }
    unsafe fn getParameterCount(&self) -> int32 {
        COUNT as i32
    }
    unsafe fn getParameterInfo(&self, index: int32, info: *mut ParameterInfo) -> tresult {
        if info.is_null() || index < 0 || index as usize >= COUNT {
            return kInvalidArgument;
        }
        let id = index as usize;
        let info = unsafe { &mut *info };
        info.id = id as u32;
        let spec = parameters::spec(id);
        copy_wstring(&parameters::host_name(id), &mut info.title);
        copy_wstring(spec.name, &mut info.shortTitle);
        copy_wstring(spec.unit, &mut info.units);
        info.stepCount = spec.choices.len().saturating_sub(1) as i32;
        info.defaultNormalizedValue = spec.default_normalized();
        info.unitId = 0;
        info.flags = if parameters::configuration_param(id) {
            0
        } else {
            ParameterInfo_::ParameterFlags_::kCanAutomate
        };
        if !spec.choices.is_empty() {
            info.flags |= ParameterInfo_::ParameterFlags_::kIsList;
        }
        if parameters::presence_param(id) {
            info.flags = ParameterInfo_::ParameterFlags_::kIsHidden
                | ParameterInfo_::ParameterFlags_::kIsList;
        }
        if id == parameters::BYPASS {
            info.flags |= ParameterInfo_::ParameterFlags_::kIsBypass;
        }
        kResultOk
    }
    unsafe fn getParamStringByValue(
        &self,
        id: ParamID,
        value: ParamValue,
        out: *mut String128,
    ) -> tresult {
        if out.is_null() || id as usize >= COUNT || !value.is_finite() {
            return kInvalidArgument;
        }
        let spec = parameters::spec(id as usize);
        let formatted = if spec.choices.is_empty() {
            format!("{:.6}", spec.plain(value))
        } else {
            spec.display(value)
        };
        copy_wstring(&formatted, unsafe { &mut *out });
        kResultOk
    }
    unsafe fn getParamValueByString(
        &self,
        id: ParamID,
        string: *mut TChar,
        out: *mut ParamValue,
    ) -> tresult {
        if out.is_null() || id as usize >= COUNT {
            return kInvalidArgument;
        }
        if string.is_null() {
            return kInvalidArgument;
        }
        let spec = parameters::spec(id as usize);
        let value = if spec.choices.is_empty() {
            match unsafe { parse_tchar_f64(string) } {
                Some(value) if value.is_finite() => value,
                _ => return kInvalidArgument,
            }
        } else {
            // VST3 supplies a NUL-terminated UTF-16 string.
            let text = String::from_utf16_lossy(unsafe {
                std::slice::from_raw_parts(string, tchar_len(string))
            });
            let Some(index) = spec.choices.iter().position(|choice| *choice == text) else {
                return kInvalidArgument;
            };
            index as f64
        };
        unsafe { *out = spec.normalized(value) };
        kResultOk
    }
    unsafe fn normalizedParamToPlain(&self, id: ParamID, value: ParamValue) -> ParamValue {
        if (id as usize) < COUNT {
            parameters::spec(id as usize).plain(value)
        } else {
            value
        }
    }
    unsafe fn plainParamToNormalized(&self, id: ParamID, value: ParamValue) -> ParamValue {
        if (id as usize) < COUNT {
            parameters::spec(id as usize).normalized(value)
        } else {
            value
        }
    }
    unsafe fn getParamNormalized(&self, id: ParamID) -> ParamValue {
        if (id as usize) < COUNT {
            self.params.get(id as usize)
        } else {
            0.0
        }
    }
    unsafe fn setParamNormalized(&self, id: ParamID, value: ParamValue) -> tresult {
        if id as usize >= COUNT || !value.is_finite() {
            return kInvalidArgument;
        }
        let id = id as usize;
        self.params.set(id, value);
        if parameters::configuration_param(id) && self.connection.shared().request(id, value) {
            restart_for_latency(&self.handler);
        }
        kResultOk
    }
    unsafe fn setComponentHandler(&self, handler: *mut IComponentHandler) -> tresult {
        let mut slot = self.handler.borrow_mut();
        if handler.is_null() {
            *slot = None;
            return kResultOk;
        }
        unsafe { ((*(*handler).vtbl).base.addRef)(handler.cast()) };
        *slot = unsafe { ComPtr::from_raw(handler) };
        kResultOk
    }
    unsafe fn createView(&self, name: FIDString) -> *mut IPlugView {
        if name.is_null()
            || unsafe { CStr::from_ptr(name) } != unsafe { CStr::from_ptr(ViewType::kEditor) }
        {
            return ptr::null_mut();
        }
        let presentation = Rc::new(Presentation::default());
        let gui = editor::new(
            Arc::clone(&self.params),
            Rc::clone(&self.handler),
            self.connection.shared(),
            Rc::clone(&self.comparison),
            Rc::clone(&presentation),
        );
        EditorView::create(gui, presentation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::{MIX, PHASE};
    use comparison::Slot;
    use std::{
        ffi::c_void,
        io::{Cursor, Read, Write},
    };

    #[derive(Default)]
    struct Stream(RefCell<Cursor<Vec<u8>>>);

    impl Class for Stream {
        type Interfaces = (IBStream,);
    }

    impl IBStreamTrait for Stream {
        unsafe fn read(&self, buffer: *mut c_void, size: i32, count: *mut i32) -> tresult {
            let buffer =
                unsafe { std::slice::from_raw_parts_mut(buffer.cast::<u8>(), size as usize) };
            match self.0.borrow_mut().read(buffer) {
                Ok(n) => {
                    unsafe {
                        *count = n as i32;
                    }
                    kResultOk
                }
                Err(_) => kResultFalse,
            }
        }

        unsafe fn write(&self, buffer: *mut c_void, size: i32, count: *mut i32) -> tresult {
            let buffer = unsafe { std::slice::from_raw_parts(buffer.cast::<u8>(), size as usize) };
            match self.0.borrow_mut().write(buffer) {
                Ok(n) => {
                    unsafe {
                        *count = n as i32;
                    }
                    kResultOk
                }
                Err(_) => kResultFalse,
            }
        }

        unsafe fn seek(&self, position: i64, _: i32, result: *mut i64) -> tresult {
            self.0.borrow_mut().set_position(position as u64);
            if !result.is_null() {
                unsafe {
                    *result = position;
                }
            }
            kResultOk
        }

        unsafe fn tell(&self, _: *mut i64) -> tresult {
            kNotImplemented
        }
    }

    #[test]
    fn project_recall_keeps_the_processor_values_and_restores_the_alternate() {
        let a = parameters::defaults();
        let mut b = a;
        b[MIX] = 0.3;
        b[PHASE] = 1.0;
        let original = Controller::new();
        original.comparison.borrow_mut().switch_to(Slot::B, a);
        original.params.replace(&b);
        let editor_state = ComWrapper::new(Stream::default())
            .to_com_ptr::<IBStream>()
            .unwrap();
        let component_state = ComWrapper::new(Stream::default())
            .to_com_ptr::<IBStream>()
            .unwrap();
        let restored = Controller::new();
        unsafe {
            assert_eq!(original.getState(editor_state.as_ptr()), kResultOk);
            assert_eq!(super::super::save(component_state.as_ptr(), &b), kResultOk);
            assert_eq!(component_state.seek(0, 0, ptr::null_mut()), kResultOk);
            assert_eq!(editor_state.seek(0, 0, ptr::null_mut()), kResultOk);
            assert_eq!(
                restored.setComponentState(component_state.as_ptr()),
                kResultOk
            );
            assert_eq!(restored.setState(editor_state.as_ptr()), kResultOk);
        }
        assert_eq!(restored.params.values(), b);
        assert_eq!(restored.comparison.borrow().active(), Slot::B);
        assert_eq!(
            restored.comparison.borrow_mut().switch_to(Slot::A, b),
            Some(a)
        );
    }
}
