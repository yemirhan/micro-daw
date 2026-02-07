import { Badge } from '@/components/ui/badge';
import type { ChordInfo } from '@/types/music';

interface ChordDisplayProps {
  chord: ChordInfo | null;
}

export function ChordDisplay({ chord }: ChordDisplayProps) {
  return (
    <Badge
      variant={chord ? 'default' : 'secondary'}
      className="min-w-[48px] justify-center font-mono text-xs"
    >
      {chord ? chord.display : '---'}
    </Badge>
  );
}
