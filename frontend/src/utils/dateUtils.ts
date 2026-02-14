/**
 * Utility for calculating "since" dates from time range filters.
 */

export type TimeRange = '1h' | '24h' | '7d' | '30d' | 'all';

const TIME_RANGE_MS: Record<Exclude<TimeRange, 'all'>, number> = {
  '1h': 60 * 60 * 1000,
  '24h': 24 * 60 * 60 * 1000,
  '7d': 7 * 24 * 60 * 60 * 1000,
  '30d': 30 * 24 * 60 * 60 * 1000,
};

/**
 * Convert a time range to an ISO date string for "since" queries.
 * Returns undefined for 'all' (no filter).
 */
export function calculateSinceDate(timeRange: TimeRange): string | undefined {
  if (timeRange === 'all') return undefined;
  return new Date(Date.now() - TIME_RANGE_MS[timeRange]).toISOString();
}
