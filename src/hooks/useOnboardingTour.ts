import { useCallback, useEffect, useRef, useState } from 'react';
import { TOUR_STEPS } from '@/data/tourSteps';
import type { AppMode } from '@/types/appMode';
import type { TourStep } from '@/types/onboarding';

interface UseOnboardingTourOptions {
  hasCompleted: boolean;
  onComplete: () => void;
  switchMode: (mode: AppMode) => void;
}

export function useOnboardingTour({ hasCompleted, onComplete, switchMode }: UseOnboardingTourOptions) {
  const [isActive, setIsActive] = useState(false);
  const [currentStepIndex, setCurrentStepIndex] = useState(0);
  const startedRef = useRef(false);

  // Auto-start on first launch after a delay
  useEffect(() => {
    if (hasCompleted || startedRef.current) return;
    startedRef.current = true;
    const timer = setTimeout(() => setIsActive(true), 500);
    return () => clearTimeout(timer);
  }, [hasCompleted]);

  const currentStep: TourStep | null = isActive ? TOUR_STEPS[currentStepIndex] ?? null : null;

  const applyModeForStep = useCallback((step: TourStep) => {
    if (step.requiredMode) {
      switchMode(step.requiredMode);
    }
  }, [switchMode]);

  const nextStep = useCallback(() => {
    const nextIndex = currentStepIndex + 1;
    if (nextIndex >= TOUR_STEPS.length) {
      setIsActive(false);
      onComplete();
    } else {
      setCurrentStepIndex(nextIndex);
      applyModeForStep(TOUR_STEPS[nextIndex]);
    }
  }, [currentStepIndex, onComplete, applyModeForStep]);

  const prevStep = useCallback(() => {
    const prev = Math.max(0, currentStepIndex - 1);
    setCurrentStepIndex(prev);
    applyModeForStep(TOUR_STEPS[prev]);
  }, [currentStepIndex, applyModeForStep]);

  const skipTour = useCallback(() => {
    setIsActive(false);
    onComplete();
    switchMode('daw');
  }, [onComplete, switchMode]);

  const startTour = useCallback(() => {
    setCurrentStepIndex(0);
    setIsActive(true);
    const first = TOUR_STEPS[0];
    if (first.requiredMode) {
      switchMode(first.requiredMode);
    }
  }, [switchMode]);

  return {
    isActive,
    currentStep,
    currentStepIndex,
    totalSteps: TOUR_STEPS.length,
    nextStep,
    prevStep,
    skipTour,
    startTour,
  };
}
