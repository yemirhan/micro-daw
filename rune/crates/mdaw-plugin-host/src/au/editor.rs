//! The plugin's editor: its Cocoa view (`kAudioUnitProperty_CocoaUI`) in a window of its own.
//!
//! The view sits in a plain container view (a window forces its content view to fill it; a
//! plugin view sizes itself), and the window follows the view's size (plugins resize their
//! editors: a rack that grows, a zoom setting). Closing the window hides it; the plugin and
//! its view stay alive in the [`EditorWindow`], so reopening is instant and keeps the layout.

use std::ffi::c_void;
use std::ptr::{self, NonNull};

use block2::RcBlock;
use core_foundation::base::CFRelease;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, msg_send, sel};
use objc2_app_kit::{
    NSBackingStoreType, NSView, NSViewFrameDidChangeNotification, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{
    NSBundle, NSNotification, NSNotificationCenter, NSPoint, NSRect, NSSize, NSString, NSURL,
};

use super::Instance;
use super::ffi::*;
use crate::PluginError;

/// Smallest editor window (a view that reports no size yet).
const MIN_SIZE: (f64, f64) = (240.0, 120.0);

fn cocoa_info_size(unit: AudioUnit) -> Option<u32> {
    let mut size = 0u32;
    let mut writable = 0u8;
    // SAFETY: valid out pointers.
    let status = unsafe {
        AudioUnitGetPropertyInfo(
            unit,
            kAudioUnitProperty_CocoaUI,
            kAudioUnitScope_Global,
            0,
            &mut size,
            &mut writable,
        )
    };
    (status == 0 && size as usize >= std::mem::size_of::<AudioUnitCocoaViewInfo>()).then_some(size)
}

pub(super) fn has_cocoa_view(unit: AudioUnit) -> bool {
    cocoa_info_size(unit).is_some()
}

/// The plugin's own view, from the factory class its bundle names.
fn cocoa_view(unit: AudioUnit) -> Option<Retained<NSView>> {
    let size = cocoa_info_size(unit)?;
    // The value is the bundle URL followed by one or more class names (all returned +1).
    let words = (size as usize).div_ceil(std::mem::size_of::<*const c_void>());
    let mut buf: Vec<*const c_void> = vec![ptr::null(); words];
    let mut io = size;
    // SAFETY: `buf` holds `size` bytes.
    let status = unsafe {
        AudioUnitGetProperty(
            unit,
            kAudioUnitProperty_CocoaUI,
            kAudioUnitScope_Global,
            0,
            buf.as_mut_ptr() as *mut c_void,
            &mut io,
        )
    };
    if status != 0 || buf.len() < 2 || buf[0].is_null() || buf[1].is_null() {
        for p in buf.iter().filter(|p| !p.is_null()) {
            // SAFETY: every non-null entry is a +1 CF object.
            unsafe { CFRelease(*p) };
        }
        return None;
    }
    // SAFETY: CFURL and CFString are toll-free bridged; ownership of the +1 moves here.
    let (url, class_name) = unsafe {
        (
            Retained::from_raw(buf[0] as *mut NSURL)?,
            Retained::from_raw(buf[1] as *mut NSString)?,
        )
    };
    for p in buf.iter().skip(2).filter(|p| !p.is_null()) {
        // SAFETY: the unused extra class names, each +1.
        unsafe { CFRelease(*p) };
    }
    let bundle = NSBundle::bundleWithURL(&url)?;
    let class = bundle.classNamed(&class_name)?;
    // SAFETY: the class implements `AUCocoaUIBase` (it is the one the unit named).
    let factory: Option<Retained<AnyObject>> = unsafe { msg_send![class, new] };
    let factory = factory?;
    // `uiViewForAudioUnit:withSize:`. Sent with a hand-written signature: the unit argument's
    // type encoding differs between SDK generations, which the checked `msg_send!` rejects.
    type ViewFn =
        unsafe extern "C-unwind" fn(*const AnyObject, Sel, AudioUnit, NSSize) -> *mut NSView;
    // SAFETY: `objc_msgSend` is called with the method's real signature (object, selector, the
    // unit pointer, an NSSize by value; returns an object pointer), on every macOS ABI.
    let view = unsafe {
        let send: ViewFn =
            std::mem::transmute(objc2::ffi::objc_msgSend as unsafe extern "C-unwind" fn());
        send(
            &*factory,
            sel!(uiViewForAudioUnit:withSize:),
            unit,
            NSSize::new(0.0, 0.0),
        )
    };
    // SAFETY: an autoreleased NSView (or nil).
    unsafe { Retained::retain(view) }
}

/// An open editor. Dropping it closes the window and releases the view.
pub struct EditorWindow {
    window: Retained<NSWindow>,
    view: Retained<NSView>,
    observer: Retained<ProtocolObject<dyn NSObjectProtocol>>,
    /// Keeps the plugin alive as long as its view exists.
    _instance: Instance,
}

fn fit(window: &NSWindow, view: &NSView) {
    let size = view.frame().size;
    let size = NSSize::new(size.width.max(MIN_SIZE.0), size.height.max(MIN_SIZE.1));
    let Some(content) = window.contentView() else {
        return;
    };
    let now = content.frame().size;
    if (now.width - size.width).abs() < 0.5 && (now.height - size.height).abs() < 0.5 {
        return;
    }
    // Keep the title bar where it is: the window grows downwards.
    let frame = window.frame();
    let new_frame = window.frameRectForContentRect(NSRect::new(NSPoint::new(0.0, 0.0), size));
    let top = frame.origin.y + frame.size.height;
    window.setFrame_display(
        NSRect::new(
            NSPoint::new(frame.origin.x, top - new_frame.size.height),
            new_frame.size,
        ),
        true,
    );
    view.setFrameOrigin(NSPoint::new(0.0, 0.0));
}

pub(super) fn open(instance: Instance, title: &str) -> Result<EditorWindow, PluginError> {
    let mtm = MainThreadMarker::new().ok_or(PluginError::NotMainThread)?;
    let view = cocoa_view(instance.unit()).ok_or(PluginError::NoEditor)?;
    let size = view.frame().size;
    let rect = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(size.width.max(MIN_SIZE.0), size.height.max(MIN_SIZE.1)),
    );
    let style =
        NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable;
    // SAFETY: a plain window created on the main thread; it is kept by `EditorWindow`, so
    // AppKit must not release it on close.
    let window = unsafe {
        let w = NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            rect,
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        w.setReleasedWhenClosed(false);
        w
    };
    window.setTitle(&NSString::from_str(title));
    let container = NSView::initWithFrame(NSView::alloc(mtm), rect);
    window.setContentView(Some(&container));
    view.setFrameOrigin(NSPoint::new(0.0, 0.0));
    container.addSubview(&view);
    view.setPostsFrameChangedNotifications(true);
    let (w2, v2) = (window.clone(), view.clone());
    let block = RcBlock::new(move |_: NonNull<NSNotification>| fit(&w2, &v2));
    // SAFETY: the block runs on the posting (main) thread; the token is removed in Drop.
    let observer = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSViewFrameDidChangeNotification),
            Some(&view),
            None,
            &block,
        )
    };
    window.center();
    window.makeKeyAndOrderFront(None);
    Ok(EditorWindow {
        window,
        view,
        observer,
        _instance: instance,
    })
}

impl EditorWindow {
    /// False once the user closed the window (it can be shown again with [`Self::focus`]).
    pub fn is_open(&self) -> bool {
        self.window.isVisible() || self.window.isMiniaturized()
    }

    /// Show the window and bring it to the front.
    pub fn focus(&self) {
        fit(&self.window, &self.view);
        if self.window.isMiniaturized() {
            self.window.deminiaturize(None);
        }
        self.window.makeKeyAndOrderFront(None);
    }

    /// Hide the window (the view stays loaded).
    pub fn close(&self) {
        self.window.orderOut(None);
    }
}

impl Drop for EditorWindow {
    fn drop(&mut self) {
        // SAFETY: the token came from `addObserverForName…` on this centre.
        unsafe {
            NSNotificationCenter::defaultCenter()
                .removeObserver(AsRef::<AnyObject>::as_ref(&*self.observer));
        }
        self.view.removeFromSuperview();
        self.window.close();
    }
}
