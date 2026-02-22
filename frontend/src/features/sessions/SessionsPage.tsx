import { useState, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { api } from '../../api';
import { useDebouncedState } from '../../hooks/useDebouncedState';
import { formatDate, formatDuration, formatRelative } from '../../utils/formatUtils';

type SortKey = 'recent' | 'events' | 'oldest';

export function SessionsPage() {
  const { value: searchInput, setValue: setSearchInput, debouncedValue: search } = useDebouncedState('');
  const [sortBy, setSortBy] = useState<SortKey>('recent');

  const { data: sessions, isLoading, error } = useQuery({
    queryKey: ['sessions'],
    queryFn: api.sessions.list,
    refetchInterval: 5000,
  });

  // Filter and sort sessions
  const filteredSessions = useMemo(() => {
    let result = sessions ?? [];

    // Search filter
    if (search) {
      const searchLower = search.toLowerCase();
      result = result.filter(
        (s) =>
          s.session_id.toLowerCase().includes(searchLower) ||
          s.cwd?.toLowerCase().includes(searchLower)
      );
    }

    // Sort
    switch (sortBy) {
      case 'recent':
        result = [...result].sort(
          (a, b) => new Date(b.last_event_at).getTime() - new Date(a.last_event_at).getTime()
        );
        break;
      case 'oldest':
        result = [...result].sort(
          (a, b) => new Date(a.last_event_at).getTime() - new Date(b.last_event_at).getTime()
        );
        break;
      case 'events':
        result = [...result].sort((a, b) => b.event_count - a.event_count);
        break;
    }

    return result;
  }, [sessions, search, sortBy]);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <span style={{ color: 'var(--ctp-subtext0)' }}>Loading sessions...</span>
      </div>
    );
  }

  if (error) {
    return (
      <div
        className="p-4 rounded-lg"
        style={{ backgroundColor: 'var(--ctp-surface0)', color: 'var(--ctp-red)' }}
      >
        Error loading sessions: {(error as Error).message}
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full gap-4">
      {/* Header */}
      <div className="flex items-center justify-end">
        <span className="text-sm" style={{ color: 'var(--ctp-subtext0)' }}>
          {filteredSessions.length} of {sessions?.length ?? 0} sessions
        </span>
      </div>

      {/* Filters */}
      <div
        className="rounded-lg p-4 flex flex-wrap gap-4"
        style={{ backgroundColor: 'var(--ctp-surface0)' }}
      >
        {/* Search */}
        <div className="flex-1 min-w-48">
          <label className="block text-xs mb-1" style={{ color: 'var(--ctp-subtext0)' }}>
            Search
          </label>
          <input
            type="text"
            value={searchInput}
            onChange={(e) => setSearchInput(e.target.value)}
            placeholder="Search by session ID or directory..."
            className="w-full px-3 py-2 rounded text-sm outline-none"
            style={{
              backgroundColor: 'var(--ctp-surface1)',
              color: 'var(--ctp-text)',
              border: '1px solid var(--ctp-surface2)',
            }}
          />
        </div>

        {/* Sort */}
        <div>
          <label className="block text-xs mb-1" style={{ color: 'var(--ctp-subtext0)' }}>
            Sort by
          </label>
          <div className="flex gap-1">
            {([
              { key: 'recent', label: 'Recent' },
              { key: 'events', label: 'Most Events' },
              { key: 'oldest', label: 'Oldest' },
            ] as const).map(({ key, label }) => (
              <button
                key={key}
                onClick={() => setSortBy(key)}
                className="px-2 py-1 rounded text-xs transition-colors"
                style={{
                  backgroundColor: sortBy === key ? 'var(--ctp-green)' : 'var(--ctp-surface1)',
                  color: sortBy === key ? 'var(--ctp-crust)' : 'var(--ctp-text)',
                }}
              >
                {label}
              </button>
            ))}
          </div>
        </div>
      </div>

      {/* Sessions list */}
      <div className="flex-1 overflow-auto space-y-2">
        {filteredSessions.map((session) => {
          const isRecent =
            new Date().getTime() - new Date(session.last_event_at).getTime() < 3600000; // 1 hour

          // Extract repo name from URL
          const repoName = session.repos?.[0]?.split('/').slice(-2).join('/').replace('.git', '') ?? null;
          const duration = formatDuration(session.duration_minutes);
          const startedAt = session.started_at ? formatDate(session.started_at, 'compact') : null;

          return (
            <Link
              key={session.session_id}
              to={`/events?session_id=${session.session_id}`}
              className="block rounded-lg p-4 hover:opacity-90 transition-opacity"
              style={{ backgroundColor: 'var(--ctp-surface0)' }}
            >
              {/* Top row: session ID, repo, branches, time */}
              <div className="flex items-center justify-between gap-4 mb-1">
                <div className="flex items-center gap-3 min-w-0">
                  {/* Activity indicator */}
                  <div
                    className={`w-2 h-2 rounded-full flex-shrink-0 ${isRecent ? 'animate-pulse' : ''}`}
                    style={{
                      backgroundColor: isRecent ? 'var(--ctp-green)' : 'var(--ctp-overlay0)',
                    }}
                  />

                  {/* Session ID */}
                  <span
                    className="text-sm font-mono"
                    style={{ color: 'var(--ctp-lavender)' }}
                    title={session.session_id}
                  >
                    {session.session_id.slice(0, 8)}
                  </span>

                  {/* Repo name */}
                  {repoName && (
                    <span
                      className="text-sm font-medium truncate"
                      style={{ color: 'var(--ctp-peach)' }}
                      title={session.repos[0]}
                    >
                      {repoName}
                    </span>
                  )}

                  {/* Branches */}
                  {session.branches?.length > 0 && (
                    <div className="hidden md:flex items-center gap-1">
                      {session.branches.slice(0, 2).map((branch) => (
                        <span
                          key={branch}
                          className="text-xs px-1.5 py-0.5 rounded font-mono"
                          style={{
                            backgroundColor: 'var(--ctp-surface1)',
                            color: 'var(--ctp-green)',
                          }}
                        >
                          {branch}
                        </span>
                      ))}
                      {session.branches.length > 2 && (
                        <span className="text-xs" style={{ color: 'var(--ctp-overlay0)' }}>
                          +{session.branches.length - 2}
                        </span>
                      )}
                    </div>
                  )}
                </div>

                {/* Time info */}
                <div className="flex items-center gap-2 text-xs flex-shrink-0" style={{ color: 'var(--ctp-subtext0)' }}>
                  {duration && (
                    <span
                      className="px-1.5 py-0.5 rounded"
                      style={{ backgroundColor: 'var(--ctp-surface1)' }}
                      title={`Session duration: ${duration}`}
                    >
                      {duration}
                    </span>
                  )}
                  <span title={startedAt ?? undefined}>
                    {formatRelative(session.last_event_at)}
                  </span>
                </div>
              </div>

              {/* First prompt - main description */}
              {session.first_prompt && (
                <div
                  className="text-sm mb-2 truncate"
                  style={{ color: 'var(--ctp-text)' }}
                  title={session.first_prompt}
                >
                  {session.first_prompt}
                </div>
              )}

              {/* Bottom row: stats */}
              <div className="flex items-center gap-3 text-xs flex-wrap" style={{ color: 'var(--ctp-subtext0)' }}>
                {/* Directory */}
                {session.cwd && (
                  <span
                    className="font-mono truncate max-w-48"
                    style={{ color: 'var(--ctp-overlay0)' }}
                    title={session.cwd}
                  >
                    {session.cwd.split('/').slice(-2).join('/')}
                  </span>
                )}

                <span style={{ color: 'var(--ctp-surface2)' }}>·</span>

                {/* Event count */}
                <span>{session.event_count.toLocaleString()} events</span>

                {/* Line changes */}
                {(session.lines_added > 0 || session.lines_removed > 0) && (
                  <>
                    <span style={{ color: 'var(--ctp-surface2)' }}>·</span>
                    <span>
                      <span style={{ color: 'var(--ctp-green)' }}>+{session.lines_added}</span>
                      {' / '}
                      <span style={{ color: 'var(--ctp-red)' }}>-{session.lines_removed}</span>
                    </span>
                  </>
                )}

                {/* Files modified */}
                {session.files_modified?.length > 0 && (
                  <>
                    <span style={{ color: 'var(--ctp-surface2)' }}>·</span>
                    <span
                      style={{ color: 'var(--ctp-blue)' }}
                      title={session.files_modified.join('\n')}
                    >
                      {session.files_modified.length} file{session.files_modified.length !== 1 ? 's' : ''}
                    </span>
                  </>
                )}

                {/* Compactions */}
                {session.compaction_count > 0 && (
                  <>
                    <span style={{ color: 'var(--ctp-surface2)' }}>·</span>
                    <span style={{ color: 'var(--ctp-yellow)' }}>
                      {session.compaction_count} compaction{session.compaction_count !== 1 ? 's' : ''}
                    </span>
                  </>
                )}

                {/* Tool uses */}
                {session.tool_use_count > 0 && (
                  <>
                    <span style={{ color: 'var(--ctp-surface2)' }}>·</span>
                    <span>{session.tool_use_count} tools</span>
                  </>
                )}
              </div>
            </Link>
          );
        })}

        {filteredSessions.length === 0 && (
          <div
            className="text-center p-8 rounded-lg"
            style={{
              backgroundColor: 'var(--ctp-surface0)',
              color: 'var(--ctp-subtext0)',
            }}
          >
            {search ? 'No sessions match your search' : 'No sessions found'}
          </div>
        )}
      </div>
    </div>
  );
}
