import { useEffect, useRef, useState } from 'react';
import type { Marker } from '@/types/arrangement';
import { MARKER_COLORS } from '@/utils/constants';

interface MarkerContextMenuProps {
  x: number;
  y: number;
  marker: Marker;
  onRename: (id: string, name: string) => void;
  onChangeColor: (id: string, color: string) => void;
  onDelete: (id: string) => void;
  onClose: () => void;
}

export function MarkerContextMenu({
  x,
  y,
  marker,
  onRename,
  onChangeColor,
  onDelete,
  onClose,
}: MarkerContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(marker.name);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        onClose();
      }
    };
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('mousedown', handleClick);
    window.addEventListener('keydown', handleKey);
    return () => {
      window.removeEventListener('mousedown', handleClick);
      window.removeEventListener('keydown', handleKey);
    };
  }, [onClose]);

  useEffect(() => {
    if (editing && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [editing]);

  const handleRenameSubmit = () => {
    if (name.trim()) {
      onRename(marker.id, name.trim());
    }
    setEditing(false);
  };

  return (
    <div
      ref={ref}
      className="fixed z-50 min-w-[160px] rounded-md border border-border bg-popover py-1 shadow-lg"
      style={{ left: x, top: y }}
    >
      {editing ? (
        <div className="px-2 py-1">
          <input
            ref={inputRef}
            className="w-full rounded border border-border bg-background px-1.5 py-0.5 text-xs text-foreground outline-none focus:ring-1 focus:ring-primary"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleRenameSubmit();
              if (e.key === 'Escape') setEditing(false);
              e.stopPropagation();
            }}
            onBlur={handleRenameSubmit}
          />
        </div>
      ) : (
        <button
          className="w-full px-3 py-1.5 text-left text-xs text-popover-foreground hover:bg-accent"
          onClick={() => setEditing(true)}
        >
          Rename
        </button>
      )}

      {/* Color picker */}
      <div className="px-3 py-1.5">
        <div className="mb-1 text-[10px] font-semibold text-muted-foreground">Color</div>
        <div className="flex gap-1">
          {MARKER_COLORS.map((c) => (
            <button
              key={c}
              className="h-4 w-4 rounded-full border border-border/50 transition-transform hover:scale-125"
              style={{
                backgroundColor: c,
                outline: c === marker.color ? '2px solid oklch(0.85 0 0)' : undefined,
                outlineOffset: 1,
              }}
              onClick={() => {
                onChangeColor(marker.id, c);
                onClose();
              }}
            />
          ))}
        </div>
      </div>

      <div className="my-0.5 h-px bg-border" />

      <button
        className="w-full px-3 py-1.5 text-left text-xs text-red-500 hover:bg-accent"
        onClick={() => {
          onDelete(marker.id);
          onClose();
        }}
      >
        Delete
      </button>
    </div>
  );
}
