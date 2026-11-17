import { LayoutGrid } from 'lucide-react';
import { FilterChips, type ChipOption } from '@/components/learn/kit';
import { PRACTICE_CATEGORY_META, PRACTICE_CATEGORY_ORDER } from './practiceMeta';
import type { PracticeCategory } from '@/types/appMode';

interface PracticeCategoryFilterProps {
  selected: PracticeCategory | null;
  onChange: (category: PracticeCategory | null) => void;
  counts?: Partial<Record<PracticeCategory, number>>;
  total?: number;
}

export function PracticeCategoryFilter({ selected, onChange, counts, total }: PracticeCategoryFilterProps) {
  const options: ChipOption<PracticeCategory | null>[] = [
    { value: null, label: 'All', count: total, icon: LayoutGrid },
    ...PRACTICE_CATEGORY_ORDER.map((c) => ({
      value: c,
      label: PRACTICE_CATEGORY_META[c].label,
      count: counts?.[c],
      icon: PRACTICE_CATEGORY_META[c].icon,
    })),
  ];
  return <FilterChips ariaLabel="Exercise category" options={options} value={selected} onChange={onChange} />;
}
