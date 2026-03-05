import { useCallback, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { Check } from 'lucide-react';
import { CodeDiffViewer } from './ui/CodeDiffViewer';
import { TerminalGlassPanel } from './ui/TerminalGlassPanel';
import { InteractionSidebar } from './ui/InteractionSidebar';
import { BottomOverlay } from './ui/BottomOverlay';
import type { ActionCard as ActionCardType } from '../types';

interface ActionCardProps {
  card: ActionCardType;
  isActive?: boolean;
  onApprove?: () => void;
  approved?: boolean;
}

const DOUBLE_TAP_MS = 300;

export function ActionCard({ card, onApprove, approved = false }: ActionCardProps) {
  const lastTapRef = useRef<number | null>(null);
  const [showApproveAnim, setShowApproveAnim] = useState(false);

  const triggerApprove = useCallback(() => {
    setShowApproveAnim(true);
    onApprove?.();
    setTimeout(() => setShowApproveAnim(false), 600);
  }, [onApprove]);

  const handleTap = useCallback(() => {
    const now = Date.now();
    if (lastTapRef.current && now - lastTapRef.current < DOUBLE_TAP_MS) {
      triggerApprove();
    }
    lastTapRef.current = now;
  }, [triggerApprove]);

  return (
    <div
      className="relative h-screen w-full overflow-hidden"
      onClick={handleTap}
    >
      {/* Layer 1: Background mesh gradient */}
      <div
        className="absolute inset-0"
        style={{
          background: `
            radial-gradient(ellipse 80% 50% at 50% 0%, #2d1b69 0%, transparent 50%),
            radial-gradient(ellipse 60% 40% at 80% 60%, #1a1d2e 0%, transparent 45%),
            radial-gradient(ellipse 70% 50% at 20% 80%, #0a0e1a 0%, transparent 50%),
            #0a0e1a
          `,
        }}
      />

      {/* Layer 1.5: Content panel */}
      <div
        className="absolute z-10 flex flex-col"
        style={{ left: '5%', right: '80px', top: '15%', bottom: '220px' }}
      >
        {card.visualType === 'CodeDiff' ? (
          <CodeDiffViewer content={card.content} />
        ) : (
          <TerminalGlassPanel content={card.content} />
        )}
      </div>

      {/* Layer 2: Interaction sidebar */}
      <InteractionSidebar
        card={card}
        onApprove={triggerApprove}
        onComment={() => { /* placeholder */ }}
        isApproved={approved}
      />

      {/* Layer 3: Bottom overlay */}
      <BottomOverlay card={card} />

      {/* Double-tap approve flash */}
      <AnimatePresence>
        {showApproveAnim && (
          <motion.div
            className="pointer-events-none absolute inset-0 z-40 flex items-center justify-center"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.1 }}
          >
            <motion.div
              initial={{ scale: 0, opacity: 1 }}
              animate={{ scale: [0, 1.5, 0.9], opacity: [1, 1, 0] }}
              transition={{ duration: 0.6, ease: 'easeOut' }}
            >
              <Check size={96} strokeWidth={2.5} style={{ color: '#10b981' }} />
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
