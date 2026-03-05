import { StatusBadge } from './StatusBadge';
import type { ActionCard } from '../../types';

interface BottomOverlayProps {
  card: ActionCard;
}

export function BottomOverlay({ card }: BottomOverlayProps) {
  return (
    <div
      className="absolute bottom-0 left-0 right-0 z-30 flex flex-col justify-end px-4 pb-10 pt-24"
      style={{
        background:
          'linear-gradient(to top, rgba(0,0,0,0.95) 0%, rgba(0,0,0,0.0) 100%)',
      }}
    >
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-xl font-bold text-white">
          {card.agentHandle}
        </span>
        <StatusBadge status={card.status} />
      </div>
      <p className="mt-1 text-sm text-zinc-300">{card.taskSummary}</p>
      {card.tags.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-2 text-xs text-zinc-500">
          {card.tags.map((tag) => (
            <span key={tag}>#{tag}</span>
          ))}
        </div>
      )}
    </div>
  );
}

