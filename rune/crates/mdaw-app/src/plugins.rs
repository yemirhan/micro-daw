//! The app's plugin manager: the installed Audio Units, the loaded instances of the project's
//! plugin slots, their editor windows and their saved state.
//!
//! - **Catalog**: every installed effect / instrument (`mdaw_plugin_host::scan`, read once at
//!   start from the system's registry; nothing is loaded for it).
//! - **Instances**: one loaded plugin per [`PluginSlot`] id, created the first time the engine
//!   asks for its processor ([`PluginFactory`], on the UI thread) and kept while the document
//!   has the slot ([`PluginManager::retain`]) and the project is the same
//!   ([`PluginFactory::begin_session`]: another project unloads everything). The slot's stored
//!   state is applied when the plugin is loaded, and only then.
//! - **State**: a plugin changes under its own editor, without telling the host what changed,
//!   so the loaded plugin is the truth and the document holds a copy for saving.
//!   [`PluginManager::capture_states`] reads every loaded plugin's state and returns the ones
//!   that differ from the document's copy; the app stores them (no undo entry) before a save
//!   or an autosave, while an editor is open, around an undo / redo (an unrelated undo must
//!   not turn a plugin's knobs back: the restored document gets the live state again) and
//!   before a slot is removed (so undoing the removal brings the plugin back as it was). A
//!   plugin whose state reads differently every time (a timestamp inside) would make the
//!   project look edited for ever: it is only captured when a save asks for it.
//! - **Offline** processors (an export) are fresh instances in the live instance's current
//!   state.
//!
//! Headless runs (snapshots, tests) have a manager that never loads anything: slots read as
//! "not installed" unless a catalog is injected for a scene.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mdaw_engine::plugin::{PluginFactory, PluginProcessor, PluginTransport};
use mdaw_model::PluginSlot;
use mdaw_plugin_host::{
    EditorWindow, HostTransport, Instance, PluginId, PluginInfo, PluginKind, Renderer,
};

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// [`base64_encode`]'s inverse; None for anything that is not padded standard base64.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let last = bytes.len() / 4;
    for (i, q) in bytes.chunks(4).enumerate() {
        let pad = q.iter().rev().take_while(|c| **c == b'=').count();
        if pad > 2 || (pad > 0 && i + 1 != last) {
            return None;
        }
        let mut n = 0u32;
        for c in &q[..4 - pad] {
            n = n << 6 | value(*c)?;
        }
        n <<= 6 * pad as u32;
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

/// The plugin a slot names.
pub fn slot_plugin_id(slot: &PluginSlot) -> Option<PluginId> {
    PluginId::from_codes(&slot.kind, &slot.subtype, &slot.manufacturer)
}

/// A document slot for an installed plugin (no id yet: the document gives it one).
pub fn slot_for(info: &PluginInfo) -> PluginSlot {
    let (kind, subtype, manufacturer) = info.id.codes();
    PluginSlot {
        id: String::new(),
        format: "au".into(),
        kind,
        subtype,
        manufacturer,
        name: info.name.clone(),
        vendor: info.vendor.clone(),
        bypassed: false,
        state: None,
        extra: Default::default(),
    }
}

/// The engine's processor around an Audio Unit's render half.
struct AuProcessor {
    renderer: Renderer,
    kind: PluginKind,
}

impl PluginProcessor for AuProcessor {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        match self.kind {
            PluginKind::Effect => self.renderer.process_effect(left, right),
            PluginKind::Instrument => self.renderer.render_instrument(left, right),
        }
    }
    fn midi(&mut self, status: u8, data1: u8, data2: u8, offset: u32) {
        self.renderer.midi(status, data1, data2, offset);
    }
    fn set_transport(&mut self, t: &PluginTransport) {
        self.renderer.set_transport(HostTransport {
            bpm: t.bpm,
            beat: t.beat,
            playing: t.playing,
        });
    }
    fn reset(&mut self) {
        self.renderer.reset();
    }
}

struct Loaded {
    instance: Instance,
    editor: Option<EditorWindow>,
    /// Two reads in a row gave different states (None: not checked yet).
    volatile: Option<bool>,
}

/// See the module docs.
pub struct PluginManager {
    /// False: never load a plugin (headless runs).
    enabled: bool,
    catalog: Vec<PluginInfo>,
    loaded: HashMap<String, Loaded>,
    /// Slot id → why its plugin did not load.
    errors: HashMap<String, String>,
    /// Bumped when something the UI shows changed (a load, an error, an editor).
    version: u64,
    /// The project session the loaded plugins belong to.
    session: Option<u64>,
}

impl PluginManager {
    /// The manager of a window run: scans the installed plugins.
    pub fn new() -> Self {
        Self {
            enabled: true,
            catalog: mdaw_plugin_host::scan(),
            loaded: HashMap::new(),
            errors: HashMap::new(),
            version: 0,
            session: None,
        }
    }

    /// A manager that lists `catalog` but never loads anything (headless runs).
    pub fn headless(catalog: Vec<PluginInfo>) -> Self {
        Self {
            enabled: false,
            catalog,
            loaded: HashMap::new(),
            errors: HashMap::new(),
            version: 0,
            session: None,
        }
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn catalog(&self) -> &[PluginInfo] {
        &self.catalog
    }

    /// The installed plugins of one kind.
    pub fn of_kind(&self, kind: PluginKind) -> Vec<&PluginInfo> {
        self.catalog.iter().filter(|p| p.kind == kind).collect()
    }

    /// Whether the slot's plugin is installed on this computer.
    pub fn is_installed(&self, slot: &PluginSlot) -> bool {
        slot_plugin_id(slot).is_some_and(|id| self.catalog.iter().any(|p| p.id == id))
    }

    /// Why the slot's plugin is not running (None: it is, or it was never asked for).
    pub fn error(&self, slot_id: &str) -> Option<&str> {
        self.errors.get(slot_id).map(String::as_str)
    }

    pub fn is_loaded(&self, slot_id: &str) -> bool {
        self.loaded.contains_key(slot_id)
    }

    fn fail(&mut self, slot: &PluginSlot, error: String) {
        eprintln!("[plugins] {}: {error}", slot.name);
        if self.errors.insert(slot.id.clone(), error.clone()) != Some(error) {
            self.version += 1;
        }
    }

    /// The slot's loaded plugin, loading it (in the slot's stored state) when it is not.
    fn ensure(&mut self, slot: &PluginSlot, sample_rate: u32) -> Option<&mut Loaded> {
        if !self.enabled {
            return None;
        }
        if !self.loaded.contains_key(&slot.id) {
            let Some(id) = slot_plugin_id(slot) else {
                self.fail(slot, "not a plugin id".into());
                return None;
            };
            match Instance::open(id, sample_rate as f64) {
                Ok(instance) => {
                    if let Some(state) = slot.state.as_deref() {
                        let applied = base64_decode(state)
                            .is_some_and(|bytes| instance.set_state(&bytes).is_ok());
                        if !applied {
                            eprintln!(
                                "[plugins] {}: the saved state could not be restored",
                                slot.name
                            );
                        }
                    }
                    self.errors.remove(&slot.id);
                    self.version += 1;
                    self.loaded.insert(
                        slot.id.clone(),
                        Loaded {
                            instance,
                            editor: None,
                            volatile: None,
                        },
                    );
                }
                Err(e) => {
                    self.fail(slot, e.to_string());
                    return None;
                }
            }
        }
        self.loaded.get_mut(&slot.id)
    }

    /// Another project (`session`) than the loaded plugins belong to: unload them all.
    fn begin_session(&mut self, session: u64) {
        if self.session.replace(session) != Some(session) && !self.loaded.is_empty() {
            self.loaded.clear();
            self.errors.clear();
            self.version += 1;
        }
    }

    /// Show the slot's editor window (loading the plugin when needed).
    pub fn open_editor(&mut self, slot: &PluginSlot, sample_rate: u32) -> Result<(), String> {
        if !self.enabled {
            return Err("plugins are not available in this run".into());
        }
        let title = slot.name.clone();
        let Some(l) = self.ensure(slot, sample_rate) else {
            return Err(self
                .errors
                .get(&slot.id)
                .cloned()
                .unwrap_or_else(|| "the plugin is not installed".into()));
        };
        match &l.editor {
            Some(w) => w.focus(),
            None => l.editor = Some(l.instance.open_editor(&title).map_err(|e| e.to_string())?),
        }
        self.version += 1;
        Ok(())
    }

    /// Whether the slot's editor window is showing.
    pub fn editor_open(&self, slot_id: &str) -> bool {
        self.loaded
            .get(slot_id)
            .and_then(|l| l.editor.as_ref())
            .is_some_and(EditorWindow::is_open)
    }

    /// Whether any editor window is showing (the app then watches for state changes).
    pub fn any_editor_open(&self) -> bool {
        self.loaded
            .values()
            .any(|l| l.editor.as_ref().is_some_and(EditorWindow::is_open))
    }

    /// The states of the loaded plugins that differ from what the document holds
    /// (`doc_state(slot id)`): (slot id, state) to store there. A plugin whose state never
    /// reads the same twice is skipped unless `force` (an explicit save).
    pub fn capture_states(
        &mut self,
        force: bool,
        doc_state: impl Fn(&str) -> Option<Option<String>>,
    ) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (id, l) in &mut self.loaded {
            let Some(in_doc) = doc_state(id) else {
                continue; // not in the document (any more)
            };
            let Some(state) = l.instance.state() else {
                continue;
            };
            let volatile = *l
                .volatile
                .get_or_insert_with(|| l.instance.state().as_deref() != Some(state.as_slice()));
            if volatile && !force {
                continue;
            }
            let state = base64_encode(&state);
            if in_doc.as_deref() != Some(state.as_str()) {
                out.push((id.clone(), state));
            }
        }
        out
    }

    /// Unload every plugin whose slot is not in `ids` (its editor closes).
    pub fn retain(&mut self, ids: &[&str]) {
        let before = self.loaded.len();
        self.loaded.retain(|id, _| ids.contains(&id.as_str()));
        self.errors.retain(|id, _| ids.contains(&id.as_str()));
        if self.loaded.len() != before {
            self.version += 1;
        }
    }

    /// Close every editor window (the app quits, another project opens).
    pub fn close_editors(&mut self) {
        for l in self.loaded.values_mut() {
            l.editor = None;
        }
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

/// The manager shared between the app (editors, state) and the engine (processors).
#[derive(Clone)]
pub struct Plugins(pub Rc<RefCell<PluginManager>>);

impl Plugins {
    pub fn new(manager: PluginManager) -> Self {
        Self(Rc::new(RefCell::new(manager)))
    }
}

impl PluginFactory for Plugins {
    fn begin_session(&mut self, session: u64) {
        self.0.borrow_mut().begin_session(session);
    }

    fn processor(
        &mut self,
        slot: &PluginSlot,
        sample_rate: u32,
    ) -> Option<Box<dyn PluginProcessor>> {
        let mut m = self.0.borrow_mut();
        let l = m.ensure(slot, sample_rate)?;
        Some(Box::new(AuProcessor {
            renderer: l.instance.renderer(),
            kind: l.instance.kind(),
        }))
    }

    fn offline_processor(
        &mut self,
        slot: &PluginSlot,
        sample_rate: u32,
    ) -> Option<Box<dyn PluginProcessor>> {
        let m = self.0.borrow();
        if !m.enabled {
            return None;
        }
        // The live instance's state right now (what the user hears), else the saved one.
        let state = m
            .loaded
            .get(&slot.id)
            .and_then(|l| l.instance.state())
            .or_else(|| slot.state.as_deref().and_then(base64_decode));
        let instance = Instance::open(slot_plugin_id(slot)?, sample_rate as f64).ok()?;
        instance.set_offline(true);
        if let Some(bytes) = state {
            let _ = instance.set_state(&bytes);
        }
        Some(Box::new(AuProcessor {
            renderer: instance.renderer(),
            kind: instance.kind(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_every_length() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        let bytes: Vec<u8> = (0..=255).collect();
        for n in 0..bytes.len() {
            let enc = base64_encode(&bytes[..n]);
            assert_eq!(base64_decode(&enc).as_deref(), Some(&bytes[..n]), "{n}");
        }
        assert_eq!(base64_decode("Zg="), None);
        assert_eq!(base64_decode("Z==="), None);
        assert_eq!(base64_decode("Zg==Zg=="), None, "padding only at the end");
        assert_eq!(base64_decode("Zm9$"), None);
    }

    #[test]
    fn a_headless_manager_lists_its_catalog_and_loads_nothing() {
        let info = PluginInfo {
            id: PluginId::from_codes("aufx", "Ni$R", "-NI-").unwrap(),
            kind: PluginKind::Effect,
            name: "Guitar Rig 6 FX".into(),
            vendor: "Native Instruments".into(),
            version: 1,
        };
        let mut slot = slot_for(&info);
        slot.id = "s1".into();
        assert_eq!(
            (slot.kind.as_str(), slot.subtype.as_str()),
            ("aufx", "Ni$R")
        );
        let mut plugins = Plugins::new(PluginManager::headless(vec![info]));
        assert!(plugins.0.borrow().is_installed(&slot));
        assert_eq!(plugins.0.borrow().of_kind(PluginKind::Instrument).len(), 0);
        assert!(plugins.processor(&slot, 48_000).is_none());
        assert!(plugins.offline_processor(&slot, 48_000).is_none());
        assert!(plugins.0.borrow_mut().open_editor(&slot, 48_000).is_err());
        assert!(!plugins.0.borrow().editor_open("s1"));
    }
}
