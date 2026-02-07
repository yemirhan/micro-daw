import { useMemo } from 'react';
import {
  CommandDialog,
  CommandInput,
  CommandList,
  CommandEmpty,
  CommandGroup,
  CommandItem,
  CommandShortcut,
} from '@/components/ui/command';
import {
  getAvailableCommands,
  buildRecentProjectCommands,
  type CommandActions,
} from '@/config/commands';
import type { AppMode } from '@/types/appMode';
import type { ProjectMeta } from '@/types/project';

interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  mode: AppMode;
  actions: CommandActions;
  recentProjects: ProjectMeta[];
  onOpenRecent: (filePath: string) => void;
}

export function CommandPalette({
  open,
  onOpenChange,
  mode,
  actions,
  recentProjects,
  onOpenRecent,
}: CommandPaletteProps) {
  const commands = useMemo(() => getAvailableCommands(mode), [mode]);
  const recentCommands = useMemo(
    () => buildRecentProjectCommands(recentProjects),
    [recentProjects]
  );

  // Group static commands by their group name
  const groups = useMemo(() => {
    const map = new Map<string, typeof commands>();
    for (const cmd of commands) {
      const group = map.get(cmd.group);
      if (group) {
        group.push(cmd);
      } else {
        map.set(cmd.group, [cmd]);
      }
    }
    return map;
  }, [commands]);

  const handleSelect = (id: string) => {
    // Check recent projects first
    const recentCmd = recentCommands.find((c) => c.id === id);
    if (recentCmd?.subtitle) {
      onOpenRecent(recentCmd.subtitle);
    } else if (actions[id]) {
      actions[id]();
    }
    onOpenChange(false);
  };

  return (
    <CommandDialog open={open} onOpenChange={onOpenChange}>
      <CommandInput placeholder="Type a command or search..." />
      <CommandList>
        <CommandEmpty>No results found.</CommandEmpty>

        {Array.from(groups).map(([groupName, items]) => (
          <CommandGroup key={groupName} heading={groupName}>
            {items.map((cmd) => (
              <CommandItem
                key={cmd.id}
                value={`${cmd.label} ${cmd.keywords ?? ''}`}
                onSelect={() => handleSelect(cmd.id)}
              >
                {cmd.icon && <cmd.icon className="mr-2 size-4" />}
                <span>{cmd.label}</span>
                {cmd.shortcut && (
                  <CommandShortcut>{cmd.shortcut}</CommandShortcut>
                )}
              </CommandItem>
            ))}
          </CommandGroup>
        ))}

        {recentCommands.length > 0 && (
          <CommandGroup heading="Recent Projects">
            {recentCommands.map((cmd) => (
              <CommandItem
                key={cmd.id}
                value={`${cmd.label} ${cmd.subtitle ?? ''} ${cmd.keywords ?? ''}`}
                onSelect={() => handleSelect(cmd.id)}
              >
                {cmd.icon && <cmd.icon className="mr-2 size-4" />}
                <div className="flex flex-col">
                  <span>{cmd.label}</span>
                  {cmd.subtitle && (
                    <span className="text-xs text-muted-foreground truncate max-w-[350px]">
                      {cmd.subtitle}
                    </span>
                  )}
                </div>
              </CommandItem>
            ))}
          </CommandGroup>
        )}
      </CommandList>
    </CommandDialog>
  );
}
