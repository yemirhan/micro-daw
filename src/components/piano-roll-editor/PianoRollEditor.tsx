import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react';
import { usePlaybackPosition } from '@/hooks/useArrangement';
import type { ArrangementTransportState, LoopMarkers, RegionNote, Track } from '@/types/arrangement';
import type { PianoRollTool, PianoRollViewport } from '@/types/pianoRollEditor';
import { midiToNoteName } from '@/utils/noteHelpers';
import {
  centerScrollTop,
  changeLengths,
  clamp,
  copyNotes,
  createDrumLayout,
  createPianoLayout,
  deleteNotes,
  duplicateNotes,
  fitView,
  followScroll,
  formatBeats,
  formatPosition,
  moveNotes,
  nudgeStep,
  pasteNotes,
  quantizeNotes,
  requiredRegionLength,
  sliderToZoom,
  snapFloor,
  zoomFromWheel,
  zoomScroll,
  zoomToSlider,
  DEFAULT_NOTE_DURATION,
  DEFAULT_NOTE_VELOCITY,
  PR_DEFAULT_DRUM_ROW_HEIGHT,
  PR_DEFAULT_PX_PER_BEAT,
  PR_DEFAULT_ROW_HEIGHT,
  PR_MAX_PX_PER_BEAT,
  PR_MAX_ROW_HEIGHT,
  PR_MIN_PX_PER_BEAT,
  PR_MIN_ROW_HEIGHT,
} from '@/utils/pianoRollHelpers';
import { usePianoRollColors } from './canvas';
import { PianoRollToolbar } from './PianoRollToolbar';
import { PianoRollRuler, RULER_HEIGHT } from './PianoRollRuler';
import { KEY_COLUMN_WIDTH, PianoKeyColumn } from './PianoKeyColumn';
import { DrumKeyColumn } from './DrumKeyColumn';
import { NoteGrid } from './NoteGrid';
import { VELOCITY_LANE_HEIGHT, VELOCITY_PAD_BOTTOM, VELOCITY_PAD_TOP, VelocityLane } from './VelocityLane';

export interface PianoRollEditorProps {
  trackId: string;
  regionId: string;
  tracks: Track[];
  /** Playhead beat; omitted = read from the engine here (the host does not re-render per frame). */
  positionBeats?: number;
  transportState: ArrangementTransportState;
  loopEnabled: boolean;
  loopMarkers?: LoopMarkers;
  onClose: () => void;
  /** Replace the region's notes as one undo step; optionally grow the region. */
  onEditNotes: (trackId: string, regionId: string, notes: RegionNote[], label?: string, newLengthBeats?: number) => void;
  onPreviewNote: (trackId: string, midi: number, velocity?: number, durationSec?: number) => void;
  onPlayFrom: (beat: number) => void;
  onStop: () => void;
  onSeek: (beat: number) => void;
  onSetLoop: (startBeat: number, endBeat: number) => void;
  onToggleLoop: () => void;
}

// Clipboard survives closing/reopening the editor (copy between regions).
let noteClipboard: RegionNote[] = [];

const SETTINGS_KEY = 'micro-daw-piano-roll';
interface EditorSettings {
  snap: number;
  drawLength: number;
  follow: boolean;
}
function loadSettings(): EditorSettings {
  const defaults: EditorSettings = { snap: 0.25, drawLength: DEFAULT_NOTE_DURATION, follow: true };
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    if (!raw) return defaults;
    const s = JSON.parse(raw) as Partial<EditorSettings>;
    return {
      snap: typeof s.snap === 'number' && s.snap >= 0 ? s.snap : defaults.snap,
      drawLength: typeof s.drawLength === 'number' && s.drawLength > 0 ? s.drawLength : defaults.drawLength,
      follow: typeof s.follow === 'boolean' ? s.follow : defaults.follow,
    };
  } catch {
    return defaults;
  }
}

const PREVIEW_SEC = 0.3;
const EMPTY_NOTES: RegionNote[] = [];

function isTypingTarget(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  return t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || t instanceof HTMLSelectElement || t.isContentEditable;
}

export function PianoRollEditor(props: PianoRollEditorProps) {
  const { trackId, regionId, tracks, onClose } = props;
  const livePosition = usePlaybackPosition();
  const positionBeats = props.positionBeats ?? livePosition;
  const track = tracks.find((t) => t.id === trackId);
  const region = track?.regions.find((r) => r.id === regionId);
  if (!track || !region) {
    return (
      <div className="flex flex-1 items-center justify-center gap-2 text-sm text-muted-foreground">
        This region no longer exists.
        <button className="underline underline-offset-2 hover:text-foreground" onClick={onClose}>
          Back to arrangement
        </button>
      </div>
    );
  }
  return <PianoRollEditorInner key={`${trackId}:${regionId}`} {...props} positionBeats={positionBeats} track={track} regionStart={region.startBeat} regionLength={region.lengthBeats} regionName={region.name ?? 'Region'} regionNotes={region.notes} />;
}

interface InnerProps extends PianoRollEditorProps {
  positionBeats: number;
  track: Track;
  regionStart: number;
  regionLength: number;
  regionName: string;
  regionNotes: RegionNote[];
}

function PianoRollEditorInner({
  trackId,
  regionId,
  track,
  regionStart,
  regionLength,
  regionName,
  regionNotes,
  positionBeats,
  transportState,
  loopEnabled,
  loopMarkers,
  onClose,
  onEditNotes,
  onPreviewNote,
  onPlayFrom,
  onStop,
  onSeek,
  onSetLoop,
  onToggleLoop,
}: InnerProps) {
  const isDrum = track.instrument.type === 'drums';
  const colors = usePianoRollColors();
  const rootRef = useRef<HTMLDivElement>(null);
  const scrollerRef = useRef<HTMLDivElement | null>(null);
  const [initialSettings] = useState(loadSettings);

  const [tool, setTool] = useState<PianoRollTool>('pointer');
  const [snap, setSnap] = useState(initialSettings.snap);
  const [drawLength, setDrawLength] = useState(initialSettings.drawLength);
  const [follow, setFollow] = useState(initialSettings.follow);
  const [selection, setSelection] = useState<Set<number>>(() => new Set());
  const [draft, setDraft] = useState<RegionNote[] | null>(null);
  const [cue, setCue] = useState(regionStart);
  const [hoverRow, setHoverRow] = useState<number | null>(null);
  const [cursor, setCursor] = useState<{ beat: number | null; pitch: number | null }>({ beat: null, pitch: null });
  const [viewport, setViewport] = useState<PianoRollViewport>({
    pxPerBeat: PR_DEFAULT_PX_PER_BEAT,
    rowHeight: isDrum ? PR_DEFAULT_DRUM_ROW_HEIGHT : PR_DEFAULT_ROW_HEIGHT,
    scrollLeft: 0,
    scrollTop: 0,
    width: 0,
    height: 0,
  });
  const pendingScroll = useRef<{ left?: number; top?: number } | null>(null);
  const rowZoomAcc = useRef(viewport.rowHeight);
  const didInitialFit = useRef(false);

  // Clear the selection when the notes change underneath us (undo/redo, recording...).
  const [committed, setCommitted] = useState(regionNotes);
  const [syncedNotes, setSyncedNotes] = useState(regionNotes);
  if (regionNotes !== syncedNotes) {
    setSyncedNotes(regionNotes);
    if (regionNotes !== committed) {
      setSelection(new Set());
      setDraft(null);
    }
  }

  const notes = draft ?? regionNotes ?? EMPTY_NOTES;

  useEffect(() => {
    try {
      localStorage.setItem(SETTINGS_KEY, JSON.stringify({ snap, drawLength, follow }));
    } catch {
      // ignore quota / privacy errors
    }
  }, [snap, drawLength, follow]);

  // --- Layout ---------------------------------------------------------------
  const drumExtrasKey = isDrum ? [...new Set(notes.map((n) => n.note))].sort((a, b) => a - b).join(',') : '';
  const layout = useMemo(
    () => (isDrum ? createDrumLayout(drumExtrasKey ? drumExtrasKey.split(',').map(Number) : []) : createPianoLayout()),
    [isDrum, drumExtrasKey],
  );

  const ghostNotes = useMemo(() => {
    const out: RegionNote[] = [];
    for (const r of track.regions) {
      if (r.id === regionId || r.audio) continue;
      for (const n of r.notes) out.push({ ...n, startBeat: n.startBeat + r.startBeat - regionStart });
    }
    return out;
  }, [track.regions, regionId, regionStart]);

  let notesEnd = 0;
  for (const n of notes) notesEnd = Math.max(notesEnd, n.startBeat + n.durationBeats);
  const viewBeats = viewport.width / viewport.pxPerBeat;
  const contentBeats = Math.max(regionLength, notesEnd) + Math.max(8, viewBeats * 0.5);

  // --- Scroll / zoom ----------------------------------------------------------
  /** Scroll without a zoom change (the scroll event syncs the viewport state). */
  const applyScroll = useCallback((left?: number, top?: number) => {
    const el = scrollerRef.current;
    if (!el) return;
    if (left !== undefined) el.scrollLeft = left;
    if (top !== undefined) el.scrollTop = top;
  }, []);

  // Apply programmatic scrolls after the content has been resized for the new zoom.
  useLayoutEffect(() => {
    const p = pendingScroll.current;
    const el = scrollerRef.current;
    if (!p || !el) return;
    pendingScroll.current = null;
    if (p.left !== undefined) el.scrollLeft = p.left;
    if (p.top !== undefined) el.scrollTop = p.top;
    // The browser may clamp: keep state in sync with what it actually did.
    const left = el.scrollLeft;
    const top = el.scrollTop;
    setViewport((v) => (v.scrollLeft === left && v.scrollTop === top ? v : { ...v, scrollLeft: left, scrollTop: top }));
  }, [viewport.pxPerBeat, viewport.rowHeight, viewport.scrollLeft, viewport.scrollTop]);

  const handleScroll = useCallback((left: number, top: number) => {
    setViewport((v) => (v.scrollLeft === left && v.scrollTop === top ? v : { ...v, scrollLeft: left, scrollTop: top }));
  }, []);

  const handleResize = useCallback((width: number, height: number) => {
    setViewport((v) => (v.width === width && v.height === height ? v : { ...v, width, height }));
  }, []);

  const zoomX = useCallback(
    (nextPx: number, cursorX?: number) => {
      setViewport((v) => {
        const px = clamp(nextPx, PR_MIN_PX_PER_BEAT, PR_MAX_PX_PER_BEAT);
        if (px === v.pxPerBeat) return v;
        const anchor = cursorX ?? v.width / 2;
        const left = zoomScroll(v.scrollLeft, anchor, v.pxPerBeat, px);
        pendingScroll.current = { ...pendingScroll.current, left };
        return { ...v, pxPerBeat: px, scrollLeft: left };
      });
    },
    [],
  );

  const zoomY = useCallback((nextRow: number, cursorY?: number) => {
    setViewport((v) => {
      rowZoomAcc.current = clamp(nextRow, PR_MIN_ROW_HEIGHT, PR_MAX_ROW_HEIGHT);
      const rh = Math.round(rowZoomAcc.current);
      if (rh === v.rowHeight) return v;
      const anchor = cursorY ?? v.height / 2;
      const top = zoomScroll(v.scrollTop, anchor, v.rowHeight, rh);
      pendingScroll.current = { ...pendingScroll.current, top };
      return { ...v, rowHeight: rh, scrollTop: top };
    });
  }, []);

  const viewportRef = useRef(viewport);
  useLayoutEffect(() => {
    viewportRef.current = viewport;
  });

  const makeWheel = useCallback(
    (source: 'grid' | 'ruler' | 'keys' | 'lane') => (e: WheelEvent) => {
      const el = scrollerRef.current;
      if (!el) return;
      const rect = el.getBoundingClientRect();
      const scale = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? rect.height : 1;
      const dy = e.deltaY * scale;
      const dx = e.deltaX * scale;
      const v = viewportRef.current;
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey) {
        e.preventDefault();
        zoomX(zoomFromWheel(v.pxPerBeat, dy, PR_MIN_PX_PER_BEAT, PR_MAX_PX_PER_BEAT), e.clientX - rect.left);
        return;
      }
      if (e.altKey || ((e.ctrlKey || e.metaKey) && e.shiftKey)) {
        e.preventDefault();
        const delta = dy || dx;
        zoomY(rowZoomAcc.current * Math.exp(-delta * 0.003), e.clientY - rect.top);
        return;
      }
      if (source === 'ruler' || source === 'lane') {
        e.preventDefault();
        el.scrollLeft += dx || dy;
      } else if (source === 'keys') {
        e.preventDefault();
        el.scrollTop += dy;
      }
      // grid: native scrolling
    },
    [zoomX, zoomY],
  );
  const wheelGrid = useMemo(() => makeWheel('grid'), [makeWheel]);
  const wheelRuler = useMemo(() => makeWheel('ruler'), [makeWheel]);
  const wheelKeys = useMemo(() => makeWheel('keys'), [makeWheel]);
  const wheelLane = useMemo(() => makeWheel('lane'), [makeWheel]);

  const fit = useCallback(() => {
    const v = viewportRef.current;
    if (v.width <= 0 || v.height <= 0) return;
    const f = fitView(regionNotes, layout, regionLength, v.width, v.height);
    let rowHeight = f.rowHeight;
    if (!isDrum) rowHeight = clamp(rowHeight, 12, 22);
    const scrollTop = centerScrollTop(regionNotes, layout, rowHeight, v.height);
    rowZoomAcc.current = rowHeight;
    pendingScroll.current = { left: 0, top: scrollTop };
    setViewport((cur) => ({ ...cur, pxPerBeat: f.pxPerBeat, rowHeight, scrollLeft: 0, scrollTop }));
  }, [regionNotes, layout, regionLength, isDrum]);

  // Fit + scroll to the notes once the viewport has a size.
  useEffect(() => {
    if (didInitialFit.current || viewport.width <= 0 || viewport.height <= 0) return;
    didInitialFit.current = true;
    fit();
    rootRef.current?.focus({ preventScroll: true });
  }, [viewport.width, viewport.height, fit]);

  // --- Transport -----------------------------------------------------------------
  const playing = transportState !== 'stopped';
  const playheadRel = playing ? positionBeats - regionStart : null;
  const cueRel = cue - regionStart;

  useEffect(() => {
    if (!follow || playheadRel === null) return;
    const v = viewportRef.current;
    const next = followScroll(playheadRel * v.pxPerBeat, v.scrollLeft, v.width);
    if (next !== null) applyScroll(next);
  }, [follow, playheadRel, applyScroll]);

  const handleSetPosition = useCallback(
    (rel: number) => {
      const abs = Math.max(0, rel + regionStart);
      if (playing) onSeek(abs);
      else setCue(abs);
    },
    [playing, onSeek, regionStart],
  );

  const loopRel = loopMarkers ? { start: loopMarkers.startBeat - regionStart, end: loopMarkers.endBeat - regionStart } : null;
  const handleSetLoop = useCallback(
    (a: number, b: number) => {
      onSetLoop(Math.max(0, a + regionStart), Math.max(0, b + regionStart));
      if (!loopEnabled) onToggleLoop();
    },
    [onSetLoop, onToggleLoop, loopEnabled, regionStart],
  );

  // --- Editing ----------------------------------------------------------------------
  const preview = useCallback(
    (pitch: number, velocity = DEFAULT_NOTE_VELOCITY) => onPreviewNote(trackId, pitch, velocity, PREVIEW_SEC),
    [onPreviewNote, trackId],
  );

  const commit = useCallback(
    (next: RegionNote[], label: string, sel: Iterable<number>) => {
      const grow = requiredRegionLength(next, regionLength);
      setCommitted(next);
      setDraft(null);
      setSelection(new Set(sel));
      onEditNotes(trackId, regionId, next, label, grow > regionLength ? grow : undefined);
    },
    [onEditNotes, trackId, regionId, regionLength],
  );

  const selectedIdx = useMemo(() => [...selection].filter((i) => i < notes.length).sort((a, b) => a - b), [selection, notes.length]);

  const quantize = useCallback(() => {
    if (snap <= 0 || regionNotes.length === 0) return;
    const indices = selectedIdx.length > 0 ? selectedIdx : undefined;
    const next = quantizeNotes(regionNotes, snap, { indices, offset: regionStart });
    commit(next, 'Quantize Notes', selectedIdx);
  }, [snap, regionNotes, selectedIdx, regionStart, commit]);

  const pasteAt = useCallback((): number => {
    const rel = playheadRel ?? cueRel;
    const at = Math.max(0, rel);
    return snap > 0 ? Math.max(0, snapFloor(at, snap, regionStart)) : at;
  }, [playheadRel, cueRel, snap, regionStart]);

  const handleKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.defaultPrevented || isTypingTarget(e.target)) return;
    const mod = e.metaKey || e.ctrlKey;
    const step = nudgeStep(snap);
    const hasSel = selectedIdx.length > 0;
    let handled = true;

    if (mod) {
      switch (e.code) {
        case 'KeyA':
          setSelection(new Set(regionNotes.map((_, i) => i)));
          break;
        case 'KeyC':
          if (hasSel) noteClipboard = copyNotes(regionNotes, selectedIdx);
          break;
        case 'KeyX':
          if (hasSel) {
            noteClipboard = copyNotes(regionNotes, selectedIdx);
            commit(deleteNotes(regionNotes, selectedIdx), 'Cut Notes', []);
          }
          break;
        case 'KeyV':
          if (noteClipboard.length > 0) {
            const res = pasteNotes(regionNotes, noteClipboard, pasteAt(), isDrum);
            commit(res.notes, 'Paste Notes', res.selection);
          }
          break;
        case 'KeyD':
          if (hasSel) {
            const res = duplicateNotes(regionNotes, selectedIdx, snap);
            commit(res.notes, 'Duplicate Notes', res.selection);
          }
          break;
        default:
          handled = false;
      }
    } else {
      switch (e.code) {
        case 'Space':
          if (playing) onStop();
          else onPlayFrom(cue);
          break;
        case 'Delete':
        case 'Backspace':
          if (hasSel) commit(deleteNotes(regionNotes, selectedIdx), selectedIdx.length > 1 ? 'Delete Notes' : 'Delete Note', []);
          break;
        case 'ArrowUp':
        case 'ArrowDown': {
          if (!hasSel) break;
          const dir = e.code === 'ArrowUp' ? -1 : 1; // rows: up = towards row 0
          const rows = isDrum ? dir : dir * (e.shiftKey ? 12 : 1);
          const next = moveNotes(regionNotes, selectedIdx, 0, rows, layout);
          commit(next, 'Transpose Notes', selectedIdx);
          if (selectedIdx.length === 1) preview(next[selectedIdx[0]].note, next[selectedIdx[0]].velocity);
          break;
        }
        case 'ArrowLeft':
        case 'ArrowRight': {
          if (!hasSel) break;
          const dir = e.code === 'ArrowRight' ? 1 : -1;
          if (e.altKey && !isDrum) {
            commit(changeLengths(regionNotes, selectedIdx, dir * step, Math.min(step, 1 / 16)), 'Change Length', selectedIdx);
          } else {
            commit(moveNotes(regionNotes, selectedIdx, dir * step, 0, layout), 'Move Notes', selectedIdx);
          }
          break;
        }
        case 'KeyQ':
          quantize();
          break;
        case 'KeyV':
          setTool('pointer');
          break;
        case 'KeyD':
        case 'KeyB':
          setTool('draw');
          break;
        case 'KeyE':
          setTool('eraser');
          break;
        case 'KeyF':
          fit();
          break;
        case 'Equal':
        case 'NumpadAdd':
          zoomX(viewport.pxPerBeat * 1.25);
          break;
        case 'Minus':
        case 'NumpadSubtract':
          zoomX(viewport.pxPerBeat / 1.25);
          break;
        default:
          handled = false;
      }
    }
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  };

  // --- Derived UI state -----------------------------------------------------------------
  const litPitches = useMemo(() => {
    const s = new Set<number>();
    for (const i of selectedIdx) if (notes[i]) s.add(notes[i].note);
    return s;
  }, [selectedIdx, notes]);

  let status = '';
  if (selectedIdx.length === 1 && notes[selectedIdx[0]]) {
    const n = notes[selectedIdx[0]];
    status = `${layout.rowLabel(layout.pitchToRow(n.note))} · ${formatPosition(n.startBeat + regionStart)} · ${formatBeats(n.durationBeats)} · vel ${Math.round(n.velocity * 127)}`;
  } else if (cursor.beat !== null) {
    const pitchLabel = cursor.pitch !== null ? (isDrum ? layout.rowLabel(layout.pitchToRow(cursor.pitch)) : midiToNoteName(cursor.pitch)) : '';
    status = `${formatPosition(cursor.beat + regionStart)}${pitchLabel ? ` · ${pitchLabel}` : ''}`;
  }

  const handleCursor = useCallback((beat: number | null, pitch: number | null) => setCursor({ beat, pitch }), []);
  const handleDrawLength = useCallback((beats: number) => setDrawLength(beats), []);

  return (
    <div
      ref={rootRef}
      data-piano-roll=""
      data-testid="piano-roll-editor"
      tabIndex={-1}
      className="flex min-h-0 flex-1 flex-col overflow-hidden bg-background outline-none"
      onKeyDown={handleKeyDown}
    >
      <PianoRollToolbar
        regionName={regionName}
        trackName={track.name}
        trackColor={track.color}
        tool={tool}
        snap={snap}
        drawLength={drawLength}
        isDrum={isDrum}
        follow={follow}
        zoom={zoomToSlider(viewport.pxPerBeat, PR_MIN_PX_PER_BEAT, PR_MAX_PX_PER_BEAT)}
        noteCount={notes.length}
        selectedCount={selectedIdx.length}
        status={status}
        onBack={onClose}
        onToolChange={setTool}
        onSnapChange={setSnap}
        onDrawLengthChange={setDrawLength}
        onQuantize={quantize}
        onFollowChange={setFollow}
        onZoomChange={(t) => zoomX(sliderToZoom(t, PR_MIN_PX_PER_BEAT, PR_MAX_PX_PER_BEAT))}
        onZoomStep={(dir) => zoomX(viewport.pxPerBeat * (dir > 0 ? 1.25 : 0.8))}
        onRowZoomStep={(dir) => zoomY(viewport.rowHeight + dir * 2)}
        onFit={fit}
      />

      <div className="flex shrink-0">
        <div
          className="flex shrink-0 items-end border-r border-b border-border bg-card px-2 pb-1 font-mono text-[9px] text-muted-foreground"
          style={{ width: KEY_COLUMN_WIDTH, height: RULER_HEIGHT }}
        >
          {snap > 0 ? `grid ${formatBeats(snap)}` : 'grid off'}
        </div>
        <div className="min-w-0 shrink-0" style={{ width: viewport.width }}>
          <PianoRollRuler
            viewport={viewport}
            colors={colors}
            offset={regionStart}
            regionLength={regionLength}
            snap={snap}
            playhead={playheadRel}
            cue={playing ? null : cueRel}
            loop={loopRel}
            loopEnabled={loopEnabled}
            onSetPosition={handleSetPosition}
            onSetLoop={handleSetLoop}
            onToggleLoop={onToggleLoop}
            onWheel={wheelRuler}
          />
        </div>
      </div>

      <div className="flex min-h-0 flex-1">
        {isDrum ? (
          <DrumKeyColumn viewport={viewport} layout={layout} hoverRow={hoverRow} litPitches={litPitches} onPreview={preview} onWheel={wheelKeys} />
        ) : (
          <PianoKeyColumn viewport={viewport} colors={colors} layout={layout} hoverRow={hoverRow} litPitches={litPitches} onPreview={preview} onWheel={wheelKeys} />
        )}
        <NoteGrid
          scrollerRef={scrollerRef}
          viewport={viewport}
          colors={colors}
          layout={layout}
          notes={notes}
          ghostNotes={ghostNotes}
          selection={selection}
          regionLength={regionLength}
          offset={regionStart}
          contentBeats={contentBeats}
          snap={snap}
          tool={tool}
          isDrum={isDrum}
          trackColor={track.color}
          drawLength={drawLength}
          drawVelocity={DEFAULT_NOTE_VELOCITY}
          playhead={playheadRel}
          cue={playing ? null : cueRel}
          onScroll={handleScroll}
          onResize={handleResize}
          onWheel={wheelGrid}
          onDraft={setDraft}
          onCommit={commit}
          onSelectionChange={setSelection}
          onPreview={preview}
          onHoverRow={setHoverRow}
          onDrawLengthChange={handleDrawLength}
          onCursor={handleCursor}
        />
      </div>

      <div className="flex shrink-0 border-t border-border">
        <div
          className="relative shrink-0 border-r border-border bg-card font-mono text-[9px] text-muted-foreground"
          style={{ width: KEY_COLUMN_WIDTH, height: VELOCITY_LANE_HEIGHT }}
        >
          <span className="absolute top-1.5 left-2 font-sans text-[10px] font-semibold tracking-wide uppercase">Vel</span>
          {[
            [127, 1],
            [64, 0.5],
            [1, 0],
          ].map(([label, v]) => (
            <span
              key={label}
              className="absolute right-2 -translate-y-1/2 leading-none"
              style={{ top: VELOCITY_PAD_TOP + (1 - v) * (VELOCITY_LANE_HEIGHT - VELOCITY_PAD_TOP - VELOCITY_PAD_BOTTOM) }}
            >
              {label}
            </span>
          ))}
        </div>
        <div className="min-w-0 shrink-0" style={{ width: viewport.width }}>
          <VelocityLane
            viewport={viewport}
            colors={colors}
            notes={notes}
            selection={selection}
            trackColor={track.color}
            regionLength={regionLength}
            onDraft={setDraft}
            onCommit={(next, label) => commit(next, label, selectedIdx)}
            onWheel={wheelLane}
          />
        </div>
      </div>
    </div>
  );
}
