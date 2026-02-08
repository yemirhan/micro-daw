import { useEffect, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
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
  const [targetRect, setTargetRect] = useState<Rect | null>(null);
  const resizeObserverRef = useRef<ResizeObserver | null>(null);

  useEffect(() => {
    if (!step) {
      setTargetRect(null);
      return;
    }

    const findTarget = () => {
      const el = document.querySelector(`[data-tour="${step.target}"]`);
      if (el) {
        const rect = el.getBoundingClientRect();
        setTargetRect({
          x: rect.x - PADDING,
          y: rect.y - PADDING,
          width: rect.width + PADDING * 2,
          height: rect.height + PADDING * 2,
        });
      } else {
        setTargetRect(null);
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
          transition={{ duration: 0.2 }}
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
                  rx={12}
                  fill="black"
                />
              </mask>
            </defs>
            <rect
              width="100%"
              height="100%"
              fill="oklch(0 0 0 / 0.6)"
              mask="url(#tour-spotlight)"
            />
          </svg>

          {/* Spotlight border glow */}
          <div
            className="absolute rounded-xl pointer-events-none"
            style={{
              left: targetRect.x,
              top: targetRect.y,
              width: targetRect.width,
              height: targetRect.height,
              boxShadow: '0 0 0 2px oklch(0.65 0.20 265 / 0.5), 0 0 20px 4px oklch(0.65 0.20 265 / 0.15)',
            }}
          />

          {/* Tooltip */}
          <motion.div
            key={currentStepIndex}
            initial={{ opacity: 0, scale: 0.95 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.95 }}
            transition={{ duration: 0.2, delay: 0.05 }}
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
