//! Port of `src/main/packs/sender.ts`: only the app's own renderer (the main window's top frame)
//! may call `packs:*`. Pure (URL strings only).

use url::Url;

/// Whether `sender_url` is the app's renderer at `renderer_url`. http(s): same origin;
/// file: the same file (query / hash ignored). Anything else is rejected.
pub fn is_trusted_renderer_url(sender_url: Option<&str>, renderer_url: &str) -> bool {
    let Some(sender_url) = sender_url.filter(|s| !s.is_empty()) else {
        return false;
    };
    let (Ok(sender), Ok(renderer)) = (Url::parse(sender_url), Url::parse(renderer_url)) else {
        return false;
    };
    if sender.scheme() != renderer.scheme() {
        return false;
    }
    match renderer.scheme() {
        "http" | "https" => sender.origin() == renderer.origin(),
        "file" => host_of(&sender) == host_of(&renderer) && sender.path() == renderer.path(),
        _ => false,
    }
}

/// WHATWG `url.host` (host plus non-default port).
fn host_of(u: &Url) -> String {
    match (u.host_str(), u.port()) {
        (Some(h), Some(p)) => format!("{h}:{p}"),
        (Some(h), None) => h.to_owned(),
        _ => String::new(),
    }
}

/// The frame that sent an IPC message (`event.senderFrame`): its URL and whether it has a parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SenderFrame {
    pub url: String,
    pub has_parent: bool,
}

/// `assertTrustedSender`: `Err("Untrusted IPC sender")` unless the event comes from the
/// renderer's top frame at `renderer_url`.
pub fn assert_trusted_sender(
    frame: Option<&SenderFrame>,
    renderer_url: &str,
) -> Result<(), &'static str> {
    match frame {
        Some(f) if !f.has_parent && is_trusted_renderer_url(Some(&f.url), renderer_url) => Ok(()),
        _ => Err("Untrusted IPC sender"),
    }
}
