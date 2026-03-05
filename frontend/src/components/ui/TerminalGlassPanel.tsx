interface TerminalGlassPanelProps {
  content: string;
}

export function TerminalGlassPanel({ content }: TerminalGlassPanelProps) {
  const lines = content.split('\n');

  return (
    <div
      className="flex h-full w-full flex-col overflow-hidden rounded-xl"
      style={{
        backgroundColor: 'rgba(10, 14, 26, 0.85)',
        backdropFilter: 'blur(20px)',
        WebkitBackdropFilter: 'blur(20px)',
        border: '1px solid rgba(255, 255, 255, 0.08)',
        boxShadow: '0 0 40px rgba(16, 185, 129, 0.2)',
      }}
    >
      <div
        className="flex items-center gap-2 px-4 py-2.5"
        style={{
          borderBottom: '1px solid rgba(255, 255, 255, 0.08)',
          backgroundColor: 'rgba(0, 0, 0, 0.2)',
        }}
      >
        <span className="h-3 w-3 rounded-full" style={{ backgroundColor: '#ef4444' }} />
        <span className="h-3 w-3 rounded-full" style={{ backgroundColor: '#f59e0b' }} />
        <span className="h-3 w-3 rounded-full" style={{ backgroundColor: '#10b981' }} />
        <span className="ml-2 text-xs font-medium opacity-70" style={{ color: '#94a3b8' }}>
          synapse-agent // terminal
        </span>
      </div>
      <pre
        className="m-0 flex-1 overflow-auto p-4 text-left"
        style={{
          fontFamily:
            '"JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
          fontSize: 12,
          lineHeight: 1.6,
          color: '#10b981',
          textShadow: '0 0 8px rgba(16, 185, 129, 0.5)',
          backgroundColor: 'rgba(10, 14, 26, 0.6)',
          minHeight: 120,
        }}
      >
        {lines.map((line, index) => {
          const upper = line.toUpperCase();
          let color = '#10b981';
          if (upper.includes('ERROR') || upper.includes('FAIL')) {
            color = '#f97373';
          } else if (upper.includes('WARN')) {
            color = '#fbbf24';
          }

          return (
            // eslint-disable-next-line react/no-array-index-key
            <span key={index} style={{ color }}>
              {line}
              {'\n'}
            </span>
          );
        })}
      </pre>
    </div>
  );
}

