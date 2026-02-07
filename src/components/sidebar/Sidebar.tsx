import { Music, GraduationCap, Target, FolderOpen, Plus, FileUp } from 'lucide-react';
import {
  Sidebar as ShadcnSidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarSeparator,
} from '@/components/ui/sidebar';
import type { AppMode } from '@/types/appMode';
import type { ProjectMeta } from '@/types/project';

interface AppSidebarProps {
  mode: AppMode;
  onModeChange: (mode: AppMode) => void;
  recentProjects: ProjectMeta[];
  onOpenRecent: (path: string) => void;
  onNewProject: () => void;
  onOpen: () => void;
}

const MODE_ITEMS: { mode: AppMode; icon: typeof Music; label: string }[] = [
  { mode: 'daw', icon: Music, label: 'DAW' },
  { mode: 'learn', icon: GraduationCap, label: 'Learn' },
  { mode: 'practice', icon: Target, label: 'Practice' },
];

export function Sidebar({ mode, onModeChange, recentProjects, onOpenRecent, onNewProject, onOpen }: AppSidebarProps) {
  return (
    <ShadcnSidebar collapsible="icon">
      <SidebarContent>
        {/* Mode navigation */}
        <SidebarGroup>
          <SidebarMenu>
            {MODE_ITEMS.map((item) => (
              <SidebarMenuItem key={item.mode}>
                <SidebarMenuButton
                  tooltip={item.label}
                  isActive={mode === item.mode}
                  onClick={() => onModeChange(item.mode)}
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
            <FolderOpen className="mr-2 h-4 w-4" />
            Projects
          </SidebarGroupLabel>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton tooltip="New Project" onClick={onNewProject}>
                <Plus />
                <span>New</span>
              </SidebarMenuButton>
            </SidebarMenuItem>
            <SidebarMenuItem>
              <SidebarMenuButton tooltip="Open Project" onClick={onOpen}>
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
    </ShadcnSidebar>
  );
}
