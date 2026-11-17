import { ChannelStrip } from './ChannelStrip';
import { MasterStrip } from './MasterStrip';
import { useMixer } from '@/hooks/useMixer';
import type { Track } from '@/types/arrangement';

interface MixerViewProps {
  tracks: Track[];
  onSetTrackVolume: (trackId: string, db: number) => void;
  onSetTrackPan: (trackId: string, pan: number) => void;
  onSetTrackMute: (trackId: string, muted: boolean) => void;
  onSetTrackSolo: (trackId: string, solo: boolean) => void;
}

export function MixerView({
  tracks,
  onSetTrackVolume,
  onSetTrackPan,
  onSetTrackMute,
  onSetTrackSolo,
}: MixerViewProps) {
  const levels = useMixer(tracks, true);

  return (
    <section className="flex shrink-0 flex-col border-t border-border bg-surface-0" aria-label="Mixer">
      <div className="flex h-7 items-center gap-2 border-b border-border bg-surface-1 px-3">
        <span className="daw-label">Mixer</span>
        <span className="text-[10.5px] text-muted-foreground">Press X to hide</span>
      </div>
      <div className="flex overflow-x-auto">
        {tracks.map((track) => (
          <ChannelStrip
            key={track.id}
            track={track}
            level={levels.tracks.get(track.id) ?? -Infinity}
            onVolumeChange={(db) => onSetTrackVolume(track.id, db)}
            onPanChange={(pan) => onSetTrackPan(track.id, pan)}
            onMuteToggle={() => onSetTrackMute(track.id, !track.muted)}
            onSoloToggle={() => onSetTrackSolo(track.id, !track.solo)}
          />
        ))}
        {tracks.length === 0 && (
          <div className="flex flex-1 items-center justify-center py-10 text-[12px] text-muted-foreground">
            Add tracks to see mixer channels
          </div>
        )}
        <div className="flex-1" />
        {tracks.length > 0 && <MasterStrip level={levels.master} />}
      </div>
    </section>
  );
}
