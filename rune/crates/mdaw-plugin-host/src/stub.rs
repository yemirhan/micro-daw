//! The host on platforms without a plugin format: nothing is installed, nothing opens.

use crate::{HostTransport, PluginError, PluginId, PluginInfo, PluginKind};

pub fn scan() -> Vec<PluginInfo> {
    Vec::new()
}

#[derive(Clone)]
pub struct Instance(());

impl Instance {
    pub fn open(_id: PluginId, _sample_rate: f64) -> Result<Self, PluginError> {
        Err(PluginError::Unsupported)
    }
    pub fn info(&self) -> &PluginInfo {
        unreachable!()
    }
    pub fn kind(&self) -> PluginKind {
        unreachable!()
    }
    pub fn state(&self) -> Option<Vec<u8>> {
        None
    }
    pub fn set_state(&self, _state: &[u8]) -> Result<(), PluginError> {
        Err(PluginError::Unsupported)
    }
    pub fn latency_seconds(&self) -> f64 {
        0.0
    }
    pub fn set_offline(&self, _offline: bool) {}
    pub fn renderer(&self) -> Renderer {
        Renderer(())
    }
    pub fn has_editor(&self) -> bool {
        false
    }
    pub fn open_editor(&self, _title: &str) -> Result<EditorWindow, PluginError> {
        Err(PluginError::Unsupported)
    }
}

pub struct Renderer(());

impl Renderer {
    pub fn set_transport(&mut self, _t: HostTransport) {}
    pub fn process_effect(&mut self, _l: &mut [f32], _r: &mut [f32]) {}
    pub fn render_instrument(&mut self, _l: &mut [f32], _r: &mut [f32]) {}
    pub fn midi(&mut self, _status: u8, _data1: u8, _data2: u8, _offset: u32) {}
    pub fn reset(&mut self) {}
}

pub struct EditorWindow(());

impl EditorWindow {
    pub fn is_open(&self) -> bool {
        false
    }
    pub fn focus(&self) {}
    pub fn close(&self) {}
}
