import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { api } from '../../api';
import { formatRelativeCompact } from '../../utils/formatUtils';

export function DashboardPage() {
  const { data: stats, isLoading, error } = useQuery({
    queryKey: ['statistics'],
    queryFn: api.statistics.get,
    refetchInterval: 5000,
  });

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <span style={{ color: 'var(--ctp-subtext0)' }}>Loading...</span>
      </div>
    );
  }

  if (error) {
    return (
      <div
        className="p-4 rounded-lg"
        style={{ backgroundColor: 'var(--ctp-surface0)', color: 'var(--ctp-red)' }}
      >
        Error loading statistics: {(error as Error).message}
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Stats cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        <StatCard label="Total Events" value={stats?.total_events ?? 0} color="var(--ctp-blue)" to="/events" />
        <StatCard label="Sessions" value={stats?.total_sessions ?? 0} color="var(--ctp-green)" to="/sessions" />
        <StatCard label="Repositories" value={stats?.repos?.length ?? 0} color="var(--ctp-peach)" />
      </div>

      {/* Repositories */}
      {stats?.repos && stats.repos.length > 0 && (
        <div className="rounded-lg p-6" style={{ backgroundColor: 'var(--ctp-surface0)' }}>
          <h2 className="text-lg font-medium mb-4" style={{ color: 'var(--ctp-text)' }}>
            Repositories
          </h2>
          <div className="space-y-2">
            {stats.repos.slice(0, 10).map((repo) => (
              <Link
                key={repo.remote_url}
                to={`/repos/${encodeURIComponent(repo.remote_url)}`}
                className="flex justify-between items-center p-3 rounded hover:opacity-80 transition-opacity"
                style={{ backgroundColor: 'var(--ctp-surface1)' }}
              >
                <div className="flex items-center gap-3 min-w-0">
                  <span className="text-lg">📁</span>
                  <div className="min-w-0">
                    <div className="font-medium truncate" style={{ color: 'var(--ctp-text)' }}>
                      {repo.repo_name || repo.remote_url}
                    </div>
                    <div className="text-xs truncate" style={{ color: 'var(--ctp-subtext0)' }}>
                      {repo.remote_url}
                    </div>
                  </div>
                </div>
                <div className="flex items-center gap-4 flex-shrink-0">
                  <span className="text-sm" style={{ color: 'var(--ctp-subtext1)' }}>
                    {repo.event_count.toLocaleString()} events
                  </span>
                  <span className="text-xs" style={{ color: 'var(--ctp-overlay0)' }}>
                    {formatRelativeCompact(repo.last_event_at)}
                  </span>
                </div>
              </Link>
            ))}
          </div>
        </div>
      )}

      {/* Events by type */}
      {stats?.events_by_type && Object.keys(stats.events_by_type).length > 0 && (
        <div className="rounded-lg p-6" style={{ backgroundColor: 'var(--ctp-surface0)' }}>
          <h2 className="text-lg font-medium mb-4" style={{ color: 'var(--ctp-text)' }}>
            Events by Type
          </h2>
          <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-5 gap-3">
            {Object.entries(stats.events_by_type)
              .sort(([, a], [, b]) => b - a)
              .map(([type, count]) => (
                <Link
                  key={type}
                  to={`/events?type=${encodeURIComponent(type)}`}
                  className="flex justify-between items-center p-3 rounded hover:opacity-80 transition-opacity"
                  style={{ backgroundColor: 'var(--ctp-surface1)' }}
                >
                  <span className="text-sm truncate" style={{ color: 'var(--ctp-subtext1)' }}>
                    {type}
                  </span>
                  <span className="text-sm font-medium ml-2" style={{ color: 'var(--ctp-text)' }}>
                    {count.toLocaleString()}
                  </span>
                </Link>
              ))}
          </div>
        </div>
      )}
    </div>
  );
}

function StatCard({
  label,
  value,
  color,
  to,
}: {
  label: string;
  value: number;
  color: string;
  to?: string;
}) {
  const content = (
    <>
      <div className="text-sm mb-2" style={{ color: 'var(--ctp-subtext0)' }}>
        {label}
      </div>
      <div className="text-3xl font-semibold" style={{ color }}>
        {value.toLocaleString()}
      </div>
    </>
  );

  if (to) {
    return (
      <Link
        to={to}
        className="rounded-lg p-6 block hover:opacity-80 transition-opacity"
        style={{ backgroundColor: 'var(--ctp-surface0)' }}
      >
        {content}
      </Link>
    );
  }

  return (
    <div
      className="rounded-lg p-6"
      style={{ backgroundColor: 'var(--ctp-surface0)' }}
    >
      {content}
    </div>
  );
}
