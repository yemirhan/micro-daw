import { useEffect, useRef } from 'react';

interface RegionContextMenuProps {
  x: number;
  y: number;
  onSplit: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
  onClose: () => void;
  onCopy?: () => void;
  onPaste?: () => void;
  canPaste?: boolean;
  onQuantize?: () => void;
  isAudioRegion?: boolean;
}

export function RegionContextMenu({
  x,
  y,
  onSplit,
  onDuplicate,
  onDelete,
  onClose,
  onCopy,
  onPaste,
  canPaste,
  onQuantize,
  isAudioRegion,
}: RegionContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);

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

  const items = [
    { label: 'Split at playhead', action: onSplit },
    { label: 'Duplicate', action: onDuplicate },
    ...(onCopy ? [{ label: 'Copy', action: onCopy }] : []),
    ...(onPaste && canPaste ? [{ label: 'Paste', action: onPaste }] : []),
    ...(!isAudioRegion && onQuantize ? [{ label: 'Quantize', action: onQuantize }] : []),
    { label: 'Delete', action: onDelete, destructive: true },
  ];

  return (
    <div
      ref={ref}
      className="fixed z-50 min-w-[140px] rounded-md border border-border bg-popover py-1 shadow-lg"
      style={{ left: x, top: y }}
    >
      {items.map((item) => (
        <button
          key={item.label}
          className={`w-full px-3 py-1.5 text-left text-xs hover:bg-accent ${
            item.destructive ? 'text-red-500' : 'text-popover-foreground'
          }`}
          onClick={() => {
            item.action();
            onClose();
          }}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}
