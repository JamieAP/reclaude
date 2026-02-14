import { useState, useCallback, useMemo, useEffect, useRef } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useSearchParams } from 'react-router-dom';
import { api, type EventsQueryParams } from '../../api';
import { VirtualizedEventTable } from '../../components/VirtualizedEventTable';
import { FilterBar, defaultFilters, type FilterState } from '../../components/FilterBar';
import { useKeyboardNavigation } from '../../hooks/useKeyboardNavigation';

const PAGE_SIZE = 100;
const POLL_INTERVAL = 3000; // 3 seconds

export function EventsPage() {
  const [searchParams] = useSearchParams();
  const [offset, setOffset] = useState(0);
  const [isLive, setIsLive] = useState(true);
  const searchInputRef = useRef<HTMLInputElement>(null);

  // Initialize filters from URL params
  const initialFilters = useMemo<FilterState>(() => {
    const sessionId = searchParams.get('session_id');
    const eventType = searchParams.get('type');
    return {
      ...defaultFilters,
      sessionId: sessionId || null,
      eventTypes: eventType ? [eventType] : [],
    };
  }, [searchParams]);

  const [filters, setFilters] = useState<FilterState>(initialFilters);

  // Update filters when URL params change
  useEffect(() => {
    setFilters(initialFilters);
  }, [initialFilters]);

  // Convert filter state to API params
  const queryParams = useMemo<EventsQueryParams>(() => {
    const params: EventsQueryParams = {
      limit: PAGE_SIZE,
      offset,
    };

    if (filters.eventTypes.length > 0) {
      params.event_type = filters.eventTypes;
    }

    if (filters.sessionId) {
      params.session_id = filters.sessionId;
    }

    if (filters.since) {
      params.since = filters.since.toISOString();
    }

    return params;
  }, [offset, filters]);

  const { data, isLoading, error, isFetching, dataUpdatedAt } = useQuery({
    queryKey: ['events', queryParams],
    queryFn: () => api.events.list(queryParams),
    refetchInterval: isLive ? POLL_INTERVAL : false,
  });

  // Filter client-side for search and deduplication
  const filteredEvents = useMemo(() => {
    let events = data ?? [];

    // Search filtering
    if (filters.search) {
      const searchLower = filters.search.toLowerCase();
      events = events.filter((e) =>
        e.content.toLowerCase().includes(searchLower)
      );
    }

    // Deduplicate consecutive events with same type and content
    // (fixes bug where plan/thinking was captured multiple times)
    const deduped: typeof events = [];
    for (const event of events) {
      const prev = deduped[deduped.length - 1];
      if (prev && prev.event_type === event.event_type && prev.content === event.content) {
        // Skip duplicate - keep the newer one (events are in desc order)
        continue;
      }
      deduped.push(event);
    }

    return deduped;
  }, [data, filters.search]);

  // Keyboard navigation
  const { selectedIndex } = useKeyboardNavigation({
    itemCount: filteredEvents.length,
    onFocusSearch: () => searchInputRef.current?.focus(),
  });

  const handleFiltersChange = useCallback((newFilters: FilterState) => {
    setFilters(newFilters);
    setOffset(0); // Reset to first page on filter change
  }, []);

  const handlePrevPage = useCallback(() => {
    setOffset((prev) => Math.max(0, prev - PAGE_SIZE));
  }, []);

  const handleNextPage = useCallback(() => {
    // If we got a full page, there's likely more
    if (data && data.length === PAGE_SIZE) {
      setOffset((prev) => prev + PAGE_SIZE);
    }
  }, [data]);

  // Only show loading on initial load (no data yet)
  if (isLoading && !data) {
    return (
      <div className="flex items-center justify-center h-64">
        <span style={{ color: 'var(--ctp-subtext0)' }}>Loading events...</span>
      </div>
    );
  }

  if (error) {
    return (
      <div
        className="p-4 rounded-lg"
        style={{ backgroundColor: 'var(--ctp-surface0)', color: 'var(--ctp-red)' }}
      >
        Error loading events: {(error as Error).message}
      </div>
    );
  }

  const eventCount = data?.length ?? 0;
  const hasMore = eventCount === PAGE_SIZE;
  const currentPage = Math.floor(offset / PAGE_SIZE) + 1;

  return (
    <div className="flex flex-col h-full gap-4">
      {/* Header */}
      <div className="flex items-center justify-end gap-4">
          {/* Live toggle */}
          <button
            onClick={() => setIsLive(!isLive)}
            className="flex items-center gap-2 px-3 py-1 rounded text-sm min-w-[72px]"
            style={{
              backgroundColor: isLive ? 'var(--ctp-peach)' : 'var(--ctp-surface1)',
              color: isLive ? 'var(--ctp-crust)' : 'var(--ctp-text)',
            }}
          >
            <span
              className="inline-block w-2 h-2 rounded-full"
              style={{
                backgroundColor: isLive ? 'var(--ctp-crust)' : 'var(--ctp-overlay0)',
                opacity: isFetching ? 0.5 : 1,
              }}
            />
            {isLive ? 'Live' : 'Paused'}
          </button>

          <span className="text-sm" style={{ color: 'var(--ctp-subtext0)' }}>
            {filteredEvents.length} events
          </span>
          <span className="text-xs" style={{ color: 'var(--ctp-overlay0)' }}>
            Updated {new Date(dataUpdatedAt).toLocaleTimeString()}
          </span>
      </div>

      {/* Filters */}
      <FilterBar
        filters={filters}
        onFiltersChange={handleFiltersChange}
        searchInputRef={searchInputRef}
      />

      {/* Table */}
      <div className="flex-1 min-h-0">
        <VirtualizedEventTable
          events={filteredEvents}
          hasMore={false}
          selectedIndex={selectedIndex}
        />
      </div>

      {/* Pagination */}
      {(offset > 0 || hasMore) && (
        <div
          className="flex items-center justify-between pt-4 border-t"
          style={{ borderColor: 'var(--ctp-surface0)' }}
        >
          <div className="text-sm" style={{ color: 'var(--ctp-subtext0)' }}>
            Page {currentPage} ({eventCount} events)
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={handlePrevPage}
              disabled={offset === 0}
              className="px-3 py-1 rounded text-sm disabled:opacity-40"
              style={{
                backgroundColor: 'var(--ctp-surface0)',
                color: 'var(--ctp-text)',
              }}
            >
              Previous
            </button>
            <button
              onClick={handleNextPage}
              disabled={!hasMore}
              className="px-3 py-1 rounded text-sm disabled:opacity-40"
              style={{
                backgroundColor: 'var(--ctp-surface0)',
                color: 'var(--ctp-text)',
              }}
            >
              Next
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
