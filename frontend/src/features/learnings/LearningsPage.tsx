import { useMemo, useReducer } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api } from '../../api';
import type { SemanticLearningResult } from '../../api/types';
import { SemanticSearch } from './SemanticSearch';
import { LearningsAnalytics } from './LearningsAnalytics';
import { LearningsListView } from './LearningsListView';
import { ExportLearnings } from './ExportLearnings';
import { useDebouncedState } from '../../hooks/useDebouncedState';
import { calculateSinceDate, type TimeRange } from '../../utils/dateUtils';

// State types
type ViewMode = 'list' | 'semantic' | 'analytics';

interface LearningsState {
  viewMode: ViewMode;
  selectedRepo: string | null;
  timeRange: TimeRange;
  similarTo: number | null;
  similarResults: SemanticLearningResult[];
  loadingSimilar: boolean;
}

// Action types
type LearningsAction =
  | { type: 'SET_VIEW_MODE'; payload: ViewMode }
  | { type: 'SET_REPO'; payload: string | null }
  | { type: 'SET_TIME_RANGE'; payload: TimeRange }
  | { type: 'START_FIND_SIMILAR'; payload: number }
  | { type: 'FINISH_FIND_SIMILAR'; payload: { id: number; results: SemanticLearningResult[] } }
  | { type: 'CLEAR_SIMILAR' };

const initialState: LearningsState = {
  viewMode: 'list',
  selectedRepo: null,
  timeRange: 'all',
  similarTo: null,
  similarResults: [],
  loadingSimilar: false,
};

function learningsReducer(state: LearningsState, action: LearningsAction): LearningsState {
  switch (action.type) {
    case 'SET_VIEW_MODE':
      return { ...state, viewMode: action.payload };
    case 'SET_REPO':
      return { ...state, selectedRepo: action.payload };
    case 'SET_TIME_RANGE':
      return { ...state, timeRange: action.payload };
    case 'START_FIND_SIMILAR':
      return { ...state, loadingSimilar: true };
    case 'FINISH_FIND_SIMILAR':
      return {
        ...state,
        loadingSimilar: false,
        similarTo: action.payload.id,
        similarResults: action.payload.results,
      };
    case 'CLEAR_SIMILAR':
      return { ...state, similarTo: null, similarResults: [], loadingSimilar: false };
    default:
      return state;
  }
}

export function LearningsPage() {
  const [state, dispatch] = useReducer(learningsReducer, initialState);
  const { viewMode, selectedRepo, timeRange, similarTo, similarResults, loadingSimilar } = state;

  const { value: searchInput, setValue: setSearchInput, debouncedValue: search } = useDebouncedState('');

  const since = useMemo(() => calculateSinceDate(timeRange), [timeRange]);

  const { data: learnings, isLoading, error } = useQuery({
    queryKey: ['learnings', { limit: 200, since, remote_url: selectedRepo }],
    queryFn: () =>
      api.learnings.list({
        limit: 200,
        since,
        remote_url: selectedRepo ?? undefined,
      }),
  });

  // Get unique repos for filtering
  const repos = useMemo(() => {
    if (!learnings) return [];
    const repoSet = new Set<string>();
    for (const l of learnings) {
      if (l.remote_url) repoSet.add(l.remote_url);
    }
    return Array.from(repoSet).sort();
  }, [learnings]);

  // Filter by search for count display
  const filteredLearnings = useMemo(() => {
    if (!learnings) return [];
    if (!search) return learnings;
    const searchLower = search.toLowerCase();
    return learnings.filter(
      (l) =>
        l.content.toLowerCase().includes(searchLower) ||
        l.repo_name?.toLowerCase().includes(searchLower) ||
        l.branch?.toLowerCase().includes(searchLower)
    );
  }, [learnings, search]);

  const handleFindSimilar = async (learningId: number) => {
    if (similarTo === learningId) {
      dispatch({ type: 'CLEAR_SIMILAR' });
      return;
    }
    dispatch({ type: 'START_FIND_SIMILAR', payload: learningId });
    try {
      const results = await api.learnings.findSimilar(learningId, 5);
      dispatch({ type: 'FINISH_FIND_SIMILAR', payload: { id: learningId, results } });
    } catch (e) {
      console.error('Failed to find similar:', e);
      dispatch({ type: 'CLEAR_SIMILAR' });
    }
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <span style={{ color: 'var(--ctp-subtext0)' }}>Loading learnings...</span>
      </div>
    );
  }

  if (error) {
    return (
      <div
        className="p-4 rounded-lg"
        style={{ backgroundColor: 'var(--ctp-surface0)', color: 'var(--ctp-red)' }}
      >
        Error loading learnings: {(error as Error).message}
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full gap-4">
      {/* Header with view mode tabs */}
      <div className="flex items-center justify-between">
        <div className="flex gap-2">
          {(['list', 'semantic', 'analytics'] as const).map((mode) => (
            <button
              key={mode}
              onClick={() => dispatch({ type: 'SET_VIEW_MODE', payload: mode })}
              className="px-3 py-1.5 rounded text-sm transition-colors"
              style={{
                backgroundColor: viewMode === mode ? 'var(--ctp-mauve)' : 'var(--ctp-surface1)',
                color: viewMode === mode ? 'var(--ctp-crust)' : 'var(--ctp-text)',
              }}
            >
              {mode === 'list' ? 'Browse' : mode === 'semantic' ? 'Semantic Search' : 'Analytics'}
            </button>
          ))}
        </div>
        {viewMode === 'list' && (
          <div className="flex items-center gap-4">
            <span className="text-sm" style={{ color: 'var(--ctp-subtext0)' }}>
              {filteredLearnings.length} of {learnings?.length ?? 0} items
            </span>
            <ExportLearnings learnings={filteredLearnings} />
          </div>
        )}
      </div>

      {/* View content */}
      {viewMode === 'semantic' ? (
        <SemanticSearch />
      ) : viewMode === 'analytics' ? (
        <LearningsAnalytics since={since} remoteUrl={selectedRepo ?? undefined} />
      ) : (
        <LearningsListView
          learnings={learnings ?? []}
          searchInput={searchInput}
          onSearchChange={setSearchInput}
          search={search}
          timeRange={timeRange}
          onTimeRangeChange={(range) => dispatch({ type: 'SET_TIME_RANGE', payload: range })}
          selectedRepo={selectedRepo}
          onRepoChange={(repo) => dispatch({ type: 'SET_REPO', payload: repo })}
          repos={repos}
          similarTo={similarTo}
          similarResults={similarResults}
          loadingSimilar={loadingSimilar}
          onFindSimilar={handleFindSimilar}
        />
      )}
    </div>
  );
}
