import { useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import ReactMarkdown from 'react-markdown';
import { Link } from 'react-router-dom';
import { api } from '../../api';
import type { SemanticLearningResult } from '../../api/types';
import { formatDate } from '../../utils/formatUtils';

export function SemanticSearch() {
  const [query, setQuery] = useState('');

  const searchMutation = useMutation({
    mutationFn: (q: string) => api.learnings.semanticSearch({ query: q, limit: 20 }),
  });

  const handleSearch = (e: React.FormEvent) => {
    e.preventDefault();
    if (query.trim()) {
      searchMutation.mutate(query.trim());
    }
  };

  return (
    <div className="space-y-4">
      <form onSubmit={handleSearch} className="flex gap-2">
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search by meaning: 'error handling patterns' or 'authentication flow'..."
          className="flex-1 px-3 py-2 rounded text-sm outline-none"
          style={{
            backgroundColor: 'var(--ctp-surface1)',
            color: 'var(--ctp-text)',
            border: '1px solid var(--ctp-surface2)',
          }}
        />
        <button
          type="submit"
          disabled={searchMutation.isPending || !query.trim()}
          className="px-4 py-2 rounded text-sm font-medium transition-opacity disabled:opacity-50"
          style={{
            backgroundColor: 'var(--ctp-mauve)',
            color: 'var(--ctp-crust)',
          }}
        >
          {searchMutation.isPending ? 'Searching...' : 'Semantic Search'}
        </button>
      </form>

      {searchMutation.error && (
        <div
          className="p-3 rounded text-sm"
          style={{ backgroundColor: 'var(--ctp-surface0)', color: 'var(--ctp-red)' }}
        >
          {(searchMutation.error as Error).message}
        </div>
      )}

      {searchMutation.data && (
        <div className="space-y-2">
          <div className="text-xs" style={{ color: 'var(--ctp-subtext0)' }}>
            {searchMutation.data.length} results by semantic similarity
          </div>
          {searchMutation.data.map((result: SemanticLearningResult) => (
            <div
              key={result.learning.id}
              className="rounded-lg p-4"
              style={{ backgroundColor: 'var(--ctp-surface0)' }}
            >
              <div className="flex items-start justify-between mb-2 gap-4">
                <div className="flex items-center gap-2">
                  <span
                    className="text-xs px-2 py-1 rounded font-mono"
                    style={{
                      backgroundColor: 'var(--ctp-surface1)',
                      color: result.similarity_pct > 70 ? 'var(--ctp-green)' : 'var(--ctp-yellow)',
                    }}
                  >
                    {result.similarity_pct.toFixed(0)}% match
                  </span>
                  <span className="text-xs font-mono" style={{ color: 'var(--ctp-subtext0)' }}>
                    {formatDate(result.learning.timestamp, 'dateWithYear')}
                  </span>
                </div>
                <div className="flex items-center gap-2">
                  {result.learning.branch && (
                    <span
                      className="text-xs px-2 py-1 rounded font-mono"
                      style={{
                        backgroundColor: 'var(--ctp-surface1)',
                        color: 'var(--ctp-green)',
                      }}
                    >
                      {result.learning.branch}
                    </span>
                  )}
                  {result.learning.repo_name && (
                    <Link
                      to={`/repos/${encodeURIComponent(result.learning.remote_url ?? '')}`}
                      className="text-xs px-2 py-1 rounded hover:opacity-80"
                      style={{
                        backgroundColor: 'var(--ctp-surface1)',
                        color: 'var(--ctp-lavender)',
                      }}
                    >
                      {result.learning.repo_name}
                    </Link>
                  )}
                </div>
              </div>
              <div className="prose prose-sm max-w-none" style={{ color: 'var(--ctp-text)' }}>
                <ReactMarkdown>{result.learning.content}</ReactMarkdown>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
