import { cn } from '@/lib/utils';
import { Faceplate, usePreviewFlash } from './Faceplate';
import { InstallButton } from './InstallButton';
import { formatBytes, LICENSE_LABEL, litPads, type InstallButtonState, type PackItem } from './packsModel';

interface PackCardProps {
  item: PackItem;
  state: InstallButtonState;
  selected: boolean;
  onSelect: () => void;
  onInstall: () => void;
  onCancel: () => void;
}

export function PackCard({ item, state, selected, onSelect, onInstall, onCancel }: PackCardProps) {
  const hits = usePreviewFlash(`${item.id}/`);
  // Size helps decide on a download; once installed / downloading it only crowds the row.
  const size = item.kind === 'remote' && state.kind === 'get' ? formatBytes(item.sizeBytes) : '';
  return (
    <article
      data-pack-id={item.id}
      className={cn(
        'group/card relative flex gap-3 rounded-lg border bg-surface-1 p-2.5 pb-2 transition-colors duration-150',
        selected ? 'border-transparent bg-surface-2' : 'border-border hover:border-hairline-strong hover:bg-surface-2',
      )}
      style={selected ? { boxShadow: `0 0 0 1.5px ${item.color}, 0 0 0 4px color-mix(in oklch, ${item.color} 18%, transparent)` } : undefined}
    >
      {/* Whole card selects; the install control sits above this button. */}
      <button
        type="button"
        onClick={onSelect}
        aria-pressed={selected}
        aria-label={`${item.name}: ${item.tagline}`}
        className="absolute inset-0 rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-ring/70"
      />
      <Faceplate color={item.color} lit={litPads(item)} hits={hits} />
      <div className="pointer-events-none flex min-w-0 flex-1 flex-col">
        <h3 className="truncate pt-0.5 text-[13.5px] font-semibold leading-tight text-foreground">{item.name}</h3>
        <p className="mt-0.5 line-clamp-2 text-[12px] leading-snug text-muted-foreground">{item.tagline}</p>
        <div className="mt-auto flex min-h-7 items-center gap-x-2 pt-1.5 text-[11px] text-muted-foreground">
          {item.kind === 'remote' ? (
            <span
              className="shrink-0 whitespace-nowrap rounded-sm border border-border px-1 py-px text-[10px] font-semibold tracking-wide text-foreground/80"
            >
              {/* Short badge on the card so the size fits next to the install button; the panel spells it out. */}
              {item.manifest.license === 'public-domain' ? (
                <>
                  <span aria-hidden>PD</span>
                  <span className="sr-only">{LICENSE_LABEL[item.manifest.license]}</span>
                </>
              ) : (
                LICENSE_LABEL[item.manifest.license]
              )}
            </span>
          ) : (
            <span className="shrink-0 rounded-sm border border-primary/30 px-1 py-px text-[10px] font-semibold tracking-wide text-primary">
              Synth
            </span>
          )}
          <span className="min-w-0 truncate whitespace-nowrap tabular-nums" title={size ? `${item.soundCount} sounds, ${size}` : undefined}>
            {item.soundCount} sounds{size && <span className="text-muted-foreground/70">, {size}</span>}
          </span>
          <div className="pointer-events-auto relative z-10 ml-auto shrink-0 -mr-0.5 -mb-0.5">
            <InstallButton state={state} name={item.name} onInstall={onInstall} onCancel={onCancel} />
          </div>
        </div>
      </div>
    </article>
  );
}
