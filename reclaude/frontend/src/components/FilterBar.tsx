import { useState, useEffect, useCallback, type RefObject } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api } from '../api';

export interface FilterState {
  eventTypes: string[];
  sessionId: string | null;
  timeRange: 'all' | '1h' | '24h' | '7d' | '30d' | 'custom';
  since: Date | null;
  search: string;
}

interface FilterBarProps {
  filters: FilterState;
  onFiltersChange: (filters: FilterState) => void;
  searchInputRef?: RefObject<HTMLInputElement | null>;
}

export function FilterBar({ filters, onFiltersChange, searchInputRef }: FilterBarProps) {
  const [searchInput, setSearchInput] = useState(filters.search);

  // Collapsible state with localStorage
  const [isExpanded, setIsExpanded] = useState(() => {
    const stored = localStorage.getItem('filterbar-expanded');
    return stored !== null ? stored === 'true' : true;
  });

  useEffect(() => {
    localStorage.setItem('filterbar-expanded', String(isExpanded));
  }, [isExpanded]);

  // Fetch available sessions
  const { data: sessions } = useQuery({
    queryKey: ['sessions'],
    queryFn: api.sessions.list,
  });

  // Fetch statistics for event types
  const { data: stats } = useQuery({
    queryKey: ['statistics'],
    queryFn: api.statistics.get,
  });

  // Debounce search input
  useEffect(() => {
    const timer = setTimeout(() => {
      if (searchInput !== filters.search) {
        onFiltersChange({ ...filters, search: searchInput });
      }
    }, 300);
    return () => clearTimeout(timer);
  }, [searchInput, filters, onFiltersChange]);

  const handleEventTypeToggle = useCallback(
    (type: string) => {
      const newTypes = filters.eventTypes.includes(type)
        ? filters.eventTypes.filter((t) => t !== type)
        : [...filters.eventTypes, type];
      onFiltersChange({ ...filters, eventTypes: newTypes });
    },
    [filters, onFiltersChange]
  );

  const handleSessionChange = useCallback(
    (e: React.ChangeEvent<HTMLSelectElement>) => {
      onFiltersChange({
        ...filters,
        sessionId: e.target.value || null,
      });
    },
    [filters, onFiltersChange]
  );

  const handleTimeRangeChange = useCallback(
    (range: FilterState['timeRange']) => {
      let since: Date | null = null;
      if (range !== 'all' && range !== 'custom') {
        const now = new Date();
        switch (range) {
          case '1h':
            since = new Date(now.getTime() - 60 * 60 * 1000);
            break;
          case '24h':
            since = new Date(now.getTime() - 24 * 60 * 60 * 1000);
            break;
          case '7d':
            since = new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000);
            break;
          case '30d':
            since = new Date(now.getTime() - 30 * 24 * 60 * 60 * 1000);
            break;
        }
      }
      onFiltersChange({ ...filters, timeRange: range, since });
    },
    [filters, onFiltersChange]
  );

  const handleClearFilters = useCallback(() => {
    setSearchInput('');
    onFiltersChange({
      eventTypes: [],
      sessionId: null,
      timeRange: 'all',
      since: null,
      search: '',
    });
  }, [onFiltersChange]);

  const hasActiveFilters =
    filters.eventTypes.length > 0 ||
    filters.sessionId ||
    filters.timeRange !== 'all' ||
    filters.search;

  return (
    <div
      className="rounded-lg p-3 space-y-3"
      style={{ backgroundColor: 'var(--ctp-surface0)' }}
    >
      {/* Search + filter toggle */}
      <div className="flex items-center gap-2">
        <input
          ref={searchInputRef}
          type="text"
          value={searchInput}
          onChange={(e) => setSearchInput(e.target.value)}
          placeholder="Search... (press / to focus)"
          className="flex-1 px-3 py-1.5 rounded text-sm outline-none"
          style={{
            backgroundColor: 'var(--ctp-surface1)',
            color: 'var(--ctp-text)',
            border: '1px solid var(--ctp-surface2)',
          }}
        />
        <button
          onClick={() => setIsExpanded(!isExpanded)}
          className="px-2 py-1.5 rounded text-sm transition-opacity hover:opacity-80"
          style={{
            backgroundColor: isExpanded ? 'var(--ctp-surface2)' : 'var(--ctp-surface1)',
            color: 'var(--ctp-subtext0)',
          }}
          title={isExpanded ? 'Hide filters' : 'Show filters'}
        >
          {isExpanded ? '▲' : '▼'}
        </button>
      </div>

      {isExpanded && (
        <div className="flex flex-wrap gap-4">
        {/* Event Types */}
        <div className="flex-1 min-w-48">
          <label
            className="block text-xs mb-2"
            style={{ color: 'var(--ctp-subtext0)' }}
          >
            Event Types
          </label>
          <div className="flex flex-wrap gap-1">
            {stats?.event_types?.map((type) => (
              <button
                key={type}
                onClick={() => handleEventTypeToggle(type)}
                className="px-2 py-1 rounded text-xs transition-colors"
                style={{
                  backgroundColor: filters.eventTypes.includes(type)
                    ? 'var(--ctp-blue)'
                    : 'var(--ctp-surface1)',
                  color: filters.eventTypes.includes(type)
                    ? 'var(--ctp-crust)'
                    : 'var(--ctp-text)',
                }}
              >
                {type}
              </button>
            ))}
          </div>
        </div>

        {/* Session */}
        <div className="w-48">
          <label
            className="block text-xs mb-2"
            style={{ color: 'var(--ctp-subtext0)' }}
          >
            Session
          </label>
          <select
            value={filters.sessionId ?? ''}
            onChange={handleSessionChange}
            className="w-full px-3 py-2 rounded text-sm outline-none"
            style={{
              backgroundColor: 'var(--ctp-surface1)',
              color: 'var(--ctp-text)',
              border: '1px solid var(--ctp-surface2)',
            }}
          >
            <option value="">All sessions</option>
            {sessions?.map((session) => (
              <option key={session.session_id} value={session.session_id}>
                {session.session_id.slice(0, 8)}... ({session.event_count})
              </option>
            ))}
          </select>
        </div>

        {/* Time Range */}
        <div className="w-40">
          <label
            className="block text-xs mb-2"
            style={{ color: 'var(--ctp-subtext0)' }}
          >
            Time Range
          </label>
          <div className="flex flex-wrap gap-1">
            {(['all', '1h', '24h', '7d', '30d'] as const).map((range) => (
              <button
                key={range}
                onClick={() => handleTimeRangeChange(range)}
                className="px-2 py-1 rounded text-xs transition-colors"
                style={{
                  backgroundColor:
                    filters.timeRange === range
                      ? 'var(--ctp-blue)'
                      : 'var(--ctp-surface1)',
                  color:
                    filters.timeRange === range
                      ? 'var(--ctp-crust)'
                      : 'var(--ctp-text)',
                }}
              >
                {range === 'all' ? 'All' : range}
              </button>
            ))}
          </div>
        </div>
        </div>
      )}

      {/* Clear button */}
      {hasActiveFilters && isExpanded && (
        <div className="flex justify-end">
          <button
            onClick={handleClearFilters}
            className="px-3 py-1 rounded text-xs"
            style={{ color: 'var(--ctp-red)' }}
          >
            Clear all filters
          </button>
        </div>
      )}
    </div>
  );
}

export const defaultFilters: FilterState = {
  eventTypes: [],
  sessionId: null,
  timeRange: 'all',
  since: null,
  search: '',
};
