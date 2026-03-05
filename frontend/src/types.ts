export type ConcurrentTaskStatus = 'running' | 'success' | 'failed';

export interface ConcurrentTask {
  id: string;
  taskType: string;
  status: ConcurrentTaskStatus;
  color: string;
}

export type ActionStatus =
  | 'running'
  | 'thinking'
  | 'success'
  | 'blocked'
  | 'failed'
  | 'queued'
  | 'cancelled';

export type ActionVisualType = 'CodeDiff' | 'TerminalOutput';

export interface ActionCard {
  id: string;
  agentName: string;
  agentHandle: string;
  specialty: string;
  status: ActionStatus;
  visualType: ActionVisualType;
  taskSummary: string;
  content: string;
  priority: number;
  concurrentTasks: ConcurrentTask[];
  tags: string[];
}
