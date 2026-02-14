// API types matching Python Pydantic models

export interface SemanticEvent {
  id: number;
  timestamp: string;
  event_type: string;
  session_id: string | null;
  content: string;
  metadata: Record<string, unknown>;
}

export interface Learning {
  id: number;
  timestamp: string;
  session_id: string | null;
  cwd: string | null;
  zellij_session: string | null;
  content: string;
  repo_root: string | null;
  remote_url: string | null;
  repo_name: string | null;
  branch: string | null;
  is_worktree: boolean | null;
}

export interface LearningCreate {
  content: string;
  session_id?: string;
  cwd?: string;
  zellij_session?: string;
  repo_root?: string;
  remote_url?: string;
  repo_name?: string;
  branch?: string;
  is_worktree?: boolean;
}

export interface SessionInfo {
  session_id: string;
  last_event_at: string;
  event_count: number;
  cwd: string | null;
  // Extended stats
  repos: string[];
  branches: string[];
  lines_added: number;
  lines_removed: number;
  learnings_count: number;
  tool_use_count: number;
  file_diff_count: number;
  compaction_count: number;
  user_prompt_count: number;
  // Summary fields
  first_prompt: string | null;
  started_at: string | null;
  duration_minutes: number | null;
  files_modified: string[];
}

export interface RepoInfo {
  remote_url: string;
  repo_name: string | null;
  event_count: number;
  last_event_at: string;
}

export interface Statistics {
  total_events: number;
  events_by_type: Record<string, number>;
  total_sessions: number;
  total_learnings: number;
  event_types: string[];
  repos: RepoInfo[];
}

export interface PaginatedResponse<T> {
  items: T[];
  total: number;
  limit: number;
  offset: number;
  has_more: boolean;
}

// Query parameter types
export interface EventsQueryParams {
  event_type?: string[];
  session_id?: string;
  since?: string;
  cwd?: string;
  cwd_prefix?: boolean;
  limit?: number;
  offset?: number;
}

export interface LearningsQueryParams {
  cwd?: string;
  cwd_prefix?: boolean;
  zellij_session?: string;
  since?: string;
  repo_root?: string;
  remote_url?: string;
  limit?: number;
}

export interface LearningsAnalyticsParams {
  since?: string;
  remote_url?: string;
}

export interface FocusQueryParams {
  project?: string;
  time_scale?: string;
  limit?: number;
}

export interface FocusSnapshot {
  id: number;
  timestamp: string;
  project: string | null;
  time_scale: string;
  period_start: string;
  period_end: string;
  focus_summary: string;
  top_topics: string[];
  event_count: number;
  metadata: Record<string, unknown>;
}

// Unified timeline entry combining events and learnings
export type TimelineEntry =
  | { kind: 'event'; data: SemanticEvent }
  | { kind: 'learning'; data: Learning };

// Semantic search types
export interface SemanticSearchRequest {
  query: string;
  limit?: number;
  distance_threshold?: number;
  remote_url?: string;
  repo_root?: string;
  since?: string;
}

export interface SemanticLearningResult {
  learning: Learning;
  distance: number;
  similarity_pct: number;
}

// Learnings analytics types
export interface LearningsTimeSeriesPoint {
  date: string;
  count: number;
}

export interface LearningsAnalytics {
  total_count: number;
  by_repo: Record<string, number>;
  by_branch: Record<string, number>;
  time_series: LearningsTimeSeriesPoint[];
  avg_content_length: number;
  date_range_start: string | null;
  date_range_end: string | null;
}
