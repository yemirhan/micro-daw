import { LayoutGrid } from 'lucide-react';
import { FilterChips, type ChipOption } from './kit';
import { CATEGORY_META, CATEGORY_ORDER } from './lessonMeta';
import type { LessonCategory } from '@/types/appMode';

interface CategoryFilterProps {
  selected: LessonCategory | null;
  onChange: (category: LessonCategory | null) => void;
  /** Lessons per category (for the count badges). */
  counts?: Partial<Record<LessonCategory, number>>;
  total?: number;
}

export function CategoryFilter({ selected, onChange, counts, total }: CategoryFilterProps) {
  const options: ChipOption<LessonCategory | null>[] = [
    { value: null, label: 'All', count: total, icon: LayoutGrid },
    ...CATEGORY_ORDER.map((c) => ({
      value: c,
      label: CATEGORY_META[c].label,
      count: counts?.[c],
      icon: CATEGORY_META[c].icon,
    })),
  ];
  return <FilterChips ariaLabel="Lesson category" options={options} value={selected} onChange={onChange} />;
}
