import { Music, GraduationCap, Target, AudioLines, FolderOpen, Plus, FileUp, Settings, Terminal } from 'lucide-react';
import {
  Sidebar as ShadcnSidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarSeparator,
} from '@/components/ui/sidebar';
import { Kbd } from '@/components/ui/tip';
import type { AppMode } from '@/types/appMode';
import type { ProjectMeta } from '@/types/project';

interface AppSidebarProps {
  mode: AppMode;
  onModeChange: (mode: AppMode) => void;
  recentProjects: ProjectMeta[];
  onOpenRecent: (path: string) => void;
  onNewProject: () => void;
  onOpen: () => void;
  onSettingsOpen: () => void;
}

const IS_MAC = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform || navigator.userAgent);
const MOD = IS_MAC ? '⌘' : 'Ctrl+';

const MODE_ITEMS: { mode: AppMode; icon: typeof Music; label: string; key: string }[] = [
  { mode: 'daw', icon: Music, label: 'DAW', key: '1' },
  { mode: 'learn', icon: GraduationCap, label: 'Learn', key: '2' },
  { mode: 'practice', icon: Target, label: 'Practice', key: '3' },
  { mode: 'samples', icon: AudioLines, label: 'Samples', key: '4' },
  { mode: 'dev', icon: Terminal, label: 'Developer', key: '5' },
];

function TipLabel({ label, shortcut }: { label: string; shortcut?: string }) {
  return (
    <span className="flex items-center gap-2">
      {label}
      {shortcut && <Kbd>{shortcut}</Kbd>}
    </span>
  );
}

export function Sidebar({ mode, onModeChange, recentProjects, onOpenRecent, onNewProject, onOpen, onSettingsOpen }: AppSidebarProps) {
  return (
    <ShadcnSidebar collapsible="icon" data-tour="sidebar">
      <SidebarContent>
        {/* Mode navigation */}
        <SidebarGroup>
          <SidebarMenu>
            {MODE_ITEMS.map((item) => (
              <SidebarMenuItem key={item.mode}>
                <SidebarMenuButton
                  tooltip={{ children: <TipLabel label={item.label} shortcut={`${MOD}${item.key}`} /> }}
                  isActive={mode === item.mode}
                  onClick={() => onModeChange(item.mode)}
                  aria-label={item.label}
                >
                  <item.icon />
                  <span>{item.label}</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            ))}
          </SidebarMenu>
        </SidebarGroup>

        <SidebarSeparator />

        {/* Projects section */}
        <SidebarGroup>
          <SidebarGroupLabel>
            <FolderOpen className="mr-2 size-3.5" />
            Project
          </SidebarGroupLabel>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton tooltip={{ children: <TipLabel label="New project" shortcut={`${MOD}N`} /> }} onClick={onNewProject} aria-label="New project">
                <Plus />
                <span>New</span>
              </SidebarMenuButton>
            </SidebarMenuItem>
            <SidebarMenuItem>
              <SidebarMenuButton tooltip={{ children: <TipLabel label="Open project" shortcut={`${MOD}O`} /> }} onClick={onOpen} aria-label="Open project">
                <FileUp />
                <span>Open</span>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarGroup>

        {/* Recent projects */}
        {recentProjects.length > 0 && (
          <SidebarGroup className="group-data-[collapsible=icon]:hidden">
            <SidebarGroupLabel>Recent</SidebarGroupLabel>
            <SidebarMenu>
              {recentProjects.map((project) => (
                <SidebarMenuItem key={project.filePath}>
                  <SidebarMenuButton
                    size="sm"
                    onClick={() => onOpenRecent(project.filePath)}
                    title={project.filePath}
                  >
                    <span>{project.name}</span>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroup>
        )}
      </SidebarContent>

      <SidebarFooter>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              tooltip={{ children: <TipLabel label="Settings" shortcut={`${MOD},`} /> }}
              isActive={mode === 'settings'}
              onClick={onSettingsOpen}
              aria-label="Settings"
            >
              <Settings />
              <span>Settings</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
    </ShadcnSidebar>
  );
}
