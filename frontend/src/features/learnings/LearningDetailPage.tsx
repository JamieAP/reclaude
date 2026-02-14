import { useParams, Link } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import ReactMarkdown from 'react-markdown';
import { api } from '../../api';
import type { SemanticLearningResult } from '../../api/types';
import { formatDate } from '../../utils/formatUtils';

export function LearningDetailPage() {
  const { id } = useParams<{ id: string }>();
  const learningId = Number(id);

  const { data: learnings } = useQuery({
    queryKey: ['learnings', { limit: 500 }],
    queryFn: () => api.learnings.list({ limit: 500 }),
  });

  const learning = learnings?.find((l) => l.id === learningId);

  const { data: similar } = useQuery({
    queryKey: ['learning-similar', learningId],
    queryFn: () => api.learnings.findSimilar(learningId, 10),
    enabled: !!learningId,
  });

  if (!learning) {
    return (
      <div className="p-4" style={{ color: 'var(--ctp-subtext0)' }}>
        Learning not found
      </div>
    );
  }

  return (
    <div className="max-w-4xl mx-auto space-y-6">
      {/* Breadcrumb */}
      <div className="text-sm" style={{ color: 'var(--ctp-subtext0)' }}>
        <Link to="/learnings" className="hover:underline">
          Learnings
        </Link>
        {' / '}
        <span>#{learning.id}</span>
      </div>

      {/* Main content */}
      <div className="rounded-lg p-6" style={{ backgroundColor: 'var(--ctp-surface0)' }}>
        <div className="flex items-center gap-4 mb-4">
          <span className="text-sm font-mono" style={{ color: 'var(--ctp-subtext0)' }}>
            {formatDate(learning.timestamp, 'full')}
          </span>
          {learning.branch && (
            <span
              className="text-xs px-2 py-1 rounded"
              style={{ backgroundColor: 'var(--ctp-surface1)', color: 'var(--ctp-green)' }}
            >
              {learning.branch}
            </span>
          )}
          {learning.repo_name && (
            <Link
              to={`/repos/${encodeURIComponent(learning.remote_url ?? '')}`}
              className="text-xs px-2 py-1 rounded hover:opacity-80"
              style={{ backgroundColor: 'var(--ctp-surface1)', color: 'var(--ctp-lavender)' }}
            >
              {learning.repo_name}
            </Link>
          )}
        </div>

        <div className="prose prose-lg max-w-none" style={{ color: 'var(--ctp-text)' }}>
          <ReactMarkdown>{learning.content}</ReactMarkdown>
        </div>

        {/* Metadata */}
        {(learning.cwd || learning.session_id) && (
          <div className="mt-6 pt-4 space-y-1" style={{ borderTop: '1px solid var(--ctp-surface2)' }}>
            {learning.cwd && (
              <div className="text-xs font-mono" style={{ color: 'var(--ctp-subtext0)' }}>
                Working directory: {learning.cwd}
              </div>
            )}
            {learning.session_id && (
              <div className="text-xs font-mono" style={{ color: 'var(--ctp-subtext0)' }}>
                Session: {learning.session_id}
              </div>
            )}
          </div>
        )}
      </div>

      {/* Similar learnings */}
      {similar && similar.length > 0 && (
        <div className="rounded-lg p-6" style={{ backgroundColor: 'var(--ctp-surface0)' }}>
          <h2 className="text-lg font-medium mb-4" style={{ color: 'var(--ctp-text)' }}>
            Related Learnings
          </h2>
          <div className="space-y-3">
            {similar.map((result: SemanticLearningResult) => (
              <Link
                key={result.learning.id}
                to={`/learnings/${result.learning.id}`}
                className="block rounded p-4 hover:opacity-90 transition-opacity"
                style={{ backgroundColor: 'var(--ctp-surface1)' }}
              >
                <div className="flex items-center gap-2 mb-2">
                  <span
                    className="text-xs px-2 py-0.5 rounded"
                    style={{
                      backgroundColor: 'var(--ctp-surface2)',
                      color: result.similarity_pct > 70 ? 'var(--ctp-green)' : 'var(--ctp-yellow)',
                    }}
                  >
                    {result.similarity_pct.toFixed(0)}% similar
                  </span>
                  <span className="text-xs" style={{ color: 'var(--ctp-subtext0)' }}>
                    {formatDate(result.learning.timestamp, 'dateOnly')}
                  </span>
                </div>
                <div className="text-sm line-clamp-2" style={{ color: 'var(--ctp-text)' }}>
                  {result.learning.content.slice(0, 200)}...
                </div>
              </Link>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
