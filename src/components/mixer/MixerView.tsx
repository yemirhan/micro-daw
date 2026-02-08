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
    <div className="flex border-t border-border bg-card/80 backdrop-blur-sm overflow-x-auto">
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
      {tracks.length > 0 && <MasterStrip level={levels.master} />}
      {tracks.length === 0 && (
        <div className="flex items-center justify-center w-full py-6 text-xs text-muted-foreground">
          Add tracks to see mixer channels
        </div>
      )}
    </div>
  );
}
