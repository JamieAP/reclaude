import type { TimeRange } from '../../utils/dateUtils';

type ViewMode = 'unified' | 'events';

interface RepoFiltersProps {
  viewMode: ViewMode;
  setViewMode: (mode: ViewMode) => void;
  timeRange: TimeRange;
  setTimeRange: (range: TimeRange) => void;
  eventTypes: string[];
  selectedTypes: string[];
  toggleType: (type: string) => void;
  clearTypes: () => void;
  countsByType: Record<string, number>;
}

const EVENT_COLORS: Record<string, string> = {
  user_prompt: 'var(--ctp-blue)',
  tool_use: 'var(--ctp-teal)',
  file_diff: 'var(--ctp-peach)',
  plan: 'var(--ctp-mauve)',
  compaction: 'var(--ctp-yellow)',
  // Task management
  task_create: 'var(--ctp-green)',
  task_update: 'var(--ctp-yellow)',
  task_get: 'var(--ctp-lavender)',
  task_list: 'var(--ctp-lavender)',
  todo_write: 'var(--ctp-peach)',
  // Subagent lifecycle
  subagent_spawn: 'var(--ctp-mauve)',
  subagent_output: 'var(--ctp-mauve)',
};

export function RepoFilters({
  viewMode,
  setViewMode,
  timeRange,
  setTimeRange,
  eventTypes,
  selectedTypes,
  toggleType,
  clearTypes,
  countsByType,
}: RepoFiltersProps) {
  return (
    <div className="flex flex-wrap items-center gap-4 p-4 rounded-lg" style={{ backgroundColor: 'var(--ctp-surface0)' }}>
      {/* View mode */}
      <div className="flex items-center gap-2">
        <span className="text-xs" style={{ color: 'var(--ctp-subtext0)' }}>View:</span>
        {(['unified', 'events'] as const).map((mode) => (
          <button
            key={mode}
            onClick={() => setViewMode(mode)}
            className="px-2 py-1 rounded text-xs transition-colors"
            style={{
              backgroundColor: viewMode === mode ? 'var(--ctp-mauve)' : 'var(--ctp-surface1)',
              color: viewMode === mode ? 'var(--ctp-crust)' : 'var(--ctp-text)',
            }}
          >
            {mode === 'unified' ? 'All' : 'Events'}
          </button>
        ))}
      </div>

      {/* Time range */}
      <div className="flex items-center gap-2">
        <span className="text-xs" style={{ color: 'var(--ctp-subtext0)' }}>Time:</span>
        {(['1h', '24h', '7d', 'all'] as const).map((range) => (
          <button
            key={range}
            onClick={() => setTimeRange(range)}
            className="px-2 py-1 rounded text-xs transition-colors"
            style={{
              backgroundColor: timeRange === range ? 'var(--ctp-blue)' : 'var(--ctp-surface1)',
              color: timeRange === range ? 'var(--ctp-crust)' : 'var(--ctp-text)',
            }}
          >
            {range === 'all' ? 'All' : range}
          </button>
        ))}
      </div>

      {/* Type filters (unified view only) */}
      {viewMode === 'unified' && eventTypes.length > 0 && (
        <div className="flex items-center gap-2 flex-wrap">
          <span className="text-xs" style={{ color: 'var(--ctp-subtext0)' }}>Types:</span>
          {eventTypes.map((type) => (
            <button
              key={type}
              onClick={() => toggleType(type)}
              className="px-2 py-1 rounded text-xs transition-colors flex items-center gap-1"
              style={{
                backgroundColor:
                  selectedTypes.length === 0 || selectedTypes.includes(type)
                    ? EVENT_COLORS[type] || 'var(--ctp-surface2)'
                    : 'var(--ctp-surface1)',
                color:
                  selectedTypes.length === 0 || selectedTypes.includes(type)
                    ? 'var(--ctp-crust)'
                    : 'var(--ctp-subtext0)',
                opacity: selectedTypes.length > 0 && !selectedTypes.includes(type) ? 0.5 : 1,
              }}
            >
              {type}
              <span className="opacity-70">({countsByType[type] || 0})</span>
            </button>
          ))}
          {selectedTypes.length > 0 && (
            <button onClick={clearTypes} className="text-xs underline" style={{ color: 'var(--ctp-red)' }}>
              clear
            </button>
          )}
        </div>
      )}
    </div>
  );
}
