export const colors = {
  background: '#0a0e1a',
  surface: '#1a1d2e',
  purple: '#2d1b69',
} as const;

export const statusColors = {
  running: '#3b82f6',
  thinking: '#f59e0b',
  success: '#10b981',
  blocked: '#ef4444',
  failed: '#dc2626',
  queued: '#6b7280',
  cancelled: '#475569',
} as const;

export const taskTypeColors = {
  code: '#3b82f6',
  test: '#10b981',
  deploy: '#8b5cf6',
  review: '#f59e0b',
  scan: '#06b6d4',
  migrate: '#ec4899',
  refactor: '#f97316',
} as const;

export const fonts = {
  ui: 'Inter, system-ui, sans-serif',
  code: 'JetBrains Mono, Fira Code, monospace',
} as const;

export type StatusKey = keyof typeof statusColors;
export type TaskTypeKey = keyof typeof taskTypeColors;

// Aliases for backward compat
export const STATUS_COLORS = statusColors;
export const TASK_TYPE_COLORS = taskTypeColors;

export function getStatusLabel(status: StatusKey): string {
  const labels: Record<StatusKey, string> = {
    running: 'Running',
    thinking: 'Thinking...',
    success: 'Completed',
    blocked: 'Needs Input',
    failed: 'Failed',
    queued: 'Queued',
    cancelled: 'Cancelled',
  };
  return labels[status] ?? status;
}
