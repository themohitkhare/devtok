import { useMemo } from 'react';
import { STATUS_COLORS } from '../../design-tokens';
import type { ActionStatus, ConcurrentTask } from '../../types';

interface AgentProfileRingProps {
  agentName: string;
  status: ActionStatus;
  concurrentTasks: ConcurrentTask[];
  size?: number;
}

function getInitials(name: string): string {
  return name
    .trim()
    .split(/\s+/)
    .map((part) => part[0])
    .join('')
    .slice(0, 2)
    .toUpperCase();
}

function hashToColor(name: string): string {
  const palette = ['#1d4ed8', '#4f46e5', '#0f766e', '#7c3aed', '#dc2626', '#f97316'];
  let hash = 0;
  for (let i = 0; i < name.length; i += 1) {
    hash = (hash << 5) - hash + name.charCodeAt(i);
    hash |= 0;
  }
  const index = Math.abs(hash) % palette.length;
  return palette[index]!;
}

const ACTIVE_STATUSES: ActionStatus[] = ['running', 'thinking', 'blocked'];

export function AgentProfileRing({
  agentName,
  status,
  concurrentTasks,
  size = 64,
}: AgentProfileRingProps) {
  const ringColor = STATUS_COLORS[status] ?? STATUS_COLORS.queued;
  const initials = useMemo(() => getInitials(agentName), [agentName]);
  const avatarFill = useMemo(() => hashToColor(agentName), [agentName]);

  const { svgSize, center, avatarRadius, orbitRadius } = useMemo(() => {
    const s = size + 16;
    const c = s / 2;
    return {
      svgSize: s,
      center: c,
      avatarRadius: size / 2,
      orbitRadius: size / 2 + 10,
    };
  }, [size]);

  const orbits = useMemo(() => {
    const total = concurrentTasks.length;
    if (total === 0) return [];
    return concurrentTasks.map((task, index) => {
      const angle = (index / total) * 2 * Math.PI - Math.PI / 2;
      const cx = center + orbitRadius * Math.cos(angle);
      const cy = center + orbitRadius * Math.sin(angle);
      const isRunning = task.status === 'running';
      return {
        key: task.id,
        cx,
        cy,
        fill: task.color,
        isRunning,
      };
    });
  }, [center, orbitRadius, concurrentTasks]);

  const isActive = ACTIVE_STATUSES.includes(status);

  return (
    <div
      className="relative flex shrink-0 items-center justify-center"
      style={{ width: svgSize, height: svgSize }}
    >
      <svg width={svgSize} height={svgSize}>
        <circle
          cx={center}
          cy={center}
          r={avatarRadius}
          fill={avatarFill}
          style={{ opacity: 0.9 }}
        />
        <circle
          cx={center}
          cy={center}
          r={avatarRadius + 4}
          fill="none"
          stroke={ringColor}
          strokeWidth={2.5}
          style={
            isActive
              ? {
                  strokeDasharray: '4 4',
                  animation: 'pulse-dot 1.4s ease-in-out infinite',
                }
              : undefined
          }
        />
        {orbits.map((orb) => (
          <circle
            key={orb.key}
            cx={orb.cx}
            cy={orb.cy}
            r={6}
            fill={orb.fill}
            style={
              orb.isRunning
                ? {
                    animation: 'orbital-blink 1.6s ease-in-out infinite',
                  }
                : undefined
            }
          />
        ))}
        <text
          x={center}
          y={center + 4}
          textAnchor="middle"
          fill="#ffffff"
          style={{
            fontFamily:
              '"JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
            fontSize: size * 0.28,
            fontWeight: 600,
          }}
        >
          {initials}
        </text>
      </svg>
    </div>
  );
}

