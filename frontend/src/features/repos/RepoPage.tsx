import { useState, useMemo } from 'react';
import { useParams, Link } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { api } from '../../api';
import { UnifiedTimeline } from '../../components/UnifiedTimeline';
import { RepoEventsView } from './RepoEventsView';
import { RepoFilters } from './RepoFilters';
import { calculateSinceDate, type TimeRange } from '../../utils/dateUtils';

type ViewMode = 'unified' | 'events';

export function RepoPage() {
  const { remoteUrl } = useParams<{ remoteUrl: string }>();
  const decodedUrl = decodeURIComponent(remoteUrl || '');
  const [viewMode, setViewMode] = useState<ViewMode>('unified');
  const [selectedTypes, setSelectedTypes] = useState<string[]>([]);
  const [timeRange, setTimeRange] = useState<TimeRange>('24h');

  const { data: stats } = useQuery({
    queryKey: ['statistics'],
    queryFn: api.statistics.get,
  });

  const repoInfo = stats?.repos?.find((r) => r.remote_url === decodedUrl);
  const since = useMemo(() => calculateSinceDate(timeRange), [timeRange]);

  const { data: events, isLoading, error } = useQuery({
    queryKey: ['repo-events', decodedUrl, since],
    queryFn: () => api.events.list({ limit: 500, since }),
    enabled: !!decodedUrl,
    refetchInterval: 5000,
  });

  // Filter events by repo
  const repoEvents = useMemo(() => {
    if (!events) return [];
    return events.filter((e) => e.metadata?.remote_url === decodedUrl);
  }, [events, decodedUrl]);

  // Event types present in this repo
  const eventTypes = useMemo(() => {
    const types = new Set(repoEvents.map((e) => e.event_type));
    return Array.from(types).sort();
  }, [repoEvents]);

  // Counts by type
  const countsByType = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const e of repoEvents) {
      counts[e.event_type] = (counts[e.event_type] || 0) + 1;
    }
    return counts;
  }, [repoEvents]);

  // Filtered data for unified view
  const filteredEvents = useMemo(() => {
    if (selectedTypes.length === 0) return repoEvents;
    return repoEvents.filter((e) => selectedTypes.includes(e.event_type));
  }, [repoEvents, selectedTypes]);

  const toggleType = (type: string) => {
    setSelectedTypes((prev) =>
      prev.includes(type) ? prev.filter((t) => t !== type) : [...prev, type]
    );
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <span style={{ color: 'var(--ctp-subtext0)' }}>Loading...</span>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-4 rounded-lg" style={{ backgroundColor: 'var(--ctp-surface0)', color: 'var(--ctp-red)' }}>
        Error loading events: {(error as Error).message}
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full gap-4">
      {/* Header */}
      <div className="flex items-center gap-4">
        <Link to="/" className="text-sm hover:underline" style={{ color: 'var(--ctp-blue)' }}>
          &lt;- Dashboard
        </Link>
        <h1 className="text-xl font-semibold" style={{ color: 'var(--ctp-text)' }}>
          {repoInfo?.repo_name || decodedUrl}
        </h1>
      </div>

      {/* Repo info bar */}
      <div className="flex items-center justify-between p-4 rounded-lg" style={{ backgroundColor: 'var(--ctp-surface0)' }}>
        <div className="text-sm" style={{ color: 'var(--ctp-subtext0)' }}>{decodedUrl}</div>
        <div className="flex items-center gap-4">
          <span className="text-sm" style={{ color: 'var(--ctp-subtext1)' }}>{filteredEvents.length} in view</span>
          <span className="text-sm" style={{ color: 'var(--ctp-text)' }}>
            {repoInfo?.event_count ?? 0} total
          </span>
        </div>
      </div>

      {/* Filters */}
      <RepoFilters
        viewMode={viewMode}
        setViewMode={setViewMode}
        timeRange={timeRange}
        setTimeRange={setTimeRange}
        eventTypes={eventTypes}
        selectedTypes={selectedTypes}
        toggleType={toggleType}
        clearTypes={() => setSelectedTypes([])}
        countsByType={countsByType}
      />

      {/* Content area */}
      <div className="flex-1 min-h-0">
        {viewMode === 'unified' && (
          <UnifiedTimeline events={filteredEvents} hasMore={false} />
        )}
        {viewMode === 'events' && (
          <RepoEventsView events={repoEvents} selectedTypes={selectedTypes} />
        )}
      </div>
    </div>
  );
}
