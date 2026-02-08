import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { Slider } from '@/components/ui/slider';
import { Label } from '@/components/ui/label';
import { Separator } from '@/components/ui/separator';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { RefreshCw, Download, RotateCcw, ExternalLink, Loader2, Play } from 'lucide-react';
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
      return <span className="flex items-center gap-1.5 text-xs text-muted-foreground"><Loader2 className="h-3 w-3 animate-spin" />Checking for updates...</span>;
    case 'available':
      return <span className="text-xs text-green-400">Version {status.version} available</span>;
    case 'not-available':
      return <span className="text-xs text-muted-foreground">You're up to date</span>;
    case 'downloading':
      return (
        <div className="flex flex-col gap-1">
          <span className="text-xs text-muted-foreground">Downloading... {Math.round(status.percent)}%</span>
          <div className="h-1.5 w-48 rounded-full bg-muted overflow-hidden">
            <div className="h-full rounded-full bg-primary transition-all" style={{ width: `${status.percent}%` }} />
          </div>
        </div>
      );
    case 'downloaded':
      return <span className="text-xs text-green-400">Version {status.version} ready to install</span>;
    case 'error':
      return <span className="text-xs text-destructive">{status.message}</span>;
  }
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
  const [confirmResetLessons, setConfirmResetLessons] = useState(false);
  const [confirmResetPractice, setConfirmResetPractice] = useState(false);

  return (
    <div className="flex-1 overflow-y-auto">
      <div className="mx-auto max-w-2xl px-6 py-8 space-y-8">
        <h1 className="text-2xl font-semibold">Settings</h1>

        {/* General */}
        <section className="space-y-4">
          <h2 className="text-sm font-medium uppercase tracking-wider text-muted-foreground">General</h2>
          <Separator />

          <div className="flex items-center justify-between">
            <Label htmlFor="auto-check-updates">Check for updates automatically</Label>
            <Switch
              id="auto-check-updates"
              checked={settings.general.autoCheckUpdates}
              onCheckedChange={(checked) =>
                onUpdateSettings({ general: { autoCheckUpdates: checked } })
              }
            />
          </div>

          <div className="flex items-center gap-3">
            <Button
              variant="outline"
              size="sm"
              onClick={onCheckForUpdates}
              disabled={updateStatus.state === 'checking' || updateStatus.state === 'downloading'}
            >
              <RefreshCw className="mr-1.5 h-3.5 w-3.5" />
              Check Now
            </Button>

            {updateStatus.state === 'available' && (
              <Button size="sm" onClick={onDownloadUpdate}>
                <Download className="mr-1.5 h-3.5 w-3.5" />
                Download &amp; Install
              </Button>
            )}

            {updateStatus.state === 'downloaded' && (
              <Button size="sm" onClick={onInstallUpdate}>
                <RotateCcw className="mr-1.5 h-3.5 w-3.5" />
                Restart to Update
              </Button>
            )}
          </div>
          <UpdateStatusText status={updateStatus} />

          <div className="flex items-center justify-between pt-2">
            <div>
              <Label>Onboarding Tour</Label>
              <p className="text-xs text-muted-foreground mt-0.5">Replay the interactive walkthrough</p>
            </div>
            <Button variant="outline" size="sm" onClick={onReplayTour}>
              <Play className="mr-1.5 h-3.5 w-3.5" />
              Replay Tour
            </Button>
          </div>
        </section>

        {/* Audio */}
        <section className="space-y-4">
          <h2 className="text-sm font-medium uppercase tracking-wider text-muted-foreground">Audio</h2>
          <Separator />

          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label>Default Master Volume</Label>
              <span className="text-xs text-muted-foreground tabular-nums w-12 text-right">
                {settings.audio.defaultMasterVolume} dB
              </span>
            </div>
            <Slider
              min={-40}
              max={0}
              step={1}
              value={[settings.audio.defaultMasterVolume]}
              onValueChange={([v]) =>
                onUpdateSettings({ audio: { ...settings.audio, defaultMasterVolume: v } })
              }
            />
          </div>

          <div className="flex items-center justify-between">
            <Label>Buffer Size</Label>
            <Select
              value={String(settings.audio.bufferSizeHint)}
              onValueChange={(v) =>
                onUpdateSettings({
                  audio: { ...settings.audio, bufferSizeHint: Number(v) as AppSettings['audio']['bufferSizeHint'] },
                })
              }
            >
              <SelectTrigger className="w-28">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {[128, 256, 512, 1024, 2048].map((size) => (
                  <SelectItem key={size} value={String(size)}>
                    {size}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </section>

        {/* MIDI */}
        <section className="space-y-4">
          <h2 className="text-sm font-medium uppercase tracking-wider text-muted-foreground">MIDI</h2>
          <Separator />

          <div className="flex items-center justify-between">
            <Label htmlFor="auto-connect-midi">Auto-connect last device</Label>
            <Switch
              id="auto-connect-midi"
              checked={settings.midi.autoConnectLastDevice}
              onCheckedChange={(checked) =>
                onUpdateSettings({ midi: { autoConnectLastDevice: checked } })
              }
            />
          </div>
        </section>

        {/* Data */}
        <section className="space-y-4">
          <h2 className="text-sm font-medium uppercase tracking-wider text-muted-foreground">Data</h2>
          <Separator />

          <div className="flex items-center justify-between">
            <div>
              <Label>Reset Lesson Progress</Label>
              <p className="text-xs text-muted-foreground mt-0.5">Clears all completed lessons and step progress</p>
            </div>
            {confirmResetLessons ? (
              <div className="flex gap-2">
                <Button variant="destructive" size="sm" onClick={() => { onResetLessonProgress(); setConfirmResetLessons(false); }}>
                  Confirm
                </Button>
                <Button variant="outline" size="sm" onClick={() => setConfirmResetLessons(false)}>
                  Cancel
                </Button>
              </div>
            ) : (
              <Button variant="outline" size="sm" onClick={() => setConfirmResetLessons(true)}>
                Reset
              </Button>
            )}
          </div>

          <div className="flex items-center justify-between">
            <div>
              <Label>Reset Practice Progress</Label>
              <p className="text-xs text-muted-foreground mt-0.5">Clears all practice scores and attempts</p>
            </div>
            {confirmResetPractice ? (
              <div className="flex gap-2">
                <Button variant="destructive" size="sm" onClick={() => { onResetPracticeProgress(); setConfirmResetPractice(false); }}>
                  Confirm
                </Button>
                <Button variant="outline" size="sm" onClick={() => setConfirmResetPractice(false)}>
                  Cancel
                </Button>
              </div>
            ) : (
              <Button variant="outline" size="sm" onClick={() => setConfirmResetPractice(true)}>
                Reset
              </Button>
            )}
          </div>
        </section>

        {/* About */}
        <section className="space-y-4">
          <h2 className="text-sm font-medium uppercase tracking-wider text-muted-foreground">About</h2>
          <Separator />

          <div className="space-y-2 text-sm">
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground">Version</span>
              <span className="font-mono">{appVersion || 'dev'}</span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground">Author</span>
              <span>Yusuf Emirhan Sahin</span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground">Source</span>
              <a
                href="https://github.com/yusufemirhansakin/micro-daw"
                target="_blank"
                rel="noopener noreferrer"
                className="flex items-center gap-1 text-primary hover:underline"
              >
                GitHub <ExternalLink className="h-3 w-3" />
              </a>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}
