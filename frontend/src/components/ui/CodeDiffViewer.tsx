import { useMemo } from 'react';

interface CodeDiffViewerProps {
  content: string;
}

export function CodeDiffViewer({ content }: CodeDiffViewerProps) {
  const lines = useMemo(() => content.split('\n'), [content]);

  return (
    <div
      className="h-full w-full overflow-y-auto rounded-xl border border-zinc-800/60 bg-zinc-900/60"
      style={{
        boxShadow: '0 0 40px rgba(59, 130, 246, 0.1)',
        fontFamily:
          '"JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
        fontSize: 12,
      }}
    >
      <pre className="m-0 p-4">
        {lines.map((line, index) => {
          let backgroundColor = 'transparent';
          let color = 'rgba(226, 232, 240, 0.9)';

          if (line.startsWith('@@')) {
            backgroundColor = 'rgba(129, 140, 248, 0.18)';
            color = '#a855f7';
          } else if (line.startsWith('+') && !line.startsWith('+++')) {
            backgroundColor = 'rgba(16,185,129,0.15)';
            color = '#34d399';
          } else if (line.startsWith('-') && !line.startsWith('---')) {
            backgroundColor = 'rgba(239,68,68,0.15)';
            color = '#f87171';
          }

          return (
            <div
              // eslint-disable-next-line react/no-array-index-key
              key={index}
              className="min-h-[18px] whitespace-pre-wrap rounded-sm px-3 py-0.5"
              style={{
                backgroundColor,
                color,
              }}
            >
              {line || ' '}
            </div>
          );
        })}
      </pre>
    </div>
  );
}

