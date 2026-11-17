import { describe, expect, it } from 'vitest';
import { KnobPickup, PICKUP_THRESHOLD, initialPickupState, pickupStep } from './KnobPickup';

describe('pickupStep', () => {
  it('passes on the first message when the knob is already near the value', () => {
    expect(pickupStep(initialPickupState(), 0.52, 0.5).pass).toBe(true);
    expect(pickupStep(initialPickupState(), 0.5 + PICKUP_THRESHOLD * 2, 0.5).pass).toBe(false);
  });

  it('picks up when the knob crosses the value (either direction) and then follows it', () => {
    let s = pickupStep(initialPickupState(), 0.9, 0.5).state;
    let r = pickupStep(s, 0.7, 0.5);
    expect(r.pass).toBe(false);
    r = pickupStep(r.state, 0.3, 0.5); // jumped across 0.5
    expect(r.pass).toBe(true);
    s = r.state;
    expect(pickupStep(s, 0.1, 0.3).pass).toBe(true); // follows, even far from the value
    const up = pickupStep(pickupStep(initialPickupState(), 0.1, 0.5).state, 0.6, 0.5);
    expect(up.pass).toBe(true);
  });

  it('re-arms when the value was changed elsewhere after the knob set it', () => {
    const p = new KnobPickup();
    expect(p.check('k', 0.5, 0.5)).toBe(true);
    p.applied('k', 0.6);
    expect(p.check('k', 0.62, 0.6)).toBe(true);
    p.applied('k', 0.62);
    // UI/undo moved it to 0.1; the knob at 0.63 must not jump it back.
    expect(p.check('k', 0.63, 0.1)).toBe(false);
    expect(p.check('k', 0.08, 0.1)).toBe(true);
  });

  it('reset forgets pickups', () => {
    const p = new KnobPickup();
    p.check('a', 0.5, 0.5);
    expect(p.isPickedUp('a')).toBe(true);
    p.reset();
    expect(p.isPickedUp('a')).toBe(false);
    expect(p.check('a', 0.9, 0.5)).toBe(false);
  });
});
