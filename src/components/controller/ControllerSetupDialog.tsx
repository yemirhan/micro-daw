import { useEffect, useMemo, type ReactNode } from 'react';
import { CornerDownLeft, Delete, Plus, RotateCcw, SkipForward, Trash2, Wand2, X } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { cn } from '@/lib/utils';
import { useControllerMapping, useMidiActivity } from '@/hooks/useControllerMapping';
import {
  controllerMapping,
  KNOB_TARGET_OPTIONS,
  knobTargetFromKey,
  knobTargetKey,
  knobTargetLabel,
  sameKnobTarget,
  type LearnState,
  type MidiActivity,
} from '@/services/ControllerMapping';
import { midiService } from '@/services/MidiService';
import { midiToNoteName } from '@/utils/noteHelpers';
import { PAD_COUNT, PADS_PER_BANK, type ControllerProfile, type PadIndex } from '@/types/sampler';
import { padLabel } from '@/components/beat/beatHelpers';

interface ControllerSetupDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const ANY = 'any';
const CHANNELS = Array.from({ length: 16 }, (_, i) => i + 1);

const noteLabel = (note: number) => `${midiToNoteName(note)} · ${note}`;

/**
 * Hardware mapping for the pads and knobs of the connected controller.
 * The "Learn pads" wizard is the main path: hit each hardware pad in order
 * (Backspace = back, Esc = cancel), so any controller layout maps in seconds.
 */
export function ControllerSetupDialog({ open, onOpenChange }: ControllerSetupDialogProps) {
  const { profile, learn } = useControllerMapping();
  const activity = useMidiActivity(open);

  // Closing the dialog ends any learn in progress.
  useEffect(() => {
    if (!open) controllerMapping.cancelLearn();
  }, [open]);

  // Wizard keys: Backspace = previous pad (Esc is handled by the dialog below).
  useEffect(() => {
    if (!open || learn.kind !== 'pad' || !learn.sequence) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Backspace') {
        e.preventDefault();
        controllerMapping.previousPad();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, learn]);

  const deviceName = useMemo(() => {
    if (!open) return null;
    const id = midiService.getActiveDeviceId();
    return midiService.getDevices().find((d) => d.id === id)?.name ?? null;
  }, [open]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="gap-4 sm:max-w-[640px]"
        onEscapeKeyDown={(e) => {
          // Esc cancels a learn first; a second Esc closes the dialog.
          if (controllerMapping.isLearning()) {
            e.preventDefault();
            controllerMapping.cancelLearn();
          }
        }}
      >
        <DialogHeader className="pr-8">
          <DialogTitle>Controller setup</DialogTitle>
          <DialogDescription>
            Map your controller&apos;s pads and knobs. {deviceName ? <>Listening to <span className="text-foreground">{deviceName}</span>.</> : 'No MIDI input selected.'}
          </DialogDescription>
        </DialogHeader>

        <ActivityIndicator activity={activity} />

        {learn.kind !== 'idle' && <LearnBanner learn={learn} />}

        <section aria-labelledby="pads-heading" className="flex flex-col gap-2">
          <div className="flex items-center justify-between gap-2">
            <h3 id="pads-heading" className="text-[12px] font-semibold tracking-wide text-muted-foreground uppercase">
              Pads
            </h3>
            <div className="flex items-center gap-2">
              <PadChannelSelect channel={profile.padMap.channel} />
              <Button
                size="sm"
                variant={learn.kind === 'pad' && learn.sequence ? 'secondary' : 'default'}
                onClick={() => controllerMapping.startPadLearn('sequence')}
              >
                <Wand2 />
                Learn pads (hit 1–16)
              </Button>
            </div>
          </div>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <PadBank bank={0} profile={profile} learn={learn} activity={activity} />
            <PadBank bank={1} profile={profile} learn={learn} activity={activity} />
          </div>
          <p className="text-[11px] text-muted-foreground">
            Click a pad to learn just that one. A note already used by another pad moves to the new pad.
          </p>
        </section>

        <KnobTable profile={profile} learn={learn} activity={activity} />

        <DialogFooter className="sm:justify-between">
          <Button variant="ghost" size="sm" onClick={() => controllerMapping.resetToDefault()}>
            <RotateCcw />
            Reset to default
          </Button>
          <Button size="sm" onClick={() => onOpenChange(false)}>
            Done
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// --- Activity -------------------------------------------------------------------

function describeActivity(a: MidiActivity): { primary: string; secondary: string } {
  if (a.kind === 'note') {
    return {
      primary: `Note ${noteLabel(a.note)}`,
      secondary: `ch ${a.channel} · vel ${Math.round(a.velocity * 127)} → ${a.pad !== null ? `Pad ${a.pad + 1}` : 'keys'}`,
    };
  }
  return {
    primary: `CC ${a.cc} = ${a.value}`,
    secondary: `ch ${a.channel} → ${a.targets.length ? a.targets.map(knobTargetLabel).join(', ') : 'unmapped'}`,
  };
}

function ActivityIndicator({ activity }: { activity: MidiActivity | null }) {
  const text = activity ? describeActivity(activity) : null;
  return (
    <div
      className="flex items-center gap-2.5 rounded-lg border border-border bg-surface-0 px-3 py-2"
      aria-live="polite"
      data-testid="midi-activity"
    >
      <span className="relative flex size-2.5 shrink-0">
        <span className="absolute inset-0 rounded-full bg-surface-4" />
        {activity && (
          <span
            key={activity.at}
            className="absolute inset-0 animate-out rounded-full bg-primary shadow-[0_0_8px_var(--primary)] fill-mode-forwards fade-out duration-700"
          />
        )}
      </span>
      {text ? (
        <span className="flex min-w-0 items-baseline gap-2 text-[12px]">
          <span className="font-mono font-semibold tabular-nums">{text.primary}</span>
          <span className="truncate text-muted-foreground">{text.secondary}</span>
        </span>
      ) : (
        <span className="text-[12px] text-muted-foreground">Waiting for MIDI… hit a pad or turn a knob.</span>
      )}
    </div>
  );
}

// --- Learn banner -----------------------------------------------------------------

function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="inline-flex h-5 min-w-5 items-center justify-center gap-0.5 rounded border border-border bg-surface-2 px-1 font-mono text-[11px] text-muted-foreground">
      {children}
    </kbd>
  );
}

function LearnBanner({ learn }: { learn: Exclude<LearnState, { kind: 'idle' }> }) {
  let title: string;
  let hint: string;
  if (learn.kind === 'pad' && learn.sequence) {
    title = `Hit pad ${learn.pad + 1}`;
    hint = `${learn.pad + 1} of ${PAD_COUNT} · bank ${learn.pad < PADS_PER_BANK ? 'A' : 'B'}${learn.pad === PADS_PER_BANK ? ' (switch your controller to bank B)' : ''}`;
  } else if (learn.kind === 'pad') {
    title = `Hit the hardware pad for Pad ${learn.pad + 1}`;
    hint = 'The next note you play is assigned to this pad.';
  } else {
    title = `Turn a knob for ${knobTargetLabel(learn.target)}`;
    hint = 'The next knob (CC) you move is bound to this target.';
  }
  const sequence = learn.kind === 'pad' && learn.sequence;

  return (
    <div
      role="status"
      className="flex items-center gap-3 rounded-lg border border-primary/40 bg-primary/10 px-3 py-2.5"
    >
      <span className="size-2 shrink-0 animate-glow-pulse rounded-full bg-primary" />
      <div className="min-w-0 flex-1">
        <div className="text-[14px] font-semibold tracking-tight">{title}</div>
        <div className="text-[11px] text-muted-foreground">{hint}</div>
      </div>
      {sequence && (
        <>
          <Button
            size="xs"
            variant="ghost"
            onClick={() => controllerMapping.previousPad()}
            disabled={learn.pad === 0}
            title="Previous pad (Backspace)"
          >
            <Delete />
            Back
          </Button>
          <Button size="xs" variant="ghost" onClick={() => controllerMapping.skipPad()} title="Leave this pad as it is">
            <SkipForward />
            Skip
          </Button>
        </>
      )}
      <Button size="xs" variant="outline" onClick={() => controllerMapping.cancelLearn()} title="Cancel (Esc)">
        Cancel <Kbd>Esc</Kbd>
      </Button>
    </div>
  );
}

// --- Pads ---------------------------------------------------------------------------

function PadChannelSelect({ channel }: { channel: number | null }) {
  return (
    <Select
      value={channel === null ? ANY : String(channel)}
      onValueChange={(v) => controllerMapping.setPadChannel(v === ANY ? null : Number(v))}
    >
      <SelectTrigger size="sm" className="w-[128px] text-[12px]" aria-label="Pad MIDI channel">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={ANY}>Any channel</SelectItem>
        {CHANNELS.map((ch) => (
          <SelectItem key={ch} value={String(ch)}>
            Channel {ch}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

interface PadBankProps {
  bank: 0 | 1;
  profile: ControllerProfile;
  learn: LearnState;
  activity: MidiActivity | null;
}

/** One bank, laid out like the hardware: pads 1–4 on the bottom row, 5–8 on top. */
function PadBank({ bank, profile, learn, activity }: PadBankProps) {
  const base = bank * PADS_PER_BANK;
  const half = PADS_PER_BANK / 2;
  const rows = [
    Array.from({ length: half }, (_, i) => base + half + i),
    Array.from({ length: half }, (_, i) => base + i),
  ];
  return (
    <div className="flex flex-col gap-1.5 rounded-lg border border-border bg-surface-0 p-2">
      <div className="text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">
        Bank {bank === 0 ? 'A' : 'B'}
      </div>
      {rows.map((row, r) => (
        <div key={r} className="grid grid-cols-4 gap-1.5">
          {row.map((pad) => (
            <PadCell
              key={pad}
              pad={pad}
              note={profile.padMap.notes[pad]}
              learning={learn.kind === 'pad' && learn.pad === pad}
              hit={activity?.kind === 'note' && activity.pad === pad ? activity.at : null}
            />
          ))}
        </div>
      ))}
    </div>
  );
}

interface PadCellProps {
  pad: PadIndex;
  note: number | null;
  learning: boolean;
  /** Timestamp of the latest hit on this pad (re-keys the flash). */
  hit: number | null;
}

function PadCell({ pad, note, learning, hit }: PadCellProps) {
  return (
    <div className="group relative">
      <button
        type="button"
        onClick={() => controllerMapping.startPadLearn(pad)}
        aria-label={`Pad ${pad + 1}: ${note === null ? 'unassigned' : `note ${note}`}. Click to learn.`}
        aria-pressed={learning}
        className={cn(
          'relative flex h-14 w-full flex-col items-start justify-between overflow-hidden rounded-md border bg-surface-2 px-2 py-1.5 text-left transition-colors duration-150 hover:border-input hover:bg-surface-3 focus-visible:outline-2 focus-visible:outline-ring',
          learning ? 'border-primary ring-2 ring-primary/50' : 'border-border',
        )}
      >
        {hit !== null && (
          <span
            key={hit}
            aria-hidden
            className="pointer-events-none absolute inset-0 animate-out bg-primary/35 fill-mode-forwards fade-out duration-500"
          />
        )}
        {learning && <span aria-hidden className="pointer-events-none absolute inset-0 animate-glow-pulse bg-primary/15" />}
        <span className="relative font-mono text-[11px] font-semibold text-muted-foreground">{padLabel(pad)}</span>
        <span
          className={cn(
            'relative flex items-baseline gap-1.5 font-mono text-[11px] whitespace-nowrap tabular-nums',
            note === null ? 'text-muted-foreground' : 'text-foreground',
          )}
        >
          {learning ? (
            'hit…'
          ) : note === null ? (
            '—'
          ) : (
            <>
              <span>{midiToNoteName(note)}</span>
              <span className="text-[11px] text-muted-foreground">{note}</span>
            </>
          )}
        </span>
      </button>
      {note !== null && !learning && (
        <button
          type="button"
          onClick={() => controllerMapping.clearPad(pad)}
          aria-label={`Clear pad ${pad + 1} mapping`}
          title="Clear mapping"
          className="absolute top-1 right-1 hidden size-4 items-center justify-center rounded-sm text-muted-foreground group-hover:flex hover:bg-surface-4 hover:text-foreground focus-visible:flex"
        >
          <X className="size-3" />
        </button>
      )}
    </div>
  );
}

// --- Knobs ---------------------------------------------------------------------------

function KnobTable({ profile, learn, activity }: { profile: ControllerProfile; learn: LearnState; activity: MidiActivity | null }) {
  const mappings = profile.ccMappings;
  const learningIndex = learn.kind === 'knob' ? learn.index : undefined;

  const addKnob = () => {
    // Default the new row to the first target that has no knob yet.
    const unused = KNOB_TARGET_OPTIONS.find((o) => !mappings.some((m) => sameKnobTarget(m.target, o.target)));
    controllerMapping.startKnobLearn((unused ?? KNOB_TARGET_OPTIONS[0]).target, mappings.length);
  };

  return (
    <section aria-labelledby="knobs-heading" className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <h3 id="knobs-heading" className="text-[12px] font-semibold tracking-wide text-muted-foreground uppercase">
          Knobs
        </h3>
        <Button size="sm" variant="outline" onClick={addKnob}>
          <Plus />
          Add knob
        </Button>
      </div>
      <div className="max-h-[220px] overflow-y-auto rounded-lg border border-border bg-surface-0">
        <table className="w-full text-[12px]">
          <thead className="sticky top-0 bg-surface-0 text-[11px] tracking-wide text-muted-foreground uppercase">
            <tr className="border-b border-border">
              <th className="px-3 py-1.5 text-left font-semibold">CC</th>
              <th className="px-2 py-1.5 text-left font-semibold">Target</th>
              <th className="px-2 py-1.5 text-right font-semibold"><span className="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {mappings.map((m, index) => {
              const rowLearning = learn.kind === 'knob' && learningIndex === index;
              const live = activity?.kind === 'cc' && activity.cc === m.cc;
              return (
                <tr key={`${index}-${m.cc}`} className="border-b border-border/60 last:border-0">
                  <td className="px-3 py-1">
                    <span className="flex items-center gap-2 font-mono tabular-nums">
                      {rowLearning ? <span className="text-primary">turn…</span> : m.cc}
                      {m.channel !== null && <span className="text-muted-foreground">ch {m.channel}</span>}
                      {live && !rowLearning && (
                        <span className="h-1 w-10 overflow-hidden rounded-full bg-surface-3">
                          <span className="block h-full bg-primary" style={{ width: `${(activity.value / 127) * 100}%` }} />
                        </span>
                      )}
                    </span>
                  </td>
                  <td className="px-2 py-1">
                    <Select
                      value={knobTargetKey(m.target)}
                      onValueChange={(key) => {
                        const target = knobTargetFromKey(key);
                        if (target) controllerMapping.setCCMapping(index, { target });
                      }}
                    >
                      <SelectTrigger size="sm" className="h-7 w-[170px] text-[12px]" aria-label={`Target for CC ${m.cc}`}>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {KNOB_TARGET_OPTIONS.map((o) => (
                          <SelectItem key={o.key} value={o.key}>
                            {o.label}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </td>
                  <td className="px-2 py-1">
                    <div className="flex items-center justify-end gap-1">
                      <Button
                        size="xs"
                        variant={rowLearning ? 'secondary' : 'ghost'}
                        onClick={() =>
                          rowLearning ? controllerMapping.cancelLearn() : controllerMapping.startKnobLearn(m.target, index)
                        }
                        title="Move a knob to bind it to this target"
                      >
                        <CornerDownLeft />
                        {rowLearning ? 'Cancel' : 'Learn'}
                      </Button>
                      <Button
                        size="icon-xs"
                        variant="ghost"
                        onClick={() => controllerMapping.removeCCMapping(index)}
                        aria-label={`Remove CC ${m.cc} mapping`}
                      >
                        <Trash2 />
                      </Button>
                    </div>
                  </td>
                </tr>
              );
            })}
            {learn.kind === 'knob' && learningIndex !== undefined && learningIndex >= mappings.length && (
              <tr>
                <td className="px-3 py-2 font-mono text-primary">turn…</td>
                <td className="px-2 py-2" colSpan={2}>
                  {knobTargetLabel(learn.target)}
                </td>
              </tr>
            )}
            {mappings.length === 0 && learn.kind !== 'knob' && (
              <tr>
                <td colSpan={3} className="px-3 py-3 text-center text-muted-foreground">
                  No knobs mapped.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </section>
  );
}
