import { useState, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import ReactMarkdown from 'react-markdown';
import { api } from '../../api';
import { formatDate } from '../../utils/formatUtils';

const TIME_SCALES = ['15min', 'hour', '8hour', 'day', 'week'] as const;
const SCALE_LABELS: Record<string, string> = {
  '15min': '15 min',
  'hour': 'Hour',
  '8hour': '8 Hour',
  'day': 'Day',
  'week': 'Week',
};

export function FocusPage() {
  const [selectedScale, setSelectedScale] = useState<string | null>(null);

  const { data: snapshots, isLoading, error } = useQuery({
    queryKey: ['focus', { time_scale: selectedScale, limit: 100 }],
    queryFn: () =>
      api.focus.list({
        time_scale: selectedScale ?? undefined,
        limit: 100,
      }),
  });

  // Group snapshots by time scale for summary
  const scaleCounts = useMemo(() => {
    if (!snapshots) return {};
    const counts: Record<string, number> = {};
    for (const s of snapshots) {
      counts[s.time_scale] = (counts[s.time_scale] || 0) + 1;
    }
    return counts;
  }, [snapshots]);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <span style={{ color: 'var(--ctp-subtext0)' }}>Loading focus snapshots...</span>
      </div>
    );
  }

  if (error) {
    return (
      <div
        className="p-4 rounded-lg"
        style={{ backgroundColor: 'var(--ctp-surface0)', color: 'var(--ctp-red)' }}
      >
        Error loading focus: {(error as Error).message}
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full gap-4">
      {/* Header */}
      <div className="flex items-center justify-end">
        <span className="text-sm" style={{ color: 'var(--ctp-subtext0)' }}>
          {snapshots?.length ?? 0} snapshots
        </span>
      </div>

      {/* Time scale filter */}
      <div
        className="rounded-lg p-4"
        style={{ backgroundColor: 'var(--ctp-surface0)' }}
      >
        <label className="block text-xs mb-2" style={{ color: 'var(--ctp-subtext0)' }}>
          Time Scale
        </label>
        <div className="flex gap-1 flex-wrap">
          <button
            onClick={() => setSelectedScale(null)}
            className="px-3 py-1 rounded text-xs transition-colors"
            style={{
              backgroundColor: selectedScale === null ? 'var(--ctp-teal)' : 'var(--ctp-surface1)',
              color: selectedScale === null ? 'var(--ctp-crust)' : 'var(--ctp-text)',
            }}
          >
            All
          </button>
          {TIME_SCALES.map((scale) => (
            <button
              key={scale}
              onClick={() => setSelectedScale(scale)}
              className="px-3 py-1 rounded text-xs transition-colors"
              style={{
                backgroundColor: selectedScale === scale ? 'var(--ctp-teal)' : 'var(--ctp-surface1)',
                color: selectedScale === scale ? 'var(--ctp-crust)' : 'var(--ctp-text)',
              }}
            >
              {SCALE_LABELS[scale] ?? scale} {scaleCounts[scale] ? `(${scaleCounts[scale]})` : ''}
            </button>
          ))}
        </div>
      </div>

      {/* Snapshots list */}
      <div className="flex-1 overflow-auto space-y-3">
        {snapshots?.map((snapshot) => (
          <div
            key={snapshot.id}
            className="rounded-lg p-4"
            style={{ backgroundColor: 'var(--ctp-surface0)' }}
          >
            {/* Header */}
            <div className="flex items-start justify-between gap-4 mb-3">
              <div className="flex items-center gap-2">
                <span
                  className="text-xs px-2 py-1 rounded capitalize font-medium"
                  style={{
                    backgroundColor: 'var(--ctp-teal)',
                    color: 'var(--ctp-crust)',
                  }}
                >
                  {snapshot.time_scale}
                </span>
                {snapshot.project && (
                  <span
                    className="text-xs px-2 py-1 rounded font-mono truncate max-w-48"
                    style={{
                      backgroundColor: 'var(--ctp-surface1)',
                      color: 'var(--ctp-lavender)',
                    }}
                    title={snapshot.project}
                  >
                    {snapshot.project.split('/').slice(-2).join('/')}
                  </span>
                )}
              </div>
              <div className="text-right flex-shrink-0">
                <div className="text-xs font-mono" style={{ color: 'var(--ctp-subtext0)' }}>
                  {formatDate(snapshot.period_start, 'compact')}
                  {' → '}
                  {formatDate(snapshot.period_end, 'compact')}
                </div>
                <div className="text-xs" style={{ color: 'var(--ctp-overlay0)' }}>
                  {snapshot.event_count} events
                </div>
              </div>
            </div>

            {/* Focus summary */}
            <div
              className="prose prose-invert prose-sm max-w-none mb-3"
              style={{ color: 'var(--ctp-text)' }}
            >
              <ReactMarkdown>{snapshot.focus_summary}</ReactMarkdown>
            </div>

            {/* Top topics */}
            {snapshot.top_topics.length > 0 && (
              <div className="flex flex-wrap gap-1">
                {snapshot.top_topics.map((topic, i) => (
                  <span
                    key={i}
                    className="text-xs px-2 py-0.5 rounded"
                    style={{
                      backgroundColor: 'var(--ctp-surface1)',
                      color: 'var(--ctp-peach)',
                    }}
                  >
                    {topic}
                  </span>
                ))}
              </div>
            )}
          </div>
        ))}

        {snapshots?.length === 0 && (
          <div
            className="text-center p-8 rounded-lg"
            style={{
              backgroundColor: 'var(--ctp-surface0)',
              color: 'var(--ctp-subtext0)',
            }}
          >
            No focus snapshots found. Run <code className="font-mono px-1 rounded" style={{ backgroundColor: 'var(--ctp-surface1)' }}>reclaude focus hour</code> to generate some.
          </div>
        )}
      </div>
    </div>
  );
}
