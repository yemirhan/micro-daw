import { useEffect, useRef, useState } from 'react';
import { AnimatePresence, motion, useReducedMotion } from 'framer-motion';
import { TourTooltip } from './TourTooltip';
import type { TourStep } from '@/types/onboarding';

interface TourOverlayProps {
  step: TourStep | null;
  currentStepIndex: number;
  totalSteps: number;
  onNext: () => void;
  onPrev: () => void;
  onSkip: () => void;
}

interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

const PADDING = 8;

function getTooltipPosition(
  target: Rect,
  placement: TourStep['placement'],
): React.CSSProperties {
  const gap = 12;
  switch (placement) {
    case 'right':
      return {
        left: target.x + target.width + gap,
        top: target.y + target.height / 2,
        transform: 'translateY(-50%)',
      };
    case 'left':
      return {
        right: window.innerWidth - target.x + gap,
        top: target.y + target.height / 2,
        transform: 'translateY(-50%)',
      };
    case 'bottom':
      return {
        left: target.x + target.width / 2,
        top: target.y + target.height + gap,
        transform: 'translateX(-50%)',
      };
    case 'top':
      return {
        left: target.x + target.width / 2,
        bottom: window.innerHeight - target.y + gap,
        transform: 'translateX(-50%)',
      };
  }
}

export function TourOverlay({
  step,
  currentStepIndex,
  totalSteps,
  onNext,
  onPrev,
  onSkip,
}: TourOverlayProps) {
  /** Measured spotlight, tagged with the step target it belongs to. */
  const [measured, setMeasured] = useState<{ target: string; rect: Rect | null } | null>(null);
  const targetRect = step && measured?.target === step.target ? measured.rect : null;
  const resizeObserverRef = useRef<ResizeObserver | null>(null);
  const reduceMotion = useReducedMotion();

  // Keyboard: Esc skips, Right advances, Left goes back.
  const handlersRef = useRef({ onNext, onPrev, onSkip });
  useEffect(() => {
    handlersRef.current = { onNext, onPrev, onSkip };
  });
  const visible = !!step && !!targetRect;
  useEffect(() => {
    if (!visible) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        handlersRef.current.onSkip();
      } else if (e.key === 'ArrowRight') {
        e.preventDefault();
        handlersRef.current.onNext();
      } else if (e.key === 'ArrowLeft') {
        e.preventDefault();
        handlersRef.current.onPrev();
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [visible]);

  useEffect(() => {
    if (!step) return;

    const findTarget = () => {
      const el = document.querySelector(`[data-tour="${step.target}"]`);
      if (el) {
        const rect = el.getBoundingClientRect();
        setMeasured({
          target: step.target,
          rect: {
            x: rect.x - PADDING,
            y: rect.y - PADDING,
            width: rect.width + PADDING * 2,
            height: rect.height + PADDING * 2,
          },
        });
      } else {
        setMeasured({ target: step.target, rect: null });
      }
    };

    // Delay to allow mode switch rendering
    const timer = setTimeout(findTarget, 100);

    // Observe resize
    resizeObserverRef.current?.disconnect();
    const observer = new ResizeObserver(findTarget);
    resizeObserverRef.current = observer;
    const el = document.querySelector(`[data-tour="${step.target}"]`);
    if (el) observer.observe(el);

    window.addEventListener('resize', findTarget);
    return () => {
      clearTimeout(timer);
      observer.disconnect();
      window.removeEventListener('resize', findTarget);
    };
  }, [step]);

  return (
    <AnimatePresence>
      {step && targetRect && (
        <motion.div
          key="tour-overlay"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: reduceMotion ? 0 : 0.18 }}
          className="fixed inset-0 z-[100]"
        >
          {/* SVG mask overlay */}
          <svg className="absolute inset-0 h-full w-full">
            <defs>
              <mask id="tour-spotlight">
                <rect width="100%" height="100%" fill="white" />
                <rect
                  x={targetRect.x}
                  y={targetRect.y}
                  width={targetRect.width}
                  height={targetRect.height}
                  rx={10}
                  fill="black"
                />
              </mask>
            </defs>
            <rect
              width="100%"
              height="100%"
              fill="oklch(0.1 0.004 65 / 0.72)"
              mask="url(#tour-spotlight)"
            />
          </svg>

          {/* Spotlight outline */}
          <div
            className="pointer-events-none absolute rounded-[10px] transition-[left,top,width,height] duration-200 motion-reduce:transition-none"
            style={{
              left: targetRect.x,
              top: targetRect.y,
              width: targetRect.width,
              height: targetRect.height,
              boxShadow: '0 0 0 1.5px var(--primary)',
            }}
          />

          {/* Tooltip */}
          <motion.div
            key={currentStepIndex}
            initial={{ opacity: 0, y: reduceMotion ? 0 : 4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0 }}
            transition={{ duration: reduceMotion ? 0 : 0.16, delay: reduceMotion ? 0 : 0.04 }}
            className="absolute z-[101]"
            style={getTooltipPosition(targetRect, step.placement)}
          >
            <TourTooltip
              title={step.title}
              description={step.description}
              currentStep={currentStepIndex}
              totalSteps={totalSteps}
              onNext={onNext}
              onPrev={onPrev}
              onSkip={onSkip}
            />
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
