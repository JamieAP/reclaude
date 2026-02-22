// API response types

export interface SemanticEvent {
  id: number;
  timestamp: string;
  event_type: string;
  session_id: string | null;
  content: string;
  metadata: Record<string, unknown>;
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

