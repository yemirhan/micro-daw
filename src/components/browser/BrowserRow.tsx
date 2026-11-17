import { memo, type DragEvent } from 'react';
import { ChevronRight, FileAudio, Folder, FolderRoot, Volume2, X } from 'lucide-react';
import type { FsDirEntry } from '@/types/electron';
import { cn } from '@/lib/utils';
import { SAMPLE_DRAG_MIME, displayName, type SampleDragPayload } from './browserHelpers';

interface BrowserRowProps {
  entry: FsDirEntry;
  index: number;
  id: string;
  selected: boolean;
  previewing: boolean;
  /** Row is a user root folder (root list view): shows the root icon + remove button. */
  isRoot?: boolean;
  onClick: (index: number) => void;
  onDoubleClick: (index: number) => void;
  onRemoveRoot?: (path: string) => void;
}

function handleDragStart(e: DragEvent<HTMLDivElement>, entry: FsDirEntry) {
  const payload: SampleDragPayload = { path: entry.path, name: displayName(entry.name) };
  e.dataTransfer.setData(SAMPLE_DRAG_MIME, JSON.stringify(payload));
  e.dataTransfer.setData('text/plain', entry.path);
  e.dataTransfer.effectAllowed = 'copy';
}

/** One dense row of the sample browser list (folder or audio file). */
export const BrowserRow = memo(function BrowserRow({
  entry,
  index,
  id,
  selected,
  previewing,
  isRoot,
  onClick,
  onDoubleClick,
  onRemoveRoot,
}: BrowserRowProps) {
  const Icon = isRoot ? FolderRoot : entry.isDir ? Folder : previewing ? Volume2 : FileAudio;
  return (
    <div
      id={id}
      role="option"
      aria-selected={selected}
      data-index={index}
      title={entry.path}
      draggable={!entry.isDir}
      onDragStart={entry.isDir ? undefined : (e) => handleDragStart(e, entry)}
      onClick={() => onClick(index)}
      onDoubleClick={() => onDoubleClick(index)}
      className={cn(
        'group relative flex h-6 cursor-default select-none items-center gap-1.5 rounded-sm pl-2 pr-1 text-[12px] leading-none',
        selected ? 'bg-surface-3 text-foreground' : 'text-foreground/85 hover:bg-surface-2',
        !entry.isDir && 'active:cursor-grabbing',
      )}
    >
      {selected && <span className="absolute inset-y-1 left-0 w-0.5 rounded-full bg-primary" aria-hidden />}
      <Icon
        className={cn(
          'size-3.5 shrink-0',
          entry.isDir ? 'text-muted-foreground' : previewing ? 'text-primary' : 'text-muted-foreground/80',
        )}
        aria-hidden
      />
      <span className="min-w-0 flex-1 truncate">{entry.isDir ? entry.name : displayName(entry.name)}</span>
      {!entry.isDir && (
        <span className="shrink-0 font-mono text-[9.5px] uppercase tracking-wide text-muted-foreground/70">{entry.ext}</span>
      )}
      {isRoot && onRemoveRoot && (
        <button
          type="button"
          aria-label={`Remove ${entry.name} from browser`}
          title="Remove folder from browser"
          onClick={(e) => {
            e.stopPropagation();
            onRemoveRoot(entry.path);
          }}
          className="flex size-4 shrink-0 items-center justify-center rounded-sm text-muted-foreground opacity-0 hover:bg-surface-4 hover:text-foreground focus-visible:opacity-100 group-hover:opacity-100"
        >
          <X className="size-3" />
        </button>
      )}
      {entry.isDir && <ChevronRight className="size-3 shrink-0 text-muted-foreground/60" aria-hidden />}
    </div>
  );
});
