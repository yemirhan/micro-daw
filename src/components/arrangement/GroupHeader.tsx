import { useState, useRef, useEffect } from 'react';
import { ChevronRight, ChevronDown, X, FolderOpen } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { TrackToggle } from './TrackToggles';
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
      className="group/grp relative flex h-7 items-center gap-1 border-b border-border pr-2 pl-2.5"
      style={{ backgroundColor: `color-mix(in oklab, ${group.color} 9%, var(--surface-2))` }}
    >
      <span aria-hidden className="absolute inset-y-0 left-0 w-1" style={{ backgroundColor: group.color }} />

      <Button
        variant="ghost"
        size="icon-xs"
        className="size-5"
        onClick={() => onToggleCollapsed(group.id)}
        aria-label={group.collapsed ? 'Expand group' : 'Collapse group'}
        aria-expanded={!group.collapsed}
      >
        {group.collapsed ? <ChevronRight className="size-3.5" /> : <ChevronDown className="size-3.5" />}
      </Button>
      <FolderOpen className="size-3.5 shrink-0 text-muted-foreground" />

      {editing ? (
        <input
          ref={inputRef}
          className="h-5 min-w-0 flex-1 rounded-[3px] border border-ring bg-surface-0 px-1 text-[11px] text-foreground outline-none"
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
          className="min-w-0 flex-1 cursor-text truncate text-[11px] font-semibold text-foreground"
          onDoubleClick={() => {
            setName(group.name);
            setEditing(true);
          }}
          title="Double-click to rename"
        >
          {group.name}
          <span className="ml-1.5 font-mono text-[10px] font-normal text-muted-foreground">{group.trackIds.length}</span>
        </span>
      )}

      <TrackToggle kind="mute" active={allMuted} onClick={() => onMuteToggle(group.id, !allMuted)} title="Mute group" />
      <TrackToggle kind="solo" active={anySolo} onClick={() => onSoloToggle(group.id, !anySolo)} title="Solo group" />

      <Button
        variant="ghost"
        size="icon-xs"
        className="size-5 opacity-0 transition-opacity group-hover/grp:opacity-100 focus-visible:opacity-100 hover:text-signal-rec"
        onClick={() => onRemove(group.id)}
        title="Ungroup"
        aria-label="Ungroup"
      >
        <X className="size-3" />
      </Button>
    </div>
  );
}
