import { memo } from 'react';
import { X } from 'lucide-react';
import { Tip } from '@/components/ui/tip';
import { cn } from '@/lib/utils';
import { usePacks } from '@/hooks/usePacks';
import { packService, type PendingLoadInfo } from '@/services/PackService';
import type { PackInstallState } from '@/types/packs';
import { ProgressRing } from '@/components/packs/InstallButton';
import { downloadProgress } from '@/components/packs/packsModel';

/**
 * Pure: chip text + tooltip for a pending "Get & load". `prefix` is dropped
 * when the chip is narrow (the icon still says downloading / recording).
 */
export function pendingLoadText(info: PendingLoadInfo): { prefix: string; name: string; text: string; title: string } {
  if (info.waitingForRecording) {
    const prefix = 'After take: ';
    return { prefix, name: info.packName, text: `${prefix}${info.packName}`, title: `${info.packName} is downloaded: its kit loads on these pads when recording stops` };
  }
  const prefix = 'Getting ';
  return {
    prefix,
    name: `${info.packName}…`,
    text: `${prefix}${info.packName}…`,
    title: `Getting ${info.packName}: its kit loads on these pads when the download is done (choosing another kit cancels it)`,
  };
}

/**
 * N2: a "Get & load" waiting for its pack (or for the take to end), shown in
 * the Beat pads header: "Getting <pack>… ✕". ✕ stops it (and its download
 * while one runs). Renders nothing when no load is pending.
 */
export const PendingLoadChip = memo(function PendingLoadChip({ className }: { className?: string }) {
  const snap = usePacks();
  const info = snap.pendingLoadInfo;
  if (!info) return null;
  return <PendingLoadChipView info={info} install={snap.installStates[info.packId]} className={className} />;
});

export function PendingLoadChipView({ info, install, className }: { info: PendingLoadInfo; install?: PackInstallState; className?: string }) {
  const { prefix, name, title } = pendingLoadText(info);
  const progress = install?.state === 'downloading' ? downloadProgress(install) : null;
  const cancel = () => {
    if (install?.state === 'downloading') void packService.cancel(info.packId);
    else packService.cancelPendingLoad();
  };
  return (
    <span
      role="status"
      data-pending-load={info.key}
      title={title}
      className={cn(
        '@container/chip flex h-6 min-w-0 items-center gap-1 rounded-md border border-primary/40 bg-primary/10 pr-0.5 pl-1 text-[11px] leading-none text-foreground',
        className,
      )}
    >
      {info.waitingForRecording ? (
        <span aria-hidden className="size-2 shrink-0 animate-rec-pulse rounded-full bg-signal-rec motion-reduce:animate-none" />
      ) : (
        <ProgressRing progress={progress} size={13} label={`Downloading ${info.packName}`} />
      )}
      <span className="min-w-0 flex-1 truncate">
        <span className="hidden @[176px]/chip:inline">{prefix}</span>
        {name}
      </span>
      <Tip label={info.waitingForRecording ? "Don't load this kit" : 'Stop getting this kit'}>
        <button
          type="button"
          aria-label={`Cancel loading ${info.packName}`}
          onClick={cancel}
          className="flex size-5 shrink-0 items-center justify-center rounded-[4px] text-muted-foreground outline-none hover:bg-surface-3 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/70"
        >
          <X className="size-3" aria-hidden />
        </button>
      </Tip>
    </span>
  );
}
