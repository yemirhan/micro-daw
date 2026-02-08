import { useState, useRef, useEffect } from 'react';
import { ChevronRight, ChevronDown, Volume2, VolumeX, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import type { TrackGroup } from '@/types/arrangement';

interface GroupHeaderProps {
  group: TrackGroup;
  allMuted: boolean;
  anySolo: boolean;
  onToggleCollapsed: (id: string) => void;
  onRename: (id: string, name: string) => void;
  onMuteToggle: (id: string, muted: boolean) => void;
  onSoloToggle: (id: string, solo: boolean) => void;
  onRemove: (id: string) => void;
}

export function GroupHeader({
  group,
  allMuted,
  anySolo,
  onToggleCollapsed,
  onRename,
  onMuteToggle,
  onSoloToggle,
  onRemove,
}: GroupHeaderProps) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(group.name);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editing && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [editing]);

  const handleSubmit = () => {
    if (name.trim()) {
      onRename(group.id, name.trim());
    }
    setEditing(false);
  };

  return (
    <div
      className="flex h-7 items-center gap-1 border-b border-border/50 px-1"
      style={{ backgroundColor: `color-mix(in oklch, ${group.color} 10%, transparent)` }}
    >
      {/* Color strip */}
      <div className="h-4 w-1 rounded-full" style={{ backgroundColor: group.color }} />

      {/* Collapse chevron */}
      <Button
        variant="ghost"
        size="sm"
        className="h-5 w-5 p-0"
        onClick={() => onToggleCollapsed(group.id)}
      >
        {group.collapsed ? (
          <ChevronRight className="h-3 w-3" />
        ) : (
          <ChevronDown className="h-3 w-3" />
        )}
      </Button>

      {/* Group name */}
      {editing ? (
        <input
          ref={inputRef}
          className="h-5 flex-1 rounded border border-border bg-background px-1 text-[10px] text-foreground outline-none"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') handleSubmit();
            if (e.key === 'Escape') setEditing(false);
            e.stopPropagation();
          }}
          onBlur={handleSubmit}
        />
      ) : (
        <span
          className="flex-1 cursor-pointer truncate text-[10px] font-semibold text-muted-foreground"
          onDoubleClick={() => {
            setName(group.name);
            setEditing(true);
          }}
        >
          {group.name}
          <span className="ml-1 text-[9px] font-normal">({group.trackIds.length})</span>
        </span>
      )}

      {/* M/S buttons */}
      <Button
        variant="ghost"
        size="sm"
        className={`h-4 w-5 p-0 text-[9px] font-bold ${allMuted ? 'text-red-400' : 'text-muted-foreground'}`}
        onClick={() => onMuteToggle(group.id, !allMuted)}
        title="Mute group"
      >
        {allMuted ? <VolumeX className="h-3 w-3" /> : <Volume2 className="h-3 w-3" />}
      </Button>
      <Button
        variant="ghost"
        size="sm"
        className={`h-4 w-5 p-0 text-[9px] font-bold ${anySolo ? 'text-yellow-400' : 'text-muted-foreground'}`}
        onClick={() => onSoloToggle(group.id, !anySolo)}
        title="Solo group"
      >
        S
      </Button>

      {/* Remove group */}
      <Button
        variant="ghost"
        size="sm"
        className="h-4 w-4 p-0 text-muted-foreground hover:text-red-400"
        onClick={() => onRemove(group.id)}
        title="Ungroup"
      >
        <X className="h-2.5 w-2.5" />
      </Button>
    </div>
  );
}
