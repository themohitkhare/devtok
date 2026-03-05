import { Check, MessageCircle, TerminalSquare, AlertTriangle } from 'lucide-react';
import { AgentProfileRing } from './AgentProfileRing';
import type { ActionCard } from '../../types';

interface InteractionSidebarProps {
  card: ActionCard;
  onApprove: () => void;
  onComment: () => void;
  isApproved: boolean;
}

export function InteractionSidebar({
  card,
  onApprove,
  onComment,
  isApproved,
}: InteractionSidebarProps) {
  const baseButtonClass =
    'flex items-center justify-center rounded-full border border-zinc-700/50 bg-zinc-900/80 backdrop-blur-sm transition-transform hover:scale-110 active:scale-95';

  return (
    <div
      className="pointer-events-none absolute right-4 top-1/2 flex -translate-y-1/2 flex-col items-center gap-4"
      style={{ zIndex: 20 }}
    >
      <div className="pointer-events-auto">
        <AgentProfileRing
          agentName={card.agentName}
          status={card.status}
          concurrentTasks={card.concurrentTasks}
        />
      </div>

      <button
        type="button"
        onClick={onApprove}
        className={`${baseButtonClass} pointer-events-auto`}
        style={{
          width: 48,
          height: 48,
          color: '#10b981',
          ...(isApproved
            ? {
                boxShadow: '0 0 24px rgba(16,185,129,0.6)',
              }
            : {}),
        }}
        aria-label="Approve"
      >
        <Check strokeWidth={2.5} size={24} />
      </button>

      <button
        type="button"
        onClick={onComment}
        className={`${baseButtonClass} pointer-events-auto`}
        style={{
          width: 48,
          height: 48,
          color: '#f59e0b',
        }}
        aria-label="Comment"
      >
        <MessageCircle strokeWidth={2} size={22} />
      </button>

      <button
        type="button"
        className={`${baseButtonClass} pointer-events-auto`}
        style={{
          width: 48,
          height: 48,
          color: '#06b6d4',
        }}
        aria-label="Terminal"
      >
        <TerminalSquare strokeWidth={2} size={22} />
      </button>

      <button
        type="button"
        className={`${baseButtonClass} pointer-events-auto`}
        style={{
          width: 48,
          height: 48,
          color: '#ef4444',
        }}
        aria-label="Escalate"
      >
        <AlertTriangle strokeWidth={2} size={22} />
      </button>
    </div>
  );
}

