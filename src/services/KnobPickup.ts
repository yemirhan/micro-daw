/**
 * Soft takeover ("pickup") for absolute hardware knobs.
 *
 * A knob's physical position and the parameter it controls drift apart
 * (another knob/pad selected, an on-screen edit, undo, a project load). To
 * avoid a jump on first touch, a knob only takes control once its position
 * crosses the parameter's current value, or comes within `threshold` of it.
 * After that it follows the knob, until the parameter changes behind the
 * knob's back (then it has to be picked up again).
 *
 * All values are normalised 0..1.
 */

/** ~6 of 127 CC steps. */
export const PICKUP_THRESHOLD = 0.05;
/** A parameter further than this from what the knob last set was changed elsewhere. */
const EXTERNAL_CHANGE_EPS = 0.02;

export interface PickupState {
  /** Last knob position seen (null before the first message). */
  lastKnob: number | null;
  /** The knob is in control. */
  pickedUp: boolean;
  /** Parameter value (0..1) right after the knob last set it. */
  lastSet: number | null;
}

export const initialPickupState = (): PickupState => ({ lastKnob: null, pickedUp: false, lastSet: null });

/**
 * Pure: should a knob at `knob` move a parameter currently at `current`?
 * Returns the decision and the next state.
 */
export function pickupStep(
  state: PickupState,
  knob: number,
  current: number,
  threshold = PICKUP_THRESHOLD,
): { pass: boolean; state: PickupState } {
  let pickedUp = state.pickedUp;
  // Moved elsewhere (UI, undo, load) since the knob last set it: pick up again.
  if (pickedUp && state.lastSet !== null && Math.abs(current - state.lastSet) > EXTERNAL_CHANGE_EPS) pickedUp = false;
  if (!pickedUp) {
    const close = Math.abs(knob - current) <= threshold;
    const last = state.lastKnob;
    const crossed = last !== null && ((last <= current && knob >= current) || (last >= current && knob <= current));
    pickedUp = close || crossed;
  }
  return { pass: pickedUp, state: { lastKnob: knob, pickedUp, lastSet: pickedUp ? state.lastSet : null } };
}

/** Pickup state per knob key (e.g. a CC number or a target+context id). */
export class KnobPickup {
  private states = new Map<string, PickupState>();

  constructor(private readonly threshold = PICKUP_THRESHOLD) {}

  /** True when the knob `key` at `knob` may set the parameter now at `current`. */
  check(key: string, knob: number, current: number): boolean {
    const { pass, state } = pickupStep(this.states.get(key) ?? initialPickupState(), knob, current, this.threshold);
    this.states.set(key, state);
    return pass;
  }

  /** Record the value (0..1) the knob just set, so later outside edits are detected. */
  applied(key: string, value: number): void {
    const s = this.states.get(key);
    if (s) s.lastSet = value;
  }

  isPickedUp(key: string): boolean {
    return this.states.get(key)?.pickedUp ?? false;
  }

  /** Forget one knob (or all): it has to be picked up again. */
  reset(key?: string): void {
    if (key === undefined) this.states.clear();
    else this.states.delete(key);
  }
}
