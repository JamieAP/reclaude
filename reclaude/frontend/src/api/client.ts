import type {
  SemanticEvent,
  Learning,
  LearningCreate,
  SessionInfo,
  Statistics,
  RepoInfo,
  FocusSnapshot,
  EventsQueryParams,
  LearningsQueryParams,
  LearningsAnalyticsParams,
  FocusQueryParams,
  SemanticSearchRequest,
  SemanticLearningResult,
  LearningsAnalytics,
} from './types';

const API_BASE = '/api';

async function fetchJson<T>(url: string, options?: RequestInit): Promise<T> {
  const response = await fetch(url, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      ...options?.headers,
    },
  });

  if (!response.ok) {
    const error = await response.text();
    throw new Error(`API error ${response.status}: ${error}`);
  }

  return response.json();
}

function buildQueryString(params: Record<string, unknown>): string {
  const searchParams = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null) {
      searchParams.set(key, String(value));
    }
  }
  const queryString = searchParams.toString();
  return queryString ? `?${queryString}` : '';
}

// Events
export async function getEvents(
  params: EventsQueryParams = {}
): Promise<SemanticEvent[]> {
  const query = buildQueryString(params as Record<string, unknown>);
  return fetchJson(`${API_BASE}/events${query}`);
}

export async function getEvent(id: number): Promise<SemanticEvent> {
  return fetchJson(`${API_BASE}/events/${id}`);
}

// Sessions
export async function getSessions(): Promise<SessionInfo[]> {
  return fetchJson(`${API_BASE}/sessions`);
}

export async function getSession(sessionId: string): Promise<SessionInfo> {
  return fetchJson(`${API_BASE}/sessions/${sessionId}`);
}

// Learnings
export async function getLearnings(
  params: LearningsQueryParams = {}
): Promise<Learning[]> {
  const query = buildQueryString(params as Record<string, unknown>);
  return fetchJson(`${API_BASE}/learnings${query}`);
}

export async function createLearning(data: LearningCreate): Promise<Learning> {
  return fetchJson(`${API_BASE}/learnings`, {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

export async function semanticSearchLearnings(
  request: SemanticSearchRequest
): Promise<SemanticLearningResult[]> {
  return fetchJson(`${API_BASE}/learnings/semantic/search`, {
    method: 'POST',
    body: JSON.stringify(request),
  });
}

export async function findSimilarLearnings(
  learningId: number,
  limit?: number
): Promise<SemanticLearningResult[]> {
  const query = buildQueryString({ limit });
  return fetchJson<SemanticLearningResult[]>(
    `${API_BASE}/learnings/semantic/${learningId}/similar${query}`
  );
}

export async function getLearningsAnalytics(
  params: LearningsAnalyticsParams = {}
): Promise<LearningsAnalytics> {
  const query = buildQueryString(params as Record<string, unknown>);
  return fetchJson(`${API_BASE}/learnings/analytics${query}`);
}

// Statistics
export async function getStatistics(): Promise<Statistics> {
  return fetchJson(`${API_BASE}/statistics`);
}

// Repos
export async function getRepos(): Promise<RepoInfo[]> {
  return fetchJson(`${API_BASE}/repos`);
}

export async function getRepoEvents(
  remoteUrl: string,
  params: Omit<EventsQueryParams, 'cwd'> = {}
): Promise<SemanticEvent[]> {
  const query = buildQueryString(params as Record<string, unknown>);
  return fetchJson(`${API_BASE}/repos/${encodeURIComponent(remoteUrl)}/events${query}`);
}

// Focus Snapshots
export async function getFocusSnapshots(
  params: FocusQueryParams = {}
): Promise<FocusSnapshot[]> {
  const query = buildQueryString(params as Record<string, unknown>);
  return fetchJson(`${API_BASE}/focus${query}`);
}

// Persona types
export interface PersonaUsageStats {
  agent_type: string;
  count: number;
  success_count: number;
  avg_duration_ms: number | null;
  persona: string | null;
}

export interface PersonaUsageResponse {
  days: number;
  total_task_calls: number;
  builtin_agents: PersonaUsageStats[];
  custom_personas: PersonaUsageStats[];
}

export interface PersonaTimelineEntry {
  date: string;
  agents: Record<string, number>;
}

export async function getPersonaUsage(days: number = 7): Promise<PersonaUsageResponse> {
  const query = buildQueryString({ days });
  return fetchJson(`${API_BASE}/personas/usage${query}`);
}

export async function getPersonaTimeline(days: number = 7): Promise<PersonaTimelineEntry[]> {
  const query = buildQueryString({ days });
  return fetchJson(`${API_BASE}/personas/timeline${query}`);
}

// API client object for convenience
export const api = {
  events: {
    list: getEvents,
    get: getEvent,
  },
  sessions: {
    list: getSessions,
    get: getSession,
  },
  learnings: {
    list: getLearnings,
    create: createLearning,
    semanticSearch: semanticSearchLearnings,
    findSimilar: findSimilarLearnings,
    getAnalytics: getLearningsAnalytics,
  },
  repos: {
    list: getRepos,
    events: getRepoEvents,
  },
  statistics: {
    get: getStatistics,
  },
  focus: {
    list: getFocusSnapshots,
  },
  personas: {
    usage: getPersonaUsage,
    timeline: getPersonaTimeline,
  },
};
