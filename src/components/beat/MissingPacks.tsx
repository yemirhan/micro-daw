import { memo } from 'react';
import { Eraser, PackageX, Replace } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tip } from '@/components/ui/tip';
import { cn } from '@/lib/utils';
import { InstallButton } from '@/components/packs/InstallButton';
import { isPausedInstall, packButtonState } from '@/components/packs/packsModel';
import { packService } from '@/services/PackService';
import { padLabel } from './beatHelpers';
import type { MissingPadFix } from './missingPackActions';
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

/** Clear / replace for the pads of a pack that can't be downloaded any more. */
export function UnavailablePackActions({ pack, onFix }: { pack: MissingPack; onFix: (pack: MissingPack, fix: MissingPadFix) => void }) {
  const pads = pack.pads.length === 1 ? 'this pad' : `these ${pack.pads.length} pads`;
  return (
    <span className="flex shrink-0 items-center gap-1">
      <Tip label={`Empty ${pads}`}>
        <Button size="xs" variant="ghost" onClick={() => onFix(pack, 'clear')} aria-label={`Clear the pads that use ${pack.name}`}>
          <Eraser aria-hidden />
          Clear pads
        </Button>
      </Tip>
      <Tip label={`Put the built-in kit's sounds on ${pads}`}>
        <Button size="xs" variant="outline" onClick={() => onFix(pack, 'builtin')} aria-label={`Replace the pads that use ${pack.name} with built-in sounds`}>
          <Replace aria-hidden />
          Replace with built-in
        </Button>
      </Tip>
    </span>
  );
}

const padCount = (n: number) => (n === 1 ? '1 pad' : `${n} pads`);

/**
 * Over the pads: packs the kit uses that aren't installed (an old project, a
 * pack uninstalled since, another machine), each with a one-click Get, or,
 * for a pack the catalog no longer lists, clear / replace. Text wraps instead
 * of truncating the pack name or the error (review L-d).
 */
export const MissingPacksStrip = memo(function MissingPacksStrip({
  missing,
  onGet,
  onFix,
  className,
}: {
  missing: MissingPack[];
  onGet: (id: string) => void;
  /** Clear / replace the pads of an unavailable pack (no actions shown when omitted). */
  onFix?: (pack: MissingPack, fix: MissingPadFix) => void;
  className?: string;
}) {
  if (missing.length === 0) return null;
  return (
    <div role="status" aria-label="Packs not installed" className={cn('flex flex-col gap-1', className)}>
      {missing.map((m) => {
        const error = m.available && m.state.state === 'error' && !isPausedInstall(m.state) ? m.state.message || 'Download failed' : null;
        return (
          <div
            key={m.id}
            data-missing-pack={m.id}
            data-unavailable={m.available ? undefined : true}
            className="flex flex-wrap items-center gap-x-2 gap-y-0.5 rounded-md border border-signal-warn/40 bg-signal-warn/10 py-0.5 pr-0.5 pl-2 text-[11.5px] leading-[1.3]"
          >
            <PackageX className="size-3.5 shrink-0 text-signal-warn" aria-hidden />
            <span
              className="min-w-[120px] flex-1 py-0.5 break-words text-foreground/90"
              title={`Pads ${m.pads.map((p) => padLabel(p)).join(', ')} play sounds from ${m.name}`}
            >
              {m.available ? 'Not installed: ' : 'No longer available: '}
              <span className="font-semibold">{m.name}</span>
              <span className="whitespace-nowrap text-muted-foreground"> · {padCount(m.pads.length)}</span>
            </span>
            <span className="ml-auto flex shrink-0 items-center">
              {m.available ? <GetPackButton pack={m} onGet={onGet} /> : onFix && <UnavailablePackActions pack={m} onFix={onFix} />}
            </span>
            {error && (
              <span role="alert" className="basis-full pb-0.5 pl-[22px] text-[11px] break-words text-signal-rec">
                {error}
              </span>
            )}
          </div>
        );
      })}
    </div>
  );
});
