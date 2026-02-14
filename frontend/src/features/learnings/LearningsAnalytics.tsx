import { useQuery } from '@tanstack/react-query';
import {
  AreaChart,
  Area,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  PieChart,
  Pie,
  Cell,
} from 'recharts';
import { api } from '../../api';

const COLORS = [
  'var(--ctp-mauve)',
  'var(--ctp-pink)',
  'var(--ctp-flamingo)',
  'var(--ctp-rosewater)',
  'var(--ctp-peach)',
  'var(--ctp-yellow)',
  'var(--ctp-green)',
  'var(--ctp-teal)',
];

interface Props {
  since?: string;
  remoteUrl?: string;
}

export function LearningsAnalytics({ since, remoteUrl }: Props) {
  const { data, isLoading, error } = useQuery({
    queryKey: ['learnings-analytics', since, remoteUrl],
    queryFn: () => api.learnings.getAnalytics({ since, remote_url: remoteUrl }),
  });

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-48">
        <span style={{ color: 'var(--ctp-subtext0)' }}>Loading analytics...</span>
      </div>
    );
  }

  if (error || !data) {
    return (
      <div className="p-4 rounded" style={{ backgroundColor: 'var(--ctp-surface0)' }}>
        <span style={{ color: 'var(--ctp-red)' }}>Failed to load analytics</span>
      </div>
    );
  }

  const repoData = Object.entries(data.by_repo)
    .sort((a, b) => b[1] - a[1])
    .slice(0, 8)
    .map(([name, value]) => ({ name, value }));

  return (
    <div className="space-y-6">
      {/* Summary stats */}
      <div className="grid grid-cols-3 gap-4">
        <div className="rounded-lg p-4" style={{ backgroundColor: 'var(--ctp-surface0)' }}>
          <div className="text-2xl font-bold" style={{ color: 'var(--ctp-mauve)' }}>
            {data.total_count}
          </div>
          <div className="text-xs" style={{ color: 'var(--ctp-subtext0)' }}>
            Total Learnings
          </div>
        </div>
        <div className="rounded-lg p-4" style={{ backgroundColor: 'var(--ctp-surface0)' }}>
          <div className="text-2xl font-bold" style={{ color: 'var(--ctp-pink)' }}>
            {Object.keys(data.by_repo).length}
          </div>
          <div className="text-xs" style={{ color: 'var(--ctp-subtext0)' }}>
            Repositories
          </div>
        </div>
        <div className="rounded-lg p-4" style={{ backgroundColor: 'var(--ctp-surface0)' }}>
          <div className="text-2xl font-bold" style={{ color: 'var(--ctp-peach)' }}>
            {Math.round(data.avg_content_length)}
          </div>
          <div className="text-xs" style={{ color: 'var(--ctp-subtext0)' }}>
            Avg. Characters
          </div>
        </div>
      </div>

      {/* Time series chart */}
      {data.time_series.length > 1 && (
        <div className="rounded-lg p-4" style={{ backgroundColor: 'var(--ctp-surface0)' }}>
          <h3 className="text-sm font-medium mb-4" style={{ color: 'var(--ctp-text)' }}>
            Learnings Over Time
          </h3>
          <ResponsiveContainer width="100%" height={200}>
            <AreaChart data={data.time_series}>
              <XAxis
                dataKey="date"
                tick={{ fill: 'var(--ctp-subtext0)', fontSize: 10 }}
                tickFormatter={(d) => d.slice(5)}
              />
              <YAxis tick={{ fill: 'var(--ctp-subtext0)', fontSize: 10 }} />
              <Tooltip
                contentStyle={{
                  backgroundColor: 'var(--ctp-surface1)',
                  border: 'none',
                  borderRadius: '8px',
                }}
                labelStyle={{ color: 'var(--ctp-text)' }}
              />
              <Area
                type="monotone"
                dataKey="count"
                stroke="var(--ctp-mauve)"
                fill="var(--ctp-mauve)"
                fillOpacity={0.3}
              />
            </AreaChart>
          </ResponsiveContainer>
        </div>
      )}

      {/* Repo distribution */}
      {repoData.length > 0 && (
        <div className="rounded-lg p-4" style={{ backgroundColor: 'var(--ctp-surface0)' }}>
          <h3 className="text-sm font-medium mb-4" style={{ color: 'var(--ctp-text)' }}>
            By Repository
          </h3>
          <div className="flex items-center gap-8">
            <ResponsiveContainer width={150} height={150}>
              <PieChart>
                <Pie
                  data={repoData}
                  dataKey="value"
                  nameKey="name"
                  cx="50%"
                  cy="50%"
                  outerRadius={60}
                  innerRadius={30}
                >
                  {repoData.map((_, i) => (
                    <Cell key={i} fill={COLORS[i % COLORS.length]} />
                  ))}
                </Pie>
              </PieChart>
            </ResponsiveContainer>
            <div className="flex-1 space-y-1">
              {repoData.map((item, i) => (
                <div key={item.name} className="flex items-center gap-2 text-xs">
                  <div
                    className="w-3 h-3 rounded-sm"
                    style={{ backgroundColor: COLORS[i % COLORS.length] }}
                  />
                  <span style={{ color: 'var(--ctp-text)' }}>{item.name}</span>
                  <span style={{ color: 'var(--ctp-subtext0)' }}>({item.value})</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
