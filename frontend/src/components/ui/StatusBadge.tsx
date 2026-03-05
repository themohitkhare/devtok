import { STATUS_COLORS, getStatusLabel } from '../../design-tokens';
import type { ActionStatus } from '../../types';

interface StatusBadgeProps {
  status: ActionStatus;
  className?: string;
}

const PULSE_STATES: ActionStatus[] = ['running', 'thinking', 'blocked'];

export function StatusBadge({ status, className = '' }: StatusBadgeProps) {
  const color = STATUS_COLORS[status] ?? STATUS_COLORS.queued;
  const label = getStatusLabel(status);
  const shouldPulse = PULSE_STATES.includes(status);

  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium ${className}`}
      style={{ color: '#e2e8f0' }}
    >
      <span
        className="h-2 w-2 shrink-0 rounded-full"
        style={{
          backgroundColor: color,
          animation: shouldPulse ? 'pulse-dot 1.2s ease-in-out infinite' : undefined,
        }}
      />
      {label}
    </span>
  );
}

