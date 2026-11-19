//! Keyboard helpers: `src/components/beat/stepKeys.ts` (roving-cell keyboard model of the step
//! grid), `keyScope.ts` and `isTypingTarget` (beatHelpers.ts). The DOM queries are ported over a
//! small element model: a chain of [`DomNode`]s from the event target up to the root.

use super::{KeyEventLike, StepCell};
use crate::js::{clamp as js_clamp, round as js_round, round_string as js_round_string};
use crate::types::PAD_COUNT;

// ---------------------------------------------------------------------------
// Typing targets / key scopes (DOM-free)
// ---------------------------------------------------------------------------

/// One element of a key event's target chain (only what the selectors look at).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DomNode {
    /// Tag name (matched case-insensitively, like HTML type selectors).
    pub tag: String,
    /// `role` attribute.
    pub role: Option<String>,
    /// `contenteditable` attribute.
    pub contenteditable: Option<String>,
    /// `data-keys-scope` attribute.
    pub keys_scope: Option<String>,
}

impl DomNode {
    pub fn new(tag: &str) -> Self {
        Self {
            tag: tag.to_string(),
            ..Self::default()
        }
    }

    pub fn with_role(mut self, role: &str) -> Self {
        self.role = Some(role.to_string());
        self
    }

    pub fn with_contenteditable(mut self, value: &str) -> Self {
        self.contenteditable = Some(value.to_string());
        self
    }

    pub fn with_keys_scope(mut self, scope: &str) -> Self {
        self.keys_scope = Some(scope.to_string());
        self
    }

    fn is_tag(&self, tag: &str) -> bool {
        self.tag.eq_ignore_ascii_case(tag)
    }

    fn role_is(&self, roles: &[&str]) -> bool {
        self.role.as_deref().is_some_and(|r| roles.contains(&r))
    }

    /// `input, textarea, select, [contenteditable=""], [contenteditable="true"], [role="combobox"],
    /// [role="textbox"]`.
    fn matches_typing_selector(&self) -> bool {
        self.is_tag("input")
            || self.is_tag("textarea")
            || self.is_tag("select")
            || matches!(self.contenteditable.as_deref(), Some("" | "true"))
            || self.role_is(&["combobox", "textbox"])
    }

    /// `[data-keys-scope], [role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"]`.
    fn matches_owned_selector(&self) -> bool {
        self.keys_scope.is_some() || self.role_is(&["dialog", "alertdialog", "menu", "listbox"])
    }
}

/// `HTMLElement.isContentEditable`: the nearest `contenteditable` with a valid keyword decides
/// (`true` / `""` / `plaintext-only` → editable, `false` → not; keywords are case-insensitive).
fn is_content_editable(chain: &[DomNode]) -> bool {
    for node in chain {
        match node
            .contenteditable
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("" | "true" | "plaintext-only") => return true,
            Some("false") => return false,
            _ => {}
        }
    }
    false
}

/// `isTypingTarget(target)`: true while the user is typing (inputs, selects, contenteditable) —
/// pad keys must not fire. `chain` = the target element then its ancestors; `None` / empty = no
/// element target (null, window).
pub fn is_typing_target(chain: Option<&[DomNode]>) -> bool {
    let Some(chain) = chain.filter(|c| !c.is_empty()) else {
        return false;
    };
    if is_content_editable(chain) {
        return true;
    }
    chain.iter().any(DomNode::matches_typing_selector)
}

/// `KEYS_SCOPE_ATTR`: a subtree marked with it owns its own single-key bindings.
pub const KEYS_SCOPE_ATTR: &str = "data-keys-scope";
/// `BROWSER_KEYS_SCOPE`: the sample browser's scope name.
pub const BROWSER_KEYS_SCOPE: &str = "browser";

/// `isScopedKeyTarget(target)`: the key belongs to whatever has focus (text fields, a keys scope,
/// dialogs, menus, listboxes) rather than to the beat view's global performance keys.
pub fn is_scoped_key_target(chain: Option<&[DomNode]>) -> bool {
    if is_typing_target(chain) {
        return true;
    }
    let Some(chain) = chain.filter(|c| !c.is_empty()) else {
        return false;
    };
    chain.iter().any(DomNode::matches_owned_selector)
}

// ---------------------------------------------------------------------------
// Step grid keyboard model (stepKeys.ts)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StepKeyAction {
    Move(StepCell),
    Toggle,
    Velocity { delta: f64 },
}

/// `VELOCITY_KEY_STEP`: velocity change per +/- (or ] / [) press.
pub const VELOCITY_KEY_STEP: f64 = 0.1;

/// `stepKeyAction(e, cell, steps, stepsPerBeat)`: what a key does on the focused step cell, or
/// None when the key isn't ours. ArrowUp = next pad up; Shift+←/→ jumps to the previous/next beat
/// line; Home/End go to the row ends (Ctrl/⌘: also the top/bottom pad).
pub fn step_key_action(
    e: &KeyEventLike,
    cell: StepCell,
    steps: usize,
    steps_per_beat: usize,
) -> Option<StepKeyAction> {
    let last = steps.saturating_sub(1) as i64;
    let per_beat = steps_per_beat.max(1) as i64;
    let mv = |pad: i64, step: i64| {
        StepKeyAction::Move(StepCell {
            pad: pad.clamp(0, PAD_COUNT as i64 - 1) as usize,
            step: step.clamp(0, last) as usize,
        })
    };
    let pad = cell.pad as i64;
    let step = cell.step as i64;
    let m = e.meta || e.ctrl;
    match e.key.as_str() {
        "ArrowRight" => {
            if e.alt || m {
                return None;
            }
            Some(mv(
                pad,
                if e.shift {
                    (step.div_euclid(per_beat) + 1) * per_beat
                } else {
                    step + 1
                },
            ))
        }
        "ArrowLeft" => {
            if e.alt || m {
                return None;
            }
            // Math.ceil(step / perBeat) for a non-negative step.
            let ceil = (step + per_beat - 1).div_euclid(per_beat);
            Some(mv(
                pad,
                if e.shift {
                    (ceil - 1) * per_beat
                } else {
                    step - 1
                },
            ))
        }
        "ArrowUp" => {
            if e.alt || m {
                return None;
            }
            Some(mv(pad + if e.shift { 4 } else { 1 }, step))
        }
        "ArrowDown" => {
            if e.alt || m {
                return None;
            }
            Some(mv(pad - if e.shift { 4 } else { 1 }, step))
        }
        "Home" => Some(mv(if m { PAD_COUNT as i64 - 1 } else { pad }, 0)),
        "End" => Some(mv(if m { 0 } else { pad }, last)),
        " " | "Enter" => {
            if e.alt || m || e.shift {
                return None;
            }
            Some(StepKeyAction::Toggle)
        }
        "+" | "=" | "]" => {
            if e.alt || m {
                return None;
            }
            Some(StepKeyAction::Velocity {
                delta: VELOCITY_KEY_STEP,
            })
        }
        "-" | "_" | "[" => {
            if e.alt || m {
                return None;
            }
            Some(StepKeyAction::Velocity {
                delta: -VELOCITY_KEY_STEP,
            })
        }
        _ => None,
    }
}

/// `nudgeVelocity(v, delta)`: 5%..100%, rounded to whole percent.
pub fn nudge_velocity(v: f64, delta: f64) -> f64 {
    js_round(js_clamp(v + delta, 0.05, 1.0) * 100.0) / 100.0
}

/// `describeStep(padName, step, velocity)`: "Kick, step 5, on, velocity 80%" / "Snare, step 2, off".
pub fn describe_step(pad_name: &str, step: usize, velocity: f64) -> String {
    let base = format!("{pad_name}, step {}", step + 1);
    if velocity > 0.0 {
        format!(
            "{base}, on, velocity {}%",
            js_round_string(velocity * 100.0)
        )
    } else {
        format!("{base}, off")
    }
}
