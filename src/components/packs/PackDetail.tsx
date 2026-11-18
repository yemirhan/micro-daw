import { useEffect, useState } from 'react';
import { Download, ExternalLink, Folder, LayoutGrid, Play, RefreshCw, RotateCw, Square, Trash2, TriangleAlert, Volume2, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tip } from '@/components/ui/tip';
import { cn } from '@/lib/utils';
import { Faceplate, usePreviewFlash } from './Faceplate';
import { InstallButton } from './InstallButton';
import {
  formatBytes,
  groupFilesByFolder,
  LICENSE_LABEL,
  LICENSE_URL,
  litPads,
  packFileLabel,
  soundFiles,
  soundKey,
  type InstallButtonState,
  type SoundRef,
  type PackItem,
} from './packsModel';

interface PackDetailProps {
  item: PackItem;
  state: InstallButtonState;
  installed: boolean;
  /** The catalog lists a newer version than the installed one. */
  updateAvailable?: boolean;
  previewing: string | null;
  onClose: () => void;
  onInstall: () => void;
  onCancel: () => void;
  onUninstall: () => void;
  /** Why the last uninstall failed (shown inline; null = no error). */
  uninstallError?: string | null;
  onDismissUninstallError?: () => void;
  /** Demo groove on kit `kitId` (a not-installed pack streams its files first). */
  onPreviewKit: (kitId: string) => void;
  onStopPreview: () => void;
  /** Why the last preview of kit-preview key `${item.id}/${kitId}` failed (null = it didn't). */
  kitPreviewError?: (key: string) => string | null;
  /** Load an installed (or built-in) kit on the pads. */
  onLoadKit: (kitId: string) => void;
  /** Not installed: download, then load kit `kitId` on the pads. */
  onGetAndLoad?: (kitId: string) => void;
  /** Kit-preview key waiting for its download before loading (null = none). */
  pendingLoad?: string | null;
  onPlaySound: (sound: SoundRef) => void;
  /** soundKey() of the sound preview playing right now (null when none). */
  soundPreviewing?: string | null;
  /** Why the last preview of soundKey() `key` failed (null = it didn't). */
  soundError?: (key: string) => string | null;
  /** The open project uses this pack: uninstalling asks with a warning. */
  usedByProject?: boolean;
}

interface KitRow {
  id: string;
  name: string;
  padCount: number;
}

function kitRows(item: PackItem): KitRow[] {
  if (item.kind === 'builtin') return [{ id: item.machine.id, name: `${item.name} kit`, padCount: litPads(item).filter(Boolean).length }];
  return item.manifest.kits.map((k) => ({ id: k.id, name: k.name, padCount: k.pads.length }));
}

function SectionTitle({ children, aside }: { children: React.ReactNode; aside?: React.ReactNode }) {
  return (
    <div className="mb-1.5 flex items-baseline justify-between gap-2 px-1">
      <h3 className="text-[12px] font-semibold text-foreground">{children}</h3>
      {aside && <span className="text-[11px] text-muted-foreground tabular-nums">{aside}</span>}
    </div>
  );
}

/** Right-hand panel for the selected pack: install, kits, sounds, credits. */
export function PackDetail({
  item,
  state,
  installed,
  updateAvailable = false,
  previewing,
  onClose,
  onInstall,
  onCancel,
  onUninstall,
  uninstallError = null,
  onDismissUninstallError,
  onPreviewKit,
  onStopPreview,
  kitPreviewError,
  onLoadKit,
  onGetAndLoad,
  pendingLoad = null,
  onPlaySound,
  soundPreviewing = null,
  soundError,
  usedByProject = false,
}: PackDetailProps) {
  const hits = usePreviewFlash(`${item.id}/`);
  const ready = item.kind === 'builtin' || installed;
  const downloading = state.kind === 'downloading';
  const errorOf = (sound: SoundRef) => soundError?.(soundKey(item.id, sound)) ?? null;
  // First failed sound preview, summarised over the list (each failed row also shows Retry).
  const failedSound = (() => {
    if (!soundError) return null;
    if (item.kind === 'builtin') {
      for (const v of item.machine.voices) {
        const error = errorOf({ kind: 'voice', voice: v.id });
        if (error) return { label: v.label, error };
      }
      return null;
    }
    for (const f of item.manifest.files) {
      const error = errorOf({ kind: 'file', file: f });
      if (error) return { label: packFileLabel(f), error };
    }
    return null;
  })();
  // The "now playing" marker follows the real preview (cleared when the sound ends).
  const isPlaying = (sound: SoundRef) => soundPreviewing !== null && soundPreviewing === soundKey(item.id, sound);
  // Uninstall asks once more ("Confirm uninstall") and falls back after a few seconds.
  const [confirmUninstall, setConfirmUninstall] = useState(false);
  useEffect(() => {
    if (!confirmUninstall) return;
    // A warning needs time to be read.
    const t = setTimeout(() => setConfirmUninstall(false), usedByProject ? 9000 : 4000);
    return () => clearTimeout(t);
  }, [confirmUninstall, usedByProject]);
  const kits = kitRows(item);
  const size = item.kind === 'remote' ? formatBytes(item.sizeBytes) : '';


  return (
    <aside
      aria-label={`${item.name} details`}
      className="flex w-[380px] shrink-0 flex-col border-l border-border bg-surface-1"
    >
      {/* Head */}
      <div className="relative shrink-0 border-b border-border p-4">
        <div
          aria-hidden
          className="pointer-events-none absolute inset-0 opacity-60"
          style={{ background: `radial-gradient(120% 90% at 0% 0%, color-mix(in oklch, ${item.color} 16%, transparent), transparent 60%)` }}
        />
        <Tip label="Close" shortcut="Esc">
          <Button variant="ghost" size="icon-xs" className="absolute top-2.5 right-2.5" onClick={onClose} aria-label="Close details">
            <X />
          </Button>
        </Tip>
        <div className="relative flex gap-4">
          <Faceplate color={item.color} lit={litPads(item)} hits={hits} size="lg" />
          <div className="flex min-w-0 flex-1 flex-col pt-0.5 pr-5">
            <h2 className="text-[17px] font-semibold leading-tight tracking-tight text-foreground">{item.name}</h2>
            <p className="mt-1 text-[12.5px] leading-snug text-muted-foreground">{item.tagline}</p>
            <div className="mt-auto flex flex-wrap items-center gap-1.5 pt-2 text-[11px] text-muted-foreground">
              {item.kind === 'remote' ? (
                <span className="rounded-sm border border-border px-1 py-px text-[10px] font-semibold text-foreground/80">
                  {LICENSE_LABEL[item.manifest.license]}
                </span>
              ) : (
                <span className="rounded-sm border border-primary/30 px-1 py-px text-[10px] font-semibold text-primary">Synth</span>
              )}
              <span className="tabular-nums">{item.soundCount} sounds</span>
              {size && <span className="tabular-nums">{size}</span>}
            </div>
          </div>
        </div>
        {item.kind === 'remote' && (
          <div className="relative mt-3 flex items-center gap-2">
            <InstallButton state={state} name={item.name} onInstall={onInstall} onCancel={onCancel} size="md" />
            {state.kind === 'get' && (
              <span className="text-[11px] text-muted-foreground">Downloads once, then works offline.</span>
            )}
            {state.kind === 'resume' && (
              <span className="text-[11px] text-muted-foreground">Paused: picks up where it stopped.</span>
            )}
            {state.kind === 'error' && (
              <span className="min-w-0 flex-1 truncate text-[11px] text-signal-rec" title={state.message}>
                {state.message || 'Download failed'}
              </span>
            )}
            {installed && updateAvailable && state.kind === 'installed' && (
              <Button variant="outline" size="sm" onClick={onInstall}>
                <RefreshCw aria-hidden />
                Update
              </Button>
            )}
            {installed && uninstallError && (
              <span role="alert" className="min-w-0 flex-1 truncate text-[11px] text-signal-rec" title={uninstallError}>
                Couldn't uninstall: {uninstallError}
              </span>
            )}
            {installed && (
              <Button
                variant="ghost"
                size="sm"
                className={cn('ml-auto', confirmUninstall ? 'text-signal-rec hover:text-signal-rec' : 'text-muted-foreground hover:text-signal-rec')}
                onClick={() => {
                  if (!confirmUninstall) {
                    onDismissUninstallError?.();
                    return setConfirmUninstall(true);
                  }
                  setConfirmUninstall(false);
                  onUninstall();
                }}
              >
                <Trash2 aria-hidden />
                {confirmUninstall ? 'Confirm uninstall' : 'Uninstall'}
              </Button>
            )}
          </div>
        )}
        {item.kind === 'remote' && installed && confirmUninstall && usedByProject && (
          <p role="alert" className="relative mt-2 flex items-start gap-1.5 rounded-md border border-signal-warn/40 bg-signal-warn/10 px-2 py-1.5 text-[11.5px] leading-snug text-foreground/90">
            <TriangleAlert className="mt-px size-3.5 shrink-0 text-signal-warn" aria-hidden />
            <span>Your open project plays sounds from this pack. Its pads go silent until you get the pack again.</span>
          </p>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {/* Kits */}
        <section aria-label="Kits" className="border-b border-border p-3">
          <SectionTitle aside={kits.length > 1 ? `${kits.length} kits` : undefined}>Kits</SectionTitle>
          {kits.length === 0 && <p className="px-1 text-[12px] text-muted-foreground">This pack has no ready-made kit; play its sounds below.</p>}
          <ul className="flex flex-col gap-1">
            {kits.map((k, i) => {
              const key = `${item.id}/${k.id}`;
              const playing = previewing === key;
              const previewError = kitPreviewError?.(key) ?? null;
              const waiting = pendingLoad === key;
              const canGet = item.kind === 'remote' && !installed && !!onGetAndLoad;
              return (
                <li key={k.id} className="rounded-md border border-border bg-surface-2 py-1.5 pr-1.5 pl-2.5">
                  <div className="flex items-center gap-2">
                    <LayoutGrid className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-[12.5px] font-medium text-foreground">{k.name}</div>
                      <div className="text-[10.5px] text-muted-foreground tabular-nums">{k.padCount} pads</div>
                    </div>
                    <Tip label={playing ? 'Stop preview' : ready ? 'Play a short groove on this kit' : 'Play a short groove (streams the sounds, nothing is installed)'}>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={playing ? `Stop preview of ${k.name}` : `Preview ${k.name}`}
                        aria-pressed={playing}
                        onClick={() => (playing ? onStopPreview() : onPreviewKit(k.id))}
                      >
                        {playing ? <Square className="size-3.5 fill-current" /> : <Play className="size-3.5 fill-current" />}
                      </Button>
                    </Tip>
                    {ready ? (
                      <Tip label="Replace the pads of the current drum track">
                        <Button size="sm" variant={i === 0 ? 'default' : 'outline'} onClick={() => onLoadKit(k.id)}>
                          Load on pads
                        </Button>
                      </Tip>
                    ) : canGet ? (
                      <Tip label={waiting ? 'Loads on the pads when the download finishes' : `Download ${item.name}, then load this kit on the pads`}>
                        <span>
                          <Button
                            size="sm"
                            variant={i === 0 ? 'default' : 'outline'}
                            disabled={downloading}
                            aria-label={waiting ? `Getting ${item.name}, then loading ${k.name}` : `Get ${item.name} and load ${k.name}`}
                            onClick={() => onGetAndLoad?.(k.id)}
                          >
                            <Download aria-hidden />
                            {waiting ? 'Getting…' : 'Get & load'}
                          </Button>
                        </span>
                      </Tip>
                    ) : null}
                  </div>
                  {previewError && (
                    <div role="alert" className="mt-1 flex items-center gap-1.5 pl-[22px] text-[11px] text-signal-rec">
                      <span className="min-w-0 flex-1 truncate" title={previewError}>
                        {playing ? previewError : `Preview failed: ${previewError}`}
                      </span>
                      <Button variant="ghost" size="xs" className="text-signal-rec hover:text-signal-rec" onClick={() => onPreviewKit(k.id)}>
                        <RotateCw aria-hidden />
                        Retry
                      </Button>
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        </section>

        {/* Sounds */}
        <section aria-label="Sounds" className="border-b border-border p-3">
          <SectionTitle aside={`${item.soundCount}`}>Sounds</SectionTitle>
          {failedSound && (
            <p role="alert" className="mb-1.5 flex items-start gap-1.5 rounded-sm bg-signal-rec/10 px-1.5 py-1 text-[11px] leading-snug text-signal-rec">
              <TriangleAlert className="mt-px size-3 shrink-0" aria-hidden />
              <span>
                Couldn't play {failedSound.label}: {failedSound.error}. Click it to retry.
              </span>
            </p>
          )}
          {item.kind === 'builtin' ? (
            <SoundList
              rows={item.machine.voices.map((v) => ({ key: v.id, label: v.label, sound: { kind: 'voice' as const, voice: v.id } }))}
              isPlaying={isPlaying}
              errorOf={errorOf}
              onPlay={onPlaySound}
            />
          ) : (
            groupFilesByFolder(soundFiles(item.manifest.files)).map((g) => (
              <div key={g.folder || '.'} className="mb-1.5 last:mb-0">
                {g.folder && (
                  <div className="flex items-center gap-1.5 px-1 pt-1 pb-0.5 text-[11px] font-medium text-muted-foreground">
                    <Folder className="size-3" aria-hidden />
                    <span className="truncate">{g.folder}</span>
                    <span className="ml-auto tabular-nums">{g.files.length}</span>
                  </div>
                )}
                <SoundList
                  rows={g.files.map((f) => ({ key: f.path, label: packFileLabel(f), sound: { kind: 'file' as const, file: f } }))}
                  isPlaying={isPlaying}
                  errorOf={errorOf}
                  onPlay={onPlaySound}
                />
              </div>
            ))
          )}
          {item.kind === 'remote' && !installed && (
            <p className="mt-2 px-1 text-[11px] text-muted-foreground">Previews stream from the source before you download.</p>
          )}
        </section>

        {/* About */}
        <section aria-label="About" className="p-3 pb-5">
          <SectionTitle>About</SectionTitle>
          <p className="px-1 text-[12.5px] leading-relaxed text-foreground/85">{item.description}</p>
          {item.kind === 'remote' ? (
            <dl className="mt-3 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 px-1 text-[12px]">
              <dt className="text-muted-foreground">Sounds by</dt>
              <dd className="min-w-0">
                <a
                  href={item.manifest.source}
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex max-w-full items-center gap-1 text-foreground underline-offset-2 hover:underline"
                >
                  <span className="truncate">{item.manifest.author}</span>
                  <ExternalLink className="size-3 shrink-0 text-muted-foreground" aria-hidden />
                </a>
              </dd>
              <dt className="text-muted-foreground">Licence</dt>
              <dd>
                {LICENSE_URL[item.manifest.license] ? (
                  <a
                    href={LICENSE_URL[item.manifest.license]!}
                    target="_blank"
                    rel="noreferrer"
                    className="inline-flex items-center gap-1 text-foreground underline-offset-2 hover:underline"
                  >
                    {LICENSE_LABEL[item.manifest.license]}
                    <ExternalLink className="size-3 text-muted-foreground" aria-hidden />
                  </a>
                ) : (
                  <span className="text-foreground">{LICENSE_LABEL[item.manifest.license]}</span>
                )}
              </dd>
              {item.manifest.attribution && (
                <>
                  <dt className="text-muted-foreground">Credit</dt>
                  <dd className="text-foreground/85">{item.manifest.attribution}</dd>
                </>
              )}
            </dl>
          ) : (
            <p className="mt-2 px-1 text-[11.5px] leading-relaxed text-muted-foreground">
              Inspired by the machine's sound; not affiliated with or endorsed by its maker.
            </p>
          )}
        </section>
      </div>
    </aside>
  );
}

interface SoundRow {
  key: string;
  label: string;
  sound: SoundRef;
}

function SoundList({
  rows,
  isPlaying,
  errorOf,
  onPlay,
}: {
  rows: SoundRow[];
  isPlaying: (sound: SoundRef) => boolean;
  errorOf: (sound: SoundRef) => string | null;
  onPlay: (sound: SoundRef) => void;
}) {
  return (
    <ul className="grid grid-cols-2 gap-0.5">
      {rows.map((r) => {
        const playing = isPlaying(r.sound);
        const error = errorOf(r.sound);
        return (
          <li key={r.key} className="min-w-0">
            <button
              type="button"
              onClick={() => onPlay(r.sound)}
              data-playing={playing || undefined}
              data-error={error ? true : undefined}
              title={error ? `Couldn't play ${r.label}: ${error}. Click to retry.` : r.label === r.key ? r.key : `${r.label} (${r.key})`}
              aria-label={error ? `Retry ${r.label} (preview failed: ${error})` : `Play ${r.label}`}
              className={cn(
                'group/sound flex w-full items-center gap-1.5 rounded-sm px-1.5 py-1 text-left text-[12px] outline-none',
                'hover:bg-surface-3 focus-visible:ring-2 focus-visible:ring-ring/60',
                error ? 'bg-signal-rec/10 text-foreground' : playing ? 'text-foreground' : 'text-foreground/80',
              )}
            >
              {error ? (
                <TriangleAlert className="size-3 shrink-0 text-signal-rec" aria-hidden />
              ) : playing ? (
                <Volume2 className="size-3 shrink-0 text-primary" aria-hidden />
              ) : (
                <Play className="size-3 shrink-0 fill-current text-muted-foreground group-hover/sound:text-foreground" aria-hidden />
              )}
              <span className="min-w-0 flex-1 truncate">{r.label}</span>
              {error && (
                <span aria-hidden className="inline-flex shrink-0 items-center gap-0.5 text-[10.5px] font-medium text-signal-rec">
                  <RotateCw className="size-2.5" />
                  Retry
                </span>
              )}
            </button>
          </li>
        );
      })}
    </ul>
  );
}
