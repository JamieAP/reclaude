import type { Learning } from '../../api/types';

interface Props {
  learnings: Learning[];
}

export function ExportLearnings({ learnings }: Props) {
  const exportAsMarkdown = () => {
    const content = learnings
      .map(
        (l) =>
          `## ${l.timestamp.slice(0, 10)} - ${l.repo_name || 'unknown'}\n\n${l.content}\n\n---`
      )
      .join('\n\n');

    const blob = new Blob([content], { type: 'text/markdown' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `learnings-${new Date().toISOString().slice(0, 10)}.md`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const exportAsJson = () => {
    const blob = new Blob([JSON.stringify(learnings, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `learnings-${new Date().toISOString().slice(0, 10)}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="flex gap-2">
      <button
        onClick={exportAsMarkdown}
        className="px-3 py-1.5 rounded text-xs hover:opacity-80 transition-opacity"
        style={{ backgroundColor: 'var(--ctp-surface1)', color: 'var(--ctp-text)' }}
      >
        Export MD
      </button>
      <button
        onClick={exportAsJson}
        className="px-3 py-1.5 rounded text-xs hover:opacity-80 transition-opacity"
        style={{ backgroundColor: 'var(--ctp-surface1)', color: 'var(--ctp-text)' }}
      >
        Export JSON
      </button>
    </div>
  );
}
