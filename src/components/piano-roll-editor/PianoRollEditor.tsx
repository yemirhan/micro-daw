import { useState, useCallback, useMemo, useRef, useEffect } from 'react';
import type { Arrangement, RegionNote } from '@/types/arrangement';
import type { PianoRollTool } from '@/types/pianoRollEditor';
import { PianoRollToolbar } from './PianoRollToolbar';
import { PianoKeyColumn } from './PianoKeyColumn';
import { DrumKeyColumn } from './DrumKeyColumn';
import { NoteGrid } from './NoteGrid';
import { VelocityLane } from './VelocityLane';
import { PR_NOTE_MIN, PR_NOTE_MAX, DRUM_ROWS } from '@/utils/pianoRollHelpers';
import { MIN_PX_PER_BEAT, MAX_PX_PER_BEAT } from '@/utils/constants';

interface PianoRollEditorProps {
  trackId: string;
  regionId: string;
  arrangement: Arrangement;
  onClose: () => void;
  onUpdateNotes: (trackId: string, regionId: string, notes: RegionNote[]) => void;
  onResizeRegion: (trackId: string, regionId: string, newStartBeat?: number, newLengthBeats?: number) => void;
}

const DEFAULT_PX_PER_BEAT = 40;
const KEY_COLUMN_WIDTH = 60;
const VELOCITY_LANE_HEIGHT = 60;
const SPRING_THRESHOLD_MS = 200;

export function PianoRollEditor({
  trackId,
  regionId,
  arrangement,
  onClose,
  onUpdateNotes,
  onResizeRegion,
}: PianoRollEditorProps) {
  const [tool, setTool] = useState<PianoRollTool>('pointer');
  const [snapValue, setSnapValue] = useState(0.25);
  const [selectedIndices, setSelectedIndices] = useState<Set<number>>(new Set());
  const [pxPerBeat, setPxPerBeat] = useState(DEFAULT_PX_PER_BEAT);
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const keyColumnRef = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [visibleHeight, setVisibleHeight] = useState(400);

  // Spring-loaded tool refs
  const previousToolRef = useRef<PianoRollTool | null>(null);
  const springTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isSpringLoaded = useRef(false);

  const track = useMemo(
    () => arrangement.tracks.find((t) => t.id === trackId),
    [arrangement, trackId],
  );
  const region = useMemo(
    () => track?.regions.find((r) => r.id === regionId),
    [track, regionId],
  );

  const isDrum = track?.instrument.type === 'drums';
  const rowHeight = isDrum ? 28 : 14;
  const notes = region?.notes ?? [];

  // Sync scroll between key column and grid
  const handleScroll = useCallback(() => {
    if (!scrollContainerRef.current || !keyColumnRef.current) return;
    keyColumnRef.current.scrollTop = scrollContainerRef.current.scrollTop;
    setScrollTop(scrollContainerRef.current.scrollTop);
  }, []);

  // Set initial scroll to center around middle C area
  useEffect(() => {
    if (!scrollContainerRef.current || isDrum) return;
    const middleCRow = PR_NOTE_MAX - 60; // MIDI 60 = C4
    const scrollY = middleCRow * rowHeight - scrollContainerRef.current.clientHeight / 2;
    scrollContainerRef.current.scrollTop = Math.max(0, scrollY);
    setVisibleHeight(scrollContainerRef.current.clientHeight);
  }, [rowHeight, isDrum]);

  useEffect(() => {
    if (!scrollContainerRef.current) return;
    const obs = new ResizeObserver(() => {
      if (scrollContainerRef.current) {
        setVisibleHeight(scrollContainerRef.current.clientHeight);
      }
    });
    obs.observe(scrollContainerRef.current);
    return () => obs.disconnect();
  }, []);

  const handleNotesChange = useCallback(
    (newNotes: RegionNote[]) => {
      onUpdateNotes(trackId, regionId, newNotes);
    },
    [trackId, regionId, onUpdateNotes],
  );

  const handleVelocityChange = useCallback(
    (noteIndex: number, velocity: number) => {
      const newNotes = [...notes];
      newNotes[noteIndex] = { ...newNotes[noteIndex], velocity };
      onUpdateNotes(trackId, regionId, newNotes);
    },
    [notes, trackId, regionId, onUpdateNotes],
  );

  // Spring-loaded tool: pointer down on a tool button
  const handleToolPointerDown = useCallback((newTool: PianoRollTool) => {
    // If already the current tool, nothing to do
    setTool((currentTool) => {
      if (currentTool === newTool) return currentTool;

      previousToolRef.current = currentTool;
      isSpringLoaded.current = false;

      // Start a timer — if held past threshold, it becomes spring-loaded
      if (springTimerRef.current !== null) {
        clearTimeout(springTimerRef.current);
      }
      springTimerRef.current = setTimeout(() => {
        springTimerRef.current = null;
        isSpringLoaded.current = true;

        // Add a global pointerup listener to revert when released
        const revert = () => {
          if (previousToolRef.current !== null) {
            setTool(previousToolRef.current);
            previousToolRef.current = null;
          }
          isSpringLoaded.current = false;
          window.removeEventListener('pointerup', revert);
        };
        window.addEventListener('pointerup', revert);
      }, SPRING_THRESHOLD_MS);

      return newTool;
    });
  }, []);

  // Spring-loaded tool: pointer up on a tool button
  const handleToolPointerUp = useCallback(() => {
    // If timer hasn't fired yet (<200ms), this is a permanent switch
    if (springTimerRef.current !== null) {
      clearTimeout(springTimerRef.current);
      springTimerRef.current = null;
      previousToolRef.current = null;
      isSpringLoaded.current = false;
      // Tool already set to the new tool — keep it (permanent switch)
    }
    // If spring-loaded, the global pointerup listener handles revert
  }, []);

  // Tool keyboard shortcuts
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      switch (e.key) {
        case 'v':
        case 'V':
          setTool('pointer');
          break;
        case 'd':
        case 'D':
          setTool('draw');
          break;
        case 'e':
        case 'E':
          setTool('eraser');
          break;
      }
    };
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, []);

  // Pinch-to-zoom (Ctrl+wheel / trackpad pinch)
  useEffect(() => {
    const container = scrollContainerRef.current;
    if (!container) return;

    const handleWheel = (e: WheelEvent) => {
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();

      const rect = container.getBoundingClientRect();
      const cursorX = e.clientX - rect.left + container.scrollLeft;
      const beatAtCursor = cursorX / pxPerBeat;

      const zoomDelta = -e.deltaY * 0.5;
      const newPxPerBeat = Math.round(
        Math.max(MIN_PX_PER_BEAT, Math.min(MAX_PX_PER_BEAT, pxPerBeat + zoomDelta))
      );

      if (newPxPerBeat !== pxPerBeat) {
        setPxPerBeat(newPxPerBeat);
        // Adjust scroll so the beat under the cursor stays in place
        const newCursorX = beatAtCursor * newPxPerBeat;
        container.scrollLeft = newCursorX - (e.clientX - rect.left);
      }
    };

    container.addEventListener('wheel', handleWheel, { passive: false });
    return () => container.removeEventListener('wheel', handleWheel);
  }, [pxPerBeat]);

  // Cleanup spring timer on unmount
  useEffect(() => {
    return () => {
      if (springTimerRef.current !== null) {
        clearTimeout(springTimerRef.current);
      }
    };
  }, []);

  if (!track || !region) {
    return (
      <div className="flex flex-1 items-center justify-center text-muted-foreground text-sm">
        Region not found.
        <button className="ml-2 underline" onClick={onClose}>Go back</button>
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      <PianoRollToolbar
        tool={tool}
        snapValue={snapValue}
        regionName={region.name}
        onToolChange={setTool}
        onSnapChange={setSnapValue}
        onBack={onClose}
        onToolPointerDown={handleToolPointerDown}
        onToolPointerUp={handleToolPointerUp}
      />

      <div className="flex flex-1 overflow-hidden">
        {/* Key column */}
        <div
          ref={keyColumnRef}
          className="shrink-0 overflow-hidden border-r border-border bg-card/60"
          style={{ width: KEY_COLUMN_WIDTH }}
        >
          {isDrum ? (
            <DrumKeyColumn rowHeight={rowHeight} />
          ) : (
            <PianoKeyColumn
              rowHeight={rowHeight}
              scrollTop={scrollTop}
              visibleHeight={visibleHeight}
            />
          )}
        </div>

        {/* Grid + velocity */}
        <div className="flex flex-1 flex-col overflow-hidden">
          {/* Grid scroll area */}
          <div
            ref={scrollContainerRef}
            className="flex-1 overflow-auto"
            onScroll={handleScroll}
          >
            <NoteGrid
              notes={notes}
              selectedIndices={selectedIndices}
              regionLengthBeats={region.lengthBeats}
              pxPerBeat={pxPerBeat}
              rowHeight={rowHeight}
              snapValue={snapValue}
              tool={tool}
              isDrum={isDrum}
              trackColor={track.color}
              onNotesChange={handleNotesChange}
              onSelectionChange={setSelectedIndices}
            />
          </div>

          {/* Velocity lane */}
          <VelocityLane
            notes={notes}
            selectedIndices={selectedIndices}
            pxPerBeat={pxPerBeat}
            color={track.color}
            height={VELOCITY_LANE_HEIGHT}
            onVelocityChange={handleVelocityChange}
          />
        </div>
      </div>
    </div>
  );
}
