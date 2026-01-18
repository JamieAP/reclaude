import { useMemo, useCallback } from 'react';
import { Link } from 'react-router-dom';
import ReactMarkdown from 'react-markdown';
import type { Learning, SemanticLearningResult } from '../../api/types';
import { formatDate } from '../../utils/formatUtils';
import type { TimeRange } from '../../utils/dateUtils';

interface LearningsListViewProps {
  learnings: Learning[];
  searchInput: string;
  onSearchChange: (value: string) => void;
  search: string;
  timeRange: TimeRange;
  onTimeRangeChange: (range: TimeRange) => void;
  selectedRepo: string | null;
  onRepoChange: (repo: string | null) => void;
  repos: string[];
  similarTo: number | null;
  similarResults: SemanticLearningResult[];
  loadingSimilar: boolean;
  onFindSimilar: (learningId: number) => void;
}

export function LearningsListView({
  learnings,
  searchInput,
  onSearchChange,
  search,
  timeRange,
  onTimeRangeChange,
  selectedRepo,
  onRepoChange,
  repos,
  similarTo,
  similarResults,
  loadingSimilar,
  onFindSimilar,
}: LearningsListViewProps) {
  const filteredLearnings = useMemo(() => {
    if (!search) return learnings;
    const searchLower = search.toLowerCase();
    return learnings.filter(
      (l) =>
        l.content.toLowerCase().includes(searchLower) ||
        l.repo_name?.toLowerCase().includes(searchLower) ||
        l.branch?.toLowerCase().includes(searchLower)
    );
  }, [learnings, search]);

  const clearFilters = useCallback(() => {
    onSearchChange('');
    onRepoChange(null);
    onTimeRangeChange('all');
  }, [onSearchChange, onRepoChange, onTimeRangeChange]);

  const hasActiveFilters = search || selectedRepo || timeRange !== 'all';

  return (
    <>
      {/* Filters */}
      <div
        className="rounded-lg p-4 space-y-4"
        style={{ backgroundColor: 'var(--ctp-surface0)' }}
      >
        {/* Search */}
        <div>
          <label className="block text-xs mb-1" style={{ color: 'var(--ctp-subtext0)' }}>
            Search
          </label>
          <input
            type="text"
            value={searchInput}
            onChange={(e) => onSearchChange(e.target.value)}
            placeholder="Search learnings..."
            className="w-full px-3 py-2 rounded text-sm outline-none"
            style={{
              backgroundColor: 'var(--ctp-surface1)',
              color: 'var(--ctp-text)',
              border: '1px solid var(--ctp-surface2)',
            }}
          />
        </div>

        <div className="flex flex-wrap gap-4">
          {/* Time range */}
          <div>
            <label className="block text-xs mb-2" style={{ color: 'var(--ctp-subtext0)' }}>
              Time Range
            </label>
            <div className="flex gap-1">
              {(['all', '24h', '7d', '30d'] as const).map((range) => (
                <button
                  key={range}
                  onClick={() => onTimeRangeChange(range)}
                  className="px-2 py-1 rounded text-xs transition-colors"
                  style={{
                    backgroundColor:
                      timeRange === range ? 'var(--ctp-pink)' : 'var(--ctp-surface1)',
                    color: timeRange === range ? 'var(--ctp-crust)' : 'var(--ctp-text)',
                  }}
                >
                  {range === 'all' ? 'All' : range}
                </button>
              ))}
            </div>
          </div>

          {/* Repo filter */}
          {repos.length > 0 && (
            <div className="flex-1 min-w-48">
              <label className="block text-xs mb-2" style={{ color: 'var(--ctp-subtext0)' }}>
                Repository
              </label>
              <select
                value={selectedRepo ?? ''}
                onChange={(e) => onRepoChange(e.target.value || null)}
                className="w-full px-3 py-2 rounded text-sm outline-none"
                style={{
                  backgroundColor: 'var(--ctp-surface1)',
                  color: 'var(--ctp-text)',
                  border: '1px solid var(--ctp-surface2)',
                }}
              >
                <option value="">All repositories</option>
                {repos.map((repo) => (
                  <option key={repo} value={repo}>
                    {repo.split('/').slice(-2).join('/')}
                  </option>
                ))}
              </select>
            </div>
          )}
        </div>

        {/* Clear button */}
        {hasActiveFilters && (
          <div className="flex justify-end">
            <button
              onClick={clearFilters}
              className="px-3 py-1 rounded text-xs"
              style={{ color: 'var(--ctp-red)' }}
            >
              Clear all filters
            </button>
          </div>
        )}
      </div>

      {/* Learnings list */}
      <div className="flex-1 overflow-auto space-y-3">
        {filteredLearnings.map((learning) => (
          <div
            key={learning.id}
            className="rounded-lg p-4"
            style={{ backgroundColor: 'var(--ctp-surface0)' }}
          >
            <div className="flex items-start justify-between mb-2 gap-4">
              <Link
                to={`/learnings/${learning.id}`}
                className="text-xs font-mono hover:underline"
                style={{ color: 'var(--ctp-subtext0)' }}
              >
                {formatDate(learning.timestamp, 'dateWithYear')}
              </Link>
              <div className="flex items-center gap-2">
                <button
                  onClick={() => onFindSimilar(learning.id)}
                  className="text-xs px-2 py-1 rounded hover:opacity-80 transition-opacity"
                  style={{
                    backgroundColor:
                      similarTo === learning.id ? 'var(--ctp-mauve)' : 'var(--ctp-surface1)',
                    color:
                      similarTo === learning.id ? 'var(--ctp-crust)' : 'var(--ctp-subtext0)',
                  }}
                >
                  {loadingSimilar && similarTo === learning.id ? '...' : 'Find Similar'}
                </button>
                {learning.branch && (
                  <span
                    className="text-xs px-2 py-1 rounded font-mono"
                    style={{
                      backgroundColor: 'var(--ctp-surface1)',
                      color: 'var(--ctp-green)',
                    }}
                  >
                    {learning.branch}
                  </span>
                )}
                {learning.repo_name && (
                  <Link
                    to={`/repos/${encodeURIComponent(learning.remote_url ?? '')}`}
                    className="text-xs px-2 py-1 rounded hover:opacity-80"
                    style={{
                      backgroundColor: 'var(--ctp-surface1)',
                      color: 'var(--ctp-lavender)',
                    }}
                  >
                    {learning.repo_name}
                  </Link>
                )}
              </div>
            </div>

            <div className="prose prose-sm max-w-none" style={{ color: 'var(--ctp-text)' }}>
              <ReactMarkdown>{learning.content}</ReactMarkdown>
            </div>

            {similarTo === learning.id && similarResults.length > 0 && (
              <div
                className="mt-3 pt-3 space-y-2"
                style={{ borderTop: '1px solid var(--ctp-surface2)' }}
              >
                <div className="text-xs" style={{ color: 'var(--ctp-subtext0)' }}>
                  Similar learnings:
                </div>
                {similarResults.map((result) => (
                  <div
                    key={result.learning.id}
                    className="rounded p-2 text-sm"
                    style={{ backgroundColor: 'var(--ctp-surface1)' }}
                  >
                    <span
                      className="text-xs px-1 rounded mr-2"
                      style={{
                        backgroundColor: 'var(--ctp-surface2)',
                        color: 'var(--ctp-green)',
                      }}
                    >
                      {result.similarity_pct.toFixed(0)}%
                    </span>
                    {result.learning.content.slice(0, 150)}
                    {result.learning.content.length > 150 ? '...' : ''}
                  </div>
                ))}
              </div>
            )}
          </div>
        ))}

        {filteredLearnings.length === 0 && (
          <div
            className="text-center p-8 rounded-lg"
            style={{
              backgroundColor: 'var(--ctp-surface0)',
              color: 'var(--ctp-subtext0)',
            }}
          >
            {search ? 'No learnings match your search' : 'No learnings found'}
          </div>
        )}
      </div>
    </>
  );
}
