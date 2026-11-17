import { useCallback, useEffect, useRef, useState, type CSSProperties } from 'react';

interface StartScreenProps {
  /** Unlocks the AudioContext; must be called from inside a user gesture. */
  onStart: () => Promise<void>;
  /**
   * Optional: the app is usable (audio running). Lets the intro leave as soon
   * as audio is up, without waiting for the rest of onStart (e.g. MIDI access).
   */
  ready?: boolean;
  /** Called once the exit transition has finished; the screen can be unmounted. */
  onExited?: () => void;
}

type Phase = 'idle' | 'starting' | 'exiting';

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

const STAGE_W = 560;
const STAGE_H = 132;
const MID = STAGE_H / 2;
/** Fixed lockup box so the trace can resolve exactly into the mark's centre. */
const LOCKUP_W = 344;
const MARK = 48;
const LOCKUP_LEFT = (STAGE_W - LOCKUP_W) / 2;
const MARK_CENTER_X = LOCKUP_LEFT + MARK / 2;

/** A plucked-string trace: sharp attack, a few bright cycles, exponential decay. */
function buildTrace(): string {
  const attackStart = 64;
  const attackEnd = 96;
  const pts: string[] = [];
  for (let x = 0; x <= STAGE_W; x += 2) {
    let env = 0;
    if (x >= attackStart && x < attackEnd) env = (x - attackStart) / (attackEnd - attackStart);
    else if (x >= attackEnd) env = Math.exp(-(x - attackEnd) / 120);
    // Slight downward chirp + a second partial so it reads as a real signal.
    const t = x - attackStart;
    const phase = (t / 26) * Math.PI * 2 * (1 - t / (STAGE_W * 4));
    const v = Math.sin(phase) * 0.82 + Math.sin(phase * 2.01 + 0.6) * 0.18 * env;
    const y = MID - v * env * (MID - 10);
    pts.push(`${x === 0 ? 'M' : 'L'}${x} ${y.toFixed(1)}`);
  }
  return pts.join(' ');
}

const TRACE_PATH = buildTrace();

function prefersReducedMotion(): boolean {
  return typeof window !== 'undefined' && !!window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
}

/** The app mark: half sine (analog), half square (digital). */
function MarkGlyph({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 48 48" width={MARK} height={MARK} className={className} aria-hidden="true">
      <path
        d="M10 25 C 13 12, 20 12, 24 25 L 24 33 L 37 33 L 37 25"
        fill="none"
        stroke="currentColor"
        strokeWidth={3.25}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export function StartScreen({ onStart, ready = false, onExited }: StartScreenProps) {
  const [phase, setPhase] = useState<Phase>('idle');
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState<string | null>(null);
  // Hold the intro until the window is actually on screen (Electron shows it on ready-to-show).
  const [visible, setVisible] = useState(() => typeof document === 'undefined' || document.visibilityState === 'visible');
  const phaseRef = useRef<Phase>('idle');
  const exitTimer = useRef<number | null>(null);

  useEffect(() => {
    let alive = true;
    window.electronAPI
      ?.getAppVersion()
      .then((v) => alive && setVersion(v))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    if (visible) return;
    const onVis = () => {
      if (document.visibilityState === 'visible') setVisible(true);
    };
    document.addEventListener('visibilitychange', onVis);
    // Never stay paused forever if the event is missed.
    const fallback = window.setTimeout(() => setVisible(true), 1500);
    return () => {
      document.removeEventListener('visibilitychange', onVis);
      window.clearTimeout(fallback);
    };
  }, [visible]);

  const beginExit = useCallback(() => {
    if (phaseRef.current === 'exiting') return;
    phaseRef.current = 'exiting';
    setPhase('exiting');
    const duration = prefersReducedMotion() ? 160 : 380;
    exitTimer.current = window.setTimeout(() => onExited?.(), duration);
  }, [onExited]);

  useEffect(() => () => {
    if (exitTimer.current !== null) window.clearTimeout(exitTimer.current);
  }, []);

  // Leave as soon as the app reports it is usable.
  useEffect(() => {
    if (ready && phaseRef.current === 'starting') beginExit();
  }, [ready, beginExit]);

  /** Must run synchronously inside the gesture handler (AudioContext unlock). */
  const start = useCallback(() => {
    if (phaseRef.current !== 'idle') return;
    phaseRef.current = 'starting';
    setPhase('starting');
    setError(null);
    onStart().then(
      () => beginExit(),
      (err: unknown) => {
        console.error('Failed to start:', err);
        phaseRef.current = 'idle';
        setPhase('idle');
        setError('Audio could not start. Click to try again.');
      },
    );
  }, [onStart, beginExit]);

  // Any key starts (Enter / Space included); leave Tab, Escape and shortcuts alone.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.repeat || e.metaKey || e.ctrlKey || e.altKey) return;
      if (['Tab', 'Escape', 'Shift', 'Control', 'Alt', 'Meta', 'CapsLock'].includes(e.key)) return;
      e.preventDefault();
      start();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [start]);

  const busy = phase !== 'idle';
  const status = phase === 'idle' ? (error ? 'Error' : 'Standby') : 'Starting';

  return (
    <div
      className={`intro-root fixed inset-0 z-[200] flex select-none flex-col ${visible ? '' : 'intro-paused'} ${phase === 'exiting' ? 'intro-exiting pointer-events-none' : ''}`}
      onClick={start}
      role="presentation"
    >
      {/* macOS title bar drag area */}
      <div className="h-10 shrink-0" style={{ WebkitAppRegion: 'drag' } as CSSProperties} />

      <div className="intro-content flex flex-1 flex-col items-center justify-center pb-10">
        {/* ── Stage: trace → mark + wordmark ── */}
        <div className="relative" style={{ width: STAGE_W, height: STAGE_H }}>
          {/* Trace (revealed by a sliding cover whose left edge is the playhead) */}
          <div className="absolute inset-0 overflow-hidden">
            <svg
              className="intro-wave absolute inset-0"
              width={STAGE_W}
              height={STAGE_H}
              viewBox={`0 0 ${STAGE_W} ${STAGE_H}`}
              aria-hidden="true"
            >
              <path
                d={TRACE_PATH}
                fill="none"
                stroke="var(--primary)"
                strokeWidth={2}
                strokeLinejoin="round"
                strokeLinecap="round"
              />
            </svg>
            <div className="intro-cover absolute inset-y-0 left-0" style={{ width: STAGE_W + 8, background: 'var(--intro-bg)' }}>
              <div className="intro-playhead absolute inset-y-3 left-0 w-[2px] -translate-x-1/2 rounded-full bg-primary" />
            </div>
          </div>

          {/* Graticule baseline */}
          <div
            className="intro-baseline pointer-events-none absolute inset-x-0 h-px"
            style={{ top: MID, background: 'oklch(1 0 0 / 0.14)' }}
          />

          {/* The trace flattens and contracts into the mark */}
          <div
            className="intro-flat pointer-events-none absolute inset-x-0 h-[2px] rounded-full bg-primary"
            style={{ top: MID - 1, transformOrigin: `${MARK_CENTER_X}px 50%` }}
          />

          {/* Lockup */}
          <div
            className="absolute flex items-center gap-4"
            style={{ left: LOCKUP_LEFT, width: LOCKUP_W, top: MID - MARK / 2, height: MARK }}
          >
            <div
              className="intro-mark flex shrink-0 items-center justify-center rounded-[11px] bg-primary text-primary-foreground"
              style={{ width: MARK, height: MARK }}
            >
              <MarkGlyph className="intro-glyph" />
            </div>
            <div className="intro-word flex items-baseline gap-[0.3em] whitespace-nowrap font-display leading-none tracking-[-0.03em]">
              <span className="text-[40px] font-extrabold text-foreground">micro</span>
              <span className="text-[40px] font-bold text-muted-foreground">daw</span>
            </div>
          </div>
        </div>

        {/* Tagline */}
        <p className="intro-tagline daw-label mt-6 !tracking-[0.28em]">Sample&nbsp;&nbsp;·&nbsp;&nbsp;Play&nbsp;&nbsp;·&nbsp;&nbsp;Record</p>

        {/* Call to action */}
        <div className="intro-cta mt-10 flex flex-col items-center gap-3">
          <button
            type="button"
            data-testid="start-button"
            disabled={busy}
            onClick={(e) => {
              e.stopPropagation();
              start();
            }}
            className="group inline-flex h-10 items-center gap-3 rounded-md border border-primary/60 bg-primary/10 pl-4 pr-2 text-sm font-semibold text-foreground transition-colors duration-150 hover:bg-primary/20 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:border-border disabled:bg-surface-2 disabled:text-muted-foreground"
          >
            {busy ? 'Starting audio engine…' : 'Start session'}
            <kbd
              className={`inline-flex h-6 min-w-6 items-center justify-center rounded-sm border px-1.5 font-mono text-[11px] font-medium transition-colors duration-150 ${
                busy
                  ? 'border-border text-muted-foreground'
                  : 'border-primary/50 bg-primary text-primary-foreground'
              }`}
            >
              ↵
            </kbd>
          </button>
          <p className={`text-xs ${error ? 'text-signal-rec' : 'text-muted-foreground'}`} role="status">
            {error ?? 'Click anywhere or press any key. This turns on audio and MIDI.'}
          </p>
        </div>
      </div>

      {/* ── Footer status strip ── */}
      <div className="intro-footer flex h-9 shrink-0 items-center justify-between px-5 font-mono text-[10.5px] uppercase tracking-[0.14em] text-muted-foreground">
        <span className="tabular">{version ? `v${version}` : 'micro daw'}</span>
        <span className="flex items-center gap-2">
          <span
            className={`size-1.5 rounded-full ${
              error ? 'bg-signal-rec' : busy ? 'intro-led-busy bg-primary' : 'intro-led bg-signal-play'
            }`}
          />
          Audio engine · {status}
        </span>
      </div>
    </div>
  );
}
