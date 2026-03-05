import { useEffect, useState } from 'react';
import { motion } from 'framer-motion';
import { ActionCard } from './ActionCard';
import type { ActionCard as ActionCardType } from '../types';

interface FeedProps {
  cards: ActionCardType[];
}

export function Feed({ cards }: FeedProps) {
  const [currentIndex, setCurrentIndex] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(0);

  useEffect(() => {
    const updateHeight = () => {
      if (typeof window !== 'undefined') {
        setViewportHeight(window.innerHeight);
      }
    };
    updateHeight();
    window.addEventListener('resize', updateHeight);
    return () => window.removeEventListener('resize', updateHeight);
  }, []);

  const clampIndex = (index: number) =>
    Math.max(0, Math.min(cards.length - 1, index));

  const goNext = () => setCurrentIndex((i) => clampIndex(i + 1));
  const goPrev = () => setCurrentIndex((i) => clampIndex(i - 1));

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'ArrowDown') {
        event.preventDefault();
        goNext();
      } else if (event.key === 'ArrowUp') {
        event.preventDefault();
        goPrev();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [goNext, goPrev]);

  const height = viewportHeight || (typeof window !== 'undefined' ? window.innerHeight : 0);

  return (
    <div
      className="relative h-screen w-screen overflow-hidden"
      style={{ background: '#0a0e1a' }}
    >
      <motion.div
        drag="y"
        dragConstraints={{ top: 0, bottom: 0 }}
        onDragEnd={(_, info) => {
          if (info.offset.y > 80) {
            goPrev();
          } else if (info.offset.y < -80) {
            goNext();
          }
        }}
        className="relative h-full w-full"
      >
        <motion.div
          className="relative h-full w-full"
          animate={{
            y: -height * currentIndex,
          }}
          transition={{ type: 'spring', stiffness: 300, damping: 30 }}
        >
          {cards.map((card, index) => (
            <div
              key={card.id}
              className="absolute inset-0"
              style={{
                transform: `translateY(${index * 100}vh)`,
              }}
            >
              <ActionCard card={card} isActive={index === currentIndex} />
            </div>
          ))}
        </motion.div>
      </motion.div>

      <div className="pointer-events-none absolute right-4 top-4 text-sm text-white/60">
        {currentIndex + 1}/{cards.length}
      </div>
    </div>
  );
}

