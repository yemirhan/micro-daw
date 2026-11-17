import { PackageX } from 'lucide-react';
import { cn } from '@/lib/utils';
import { InstallButton } from '@/components/packs/InstallButton';
import { packButtonState } from '@/components/packs/packsModel';
import { packService } from '@/services/PackService';
import { padLabel } from './beatHelpers';
import type { MissingPack } from './usePackAvailability';

/** Get / progress + cancel / retry for a pack some pads need. */
export function GetPackButton({ pack, onGet, size = 'sm' }: { pack: MissingPack; onGet: (id: string) => void; size?: 'sm' | 'md' }) {
  return (
    <InstallButton
      state={packButtonState(pack.state, false)}
      name={pack.name}
      size={size}
      onInstall={() => onGet(pack.id)}
      onCancel={() => void packService.cancel(pack.id)}
    />
  );
}

/**
 * Over the pads: packs the kit uses that aren't installed (an old project, a
 * pack uninstalled since, another machine), each with a one-click Get.
 */
export function MissingPacksStrip({ missing, onGet, className }: { missing: MissingPack[]; onGet: (id: string) => void; className?: string }) {
  if (missing.length === 0) return null;
  return (
    <div role="status" aria-label="Packs not installed" className={cn('flex flex-col gap-1', className)}>
      {missing.map((m) => (
        <div key={m.id} data-missing-pack={m.id} className="flex items-center gap-2 rounded-md border border-signal-warn/40 bg-signal-warn/10 py-0.5 pr-0.5 pl-2 text-[11.5px]">
          <PackageX className="size-3.5 shrink-0 text-signal-warn" aria-hidden />
          <span className="min-w-0 flex-1 truncate text-foreground/90" title={`Pads ${m.pads.map((p) => padLabel(p)).join(', ')} play sounds from ${m.name}`}>
            Pack not installed: <span className="font-semibold">{m.name}</span>
            <span className="text-muted-foreground"> · {m.pads.length === 1 ? '1 pad' : `${m.pads.length} pads`}</span>
          </span>
          {m.state.state === 'error' && (
            <span className="max-w-[120px] truncate text-[11px] text-signal-rec" title={m.state.message}>
              {m.state.message || 'Download failed'}
            </span>
          )}
          <GetPackButton pack={m} onGet={onGet} />
        </div>
      ))}
    </div>
  );
}
