/**
 * Built-in classic kits: original syntheses in the style of classic drum
 * machines (808-, 909-, 707-, 606-, LinnDrum-, CR-78-style). See
 * docs/packs-plan.md (P1).
 */
export {
  CLASSIC_MACHINES,
  CLASSIC_MACHINE_IDS,
  classicKitId,
  classicSampleName,
  classicVoiceLabel,
  getClassicMachine,
  getClassicVoice,
  isClassicMachineId,
  isClassicVoice,
  type ClassicMachine,
  type ClassicVoiceInfo,
} from './machines';
export { classicSample, createClassicKit } from './kit';
export { CLASSIC_SAMPLE_RATE, hasClassicRecipe, renderClassicVoice, synthesizeClassicVoice, synthesizeClassicVoiceAsync } from './render';
export { analyzeVoice, type VoiceAnalysis } from './analysis';
