import { useState, type ReactNode } from 'react';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { Slider } from '@/components/ui/slider';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  CheckCircle2,
  CircleAlert,
  Database,
  Download,
  ExternalLink,
  Info,
  Loader2,
  Play,
  RefreshCw,
  RotateCcw,
  Settings2,
  SlidersHorizontal,
  Usb,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { ViewHeader } from '@/components/learn/kit';
import type { AppSettings, UpdateStatus } from '@/types/settings';

interface SettingsViewProps {
  settings: AppSettings;
  onUpdateSettings: (partial: Partial<AppSettings>) => void;
  onResetLessonProgress: () => void;
  onResetPracticeProgress: () => void;
  updateStatus: UpdateStatus;
  onCheckForUpdates: () => void;
  onDownloadUpdate: () => void;
  onInstallUpdate: () => void;
  appVersion: string;
  onReplayTour: () => void;
}

function UpdateStatusText({ status }: { status: UpdateStatus }) {
  switch (status.state) {
    case 'idle':
      return null;
    case 'checking':
      return (
        <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <Loader2 className="size-3.5 animate-spin" />
          Checking for updates…
        </span>
      );
    case 'available':
      return (
        <span className="flex items-center gap-1.5 text-xs font-medium text-primary">
          <Download className="size-3.5" />
          Version {status.version} is available
        </span>
      );
    case 'not-available':
      return (
        <span className="flex items-center gap-1.5 text-xs text-signal-play">
          <CheckCircle2 className="size-3.5" />
          You're up to date
        </span>
      );
    case 'downloading':
      return (
        <div className="flex items-center gap-2">
          <span className="font-mono text-xs tabular-nums text-muted-foreground">Downloading {Math.round(status.percent)}%</span>
          <div className="h-1.5 w-40 overflow-hidden rounded-full bg-surface-3">
            <div className="h-full rounded-full bg-primary transition-[width]" style={{ width: `${status.percent}%` }} />
          </div>
        </div>
      );
    case 'downloaded':
      return (
        <span className="flex items-center gap-1.5 text-xs font-medium text-signal-play">
          <CheckCircle2 className="size-3.5" />
          Version {status.version} is ready to install
        </span>
      );
    case 'error':
      return (
        <span className="flex items-center gap-1.5 text-xs text-destructive">
          <CircleAlert className="size-3.5" />
          {status.message}
        </span>
      );
  }
}

/** A titled group of setting rows. */
function Section({ icon: Icon, title, children, tone }: { icon: typeof Info; title: string; children: ReactNode; tone?: 'danger' }) {
  return (
    <section aria-label={title}>
      <h2 className="mb-2 flex items-center gap-2 px-1 text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">
        <Icon className="size-3.5" aria-hidden />
        {title}
      </h2>
      <div
        className={cn(
          'divide-y divide-border overflow-hidden rounded-xl border bg-surface-1 shadow-[inset_0_1px_0_var(--hairline)]',
          tone === 'danger' ? 'border-destructive/25' : 'border-border',
        )}
      >
        {children}
      </div>
    </section>
  );
}

/** One setting: label + description on the left, control on the right. */
function Row({ label, description, htmlFor, children, below }: { label: string; description?: ReactNode; htmlFor?: string; children?: ReactNode; below?: ReactNode }) {
  return (
    <div className="px-4 py-3">
      <div className="flex min-h-9 items-center justify-between gap-6">
        <div className="min-w-0">
          <label htmlFor={htmlFor} className="block text-[13px] font-medium text-foreground">
            {label}
          </label>
          {description && <p className="mt-0.5 text-xs text-muted-foreground">{description}</p>}
        </div>
        {children && <div className="flex shrink-0 items-center gap-2">{children}</div>}
      </div>
      {below && <div className="mt-2.5">{below}</div>}
    </div>
  );
}

function ConfirmReset({ onConfirm, label }: { onConfirm: () => void; label: string }) {
  const [confirming, setConfirming] = useState(false);
  if (!confirming) {
    return (
      <Button variant="outline" size="sm" onClick={() => setConfirming(true)}>
        <RotateCcw className="size-3.5" />
        Reset
      </Button>
    );
  }
  return (
    <div className="flex items-center gap-2 animate-in fade-in-0 duration-150">
      <span className="text-xs text-muted-foreground">Erase {label}?</span>
      <Button variant="ghost" size="sm" onClick={() => setConfirming(false)}>
        Cancel
      </Button>
      <Button
        variant="destructive"
        size="sm"
        onClick={() => {
          onConfirm();
          setConfirming(false);
        }}
      >
        Erase
      </Button>
    </div>
  );
}

export function SettingsView({
  settings,
  onUpdateSettings,
  onResetLessonProgress,
  onResetPracticeProgress,
  updateStatus,
  onCheckForUpdates,
  onDownloadUpdate,
  onInstallUpdate,
  appVersion,
  onReplayTour,
}: SettingsViewProps) {
  return (
    <div className="flex flex-1 flex-col overflow-hidden bg-background">
      <ViewHeader icon={Settings2} title="Settings" subtitle="Preferences are saved on this computer." />

      <div className="flex-1 overflow-y-auto px-8 py-6">
        <div className="mx-auto flex max-w-2xl flex-col gap-7 pb-8">
          <Section icon={Settings2} title="General">
            <Row
              label="Check for updates automatically"
              description="Looks for a new version when the app starts."
              htmlFor="auto-check-updates"
            >
              <Switch
                id="auto-check-updates"
                checked={settings.general.autoCheckUpdates}
                onCheckedChange={(checked) =>
                  onUpdateSettings({ general: { ...settings.general, autoCheckUpdates: checked } })
                }
              />
            </Row>
            <Row
              label="Software update"
              description={`Installed: ${appVersion || 'dev'}`}
              below={updateStatus.state !== 'idle' ? <UpdateStatusText status={updateStatus} /> : undefined}
            >
              {updateStatus.state === 'available' && (
                <Button size="sm" onClick={onDownloadUpdate}>
                  <Download className="size-3.5" />
                  Download &amp; install
                </Button>
              )}
              {updateStatus.state === 'downloaded' && (
                <Button size="sm" onClick={onInstallUpdate}>
                  <RotateCcw className="size-3.5" />
                  Restart to update
                </Button>
              )}
              <Button
                variant="outline"
                size="sm"
                onClick={onCheckForUpdates}
                disabled={updateStatus.state === 'checking' || updateStatus.state === 'downloading'}
              >
                <RefreshCw className={cn('size-3.5', updateStatus.state === 'checking' && 'animate-spin')} />
                Check now
              </Button>
            </Row>
            <Row label="Onboarding tour" description="Replay the interactive walkthrough of the app.">
              <Button variant="outline" size="sm" onClick={onReplayTour}>
                <Play className="size-3.5" />
                Replay tour
              </Button>
            </Row>
          </Section>

          <Section icon={SlidersHorizontal} title="Audio">
            <Row
              label="Default master volume"
              description="Used for new projects."
              below={
                <div className="flex items-center gap-3">
                  <Slider
                    min={-40}
                    max={0}
                    step={1}
                    value={[settings.audio.defaultMasterVolume]}
                    aria-label="Default master volume"
                    onValueChange={([v]) => onUpdateSettings({ audio: { ...settings.audio, defaultMasterVolume: v } })}
                    className="flex-1"
                  />
                  <span className="daw-lcd flex h-7 w-20 items-center justify-end px-2 text-xs text-foreground">
                    {settings.audio.defaultMasterVolume} dB
                  </span>
                </div>
              }
            />
            <Row label="Buffer size" description="Smaller is more responsive; larger is safer on slow machines.">
              <Select
                value={String(settings.audio.bufferSizeHint)}
                onValueChange={(v) =>
                  onUpdateSettings({
                    audio: { ...settings.audio, bufferSizeHint: Number(v) as AppSettings['audio']['bufferSizeHint'] },
                  })
                }
              >
                <SelectTrigger className="h-8 w-32 font-mono text-xs" aria-label="Buffer size">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {[128, 256, 512, 1024, 2048].map((size) => (
                    <SelectItem key={size} value={String(size)} className="font-mono text-xs">
                      {size} samples
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Row>
          </Section>

          <Section icon={Usb} title="MIDI">
            <Row
              label="Auto-connect last device"
              description="Reconnect to the keyboard you used last time."
              htmlFor="auto-connect-midi"
            >
              <Switch
                id="auto-connect-midi"
                checked={settings.midi.autoConnectLastDevice}
                onCheckedChange={(checked) => onUpdateSettings({ midi: { autoConnectLastDevice: checked } })}
              />
            </Row>
          </Section>

          <Section icon={Database} title="Data" tone="danger">
            <Row label="Reset lesson progress" description="Clears completed lessons and step progress.">
              <ConfirmReset onConfirm={onResetLessonProgress} label="lesson progress" />
            </Row>
            <Row label="Reset practice progress" description="Clears practice scores and attempts.">
              <ConfirmReset onConfirm={onResetPracticeProgress} label="practice progress" />
            </Row>
          </Section>

          <Section icon={Info} title="About">
            <Row label="Version">
              <span className="font-mono text-xs text-foreground">{appVersion || 'dev'}</span>
            </Row>
            <Row label="Author">
              <span className="text-xs text-foreground">Yusuf Emirhan Sahin</span>
            </Row>
            <Row label="Source code">
              <a
                href="https://github.com/yemirhan/micro-daw"
                target="_blank"
                rel="noopener noreferrer"
                className="flex items-center gap-1 rounded text-xs font-medium text-primary outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring/60"
              >
                GitHub <ExternalLink className="size-3" />
              </a>
            </Row>
            <Row
              label="Credits"
              description={
                <>
                  Piano samples: Salamander Grand Piano V3 by Alexander Holm, licensed under{' '}
                  <a
                    href="https://creativecommons.org/licenses/by/3.0/"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="rounded font-medium text-primary outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring/60"
                  >
                    CC BY 3.0
                  </a>
                  .
                </>
              }
            />
          </Section>
        </div>
      </div>
    </div>
  );
}
