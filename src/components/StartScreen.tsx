import { useState } from 'react';
import { motion } from 'framer-motion';

interface StartScreenProps {
  onStart: () => Promise<void>;
}

/* Animated EQ bars */
function AudioBars() {
  const bars = [0.6, 1, 0.45, 0.85, 0.35, 0.7, 1, 0.5, 0.8, 0.4, 0.95, 0.55];
  return (
    <div className="flex items-end gap-[3px] h-10">
      {bars.map((peak, i) => (
        <div
          key={i}
          className="start-anim-bar w-[3px] rounded-full"
          style={{
            height: `${peak * 100}%`,
            background: 'linear-gradient(to top, oklch(0.65 0.22 265), oklch(0.75 0.18 290))',
            animationDelay: `${i * 0.12}s`,
            animationDuration: `${1.2 + (i % 3) * 0.2}s`,
          }}
        />
      ))}
    </div>
  );
}

/* Stagger children container */
const stagger = {
  hidden: {},
  show: { transition: { staggerChildren: 0.12, delayChildren: 0.2 } },
};

const fadeUp = {
  hidden: { opacity: 0, y: 24, filter: 'blur(6px)' },
  show: {
    opacity: 1,
    y: 0,
    filter: 'blur(0px)',
    transition: { duration: 0.7, ease: [0.16, 1, 0.3, 1] },
  },
};

export function StartScreen({ onStart }: StartScreenProps) {
  const [loading, setLoading] = useState(false);
  const [hovered, setHovered] = useState(false);

  const handleStart = async () => {
    setLoading(true);
    try {
      await onStart();
    } catch (err) {
      console.error('Failed to start:', err);
      setLoading(false);
    }
  };

  return (
    <div
      className="relative flex flex-1 flex-col items-center justify-center overflow-hidden"
      style={{ background: 'oklch(0.06 0.015 270)' }}
    >
      {/* ── Central content ── */}
      <motion.div
        className="relative z-10 flex flex-col items-center gap-8"
        variants={stagger}
        initial="hidden"
        animate="show"
      >
        {/* Icon */}
        <motion.div
          variants={fadeUp}
          className="flex items-center justify-center rounded-2xl"
          style={{
            width: 88,
            height: 88,
            background: 'linear-gradient(135deg, oklch(0.14 0.03 265 / 0.7), oklch(0.10 0.02 270 / 0.5))',
            backdropFilter: 'blur(16px)',
            border: '1px solid oklch(0.65 0.22 265 / 0.12)',
            boxShadow: '0 8px 32px oklch(0 0 0 / 0.5), inset 0 1px 0 oklch(1 0 0 / 0.04)',
          }}
        >
          <AudioBars />
        </motion.div>

        {/* Title */}
        <motion.div variants={fadeUp} className="flex flex-col items-center gap-2.5">
          <h1
            style={{
              fontFamily: "'Syne', sans-serif",
              fontWeight: 800,
              fontSize: '2.75rem',
              letterSpacing: '-0.03em',
              lineHeight: 1,
              background:
                'linear-gradient(135deg, oklch(0.96 0.01 270) 0%, oklch(0.72 0.15 265) 45%, oklch(0.60 0.22 290) 100%)',
              WebkitBackgroundClip: 'text',
              WebkitTextFillColor: 'transparent',
              backgroundClip: 'text',
              filter: 'drop-shadow(0 2px 12px oklch(0.65 0.22 265 / 0.3))',
            }}
          >
            MICRO DAW
          </h1>
          <p
            style={{
              fontFamily: "'JetBrains Mono', monospace",
              fontSize: '0.65rem',
              letterSpacing: '0.22em',
              textTransform: 'uppercase' as const,
              color: 'oklch(0.50 0.08 265)',
            }}
          >
            Create &middot; Compose &middot; Perform
          </p>
        </motion.div>

        {/* Start button */}
        <motion.button
          variants={fadeUp}
          onClick={handleStart}
          disabled={loading}
          onMouseEnter={() => setHovered(true)}
          onMouseLeave={() => setHovered(false)}
          whileHover={!loading ? { scale: 1.04 } : {}}
          whileTap={!loading ? { scale: 0.97 } : {}}
          className="start-anim-glow-btn relative overflow-hidden rounded-xl px-10 py-3 font-semibold tracking-wide disabled:opacity-50 disabled:cursor-not-allowed"
          style={{
            background: loading
              ? 'oklch(0.20 0.03 265)'
              : 'linear-gradient(135deg, oklch(0.50 0.22 265), oklch(0.45 0.20 280))',
            color: 'oklch(0.95 0.01 270)',
            fontSize: '0.9rem',
            fontFamily: "'Manrope', sans-serif",
            border: '1px solid oklch(0.55 0.18 265 / 0.3)',
            cursor: loading ? undefined : 'pointer',
          }}
        >
          {/* Shimmer sweep */}
          {!loading && (
            <div
              className="pointer-events-none absolute inset-0"
              style={{
                background:
                  'linear-gradient(105deg, transparent 40%, oklch(1 0 0 / 0.07) 50%, transparent 60%)',
                transform: hovered ? 'translateX(100%)' : 'translateX(-100%)',
                transition: 'transform 0.6s ease',
              }}
            />
          )}
          {loading ? (
            <span className="flex items-center gap-2">
              <svg className="animate-spin h-4 w-4" viewBox="0 0 24 24" fill="none">
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="3" />
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
              </svg>
              Initializing...
            </span>
          ) : (
            'Start Session'
          )}
        </motion.button>

        {/* Hint text */}
        <motion.p
          variants={fadeUp}
          className="max-w-xs text-center"
          style={{
            fontSize: '0.65rem',
            lineHeight: 1.7,
            color: 'oklch(0.35 0.02 270)',
          }}
        >
          Initializes the audio engine and MIDI system.
          <br />
          Connect a controller for the best experience.
        </motion.p>
      </motion.div>

      {/* ── Version ── */}
      <motion.div
        className="absolute bottom-4"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ delay: 1.2, duration: 0.8 }}
        style={{
          fontFamily: "'JetBrains Mono', monospace",
          fontSize: '0.55rem',
          color: 'oklch(0.25 0.02 270)',
          letterSpacing: '0.12em',
        }}
      >
        v1.0.0
      </motion.div>
    </div>
  );
}
