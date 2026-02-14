import { format, formatDistanceToNow, isToday } from 'date-fns';

/**
 * Standard date formats used across the app.
 */
export const DATE_FORMATS = {
  compact: 'MMM d, HH:mm',              // "Jan 16, 14:30"
  compactWithSeconds: 'MMM d, HH:mm:ss', // "Jan 16, 14:30:45"
  full: 'MMMM d, yyyy HH:mm:ss',        // "January 16, 2026 14:30:45"
  dateWithYear: 'MMM d, yyyy HH:mm',    // "Jan 16, 2026 14:30"
  dateOnly: 'MMM d, yyyy',              // "Jan 16, 2026"
  timeOnly: 'HH:mm:ss',                 // "14:30:45"
} as const;

export type DateFormatType = keyof typeof DATE_FORMATS;

/**
 * Format a date using one of the standard formats.
 */
export function formatDate(
  date: string | Date,
  formatType: DateFormatType = 'compact'
): string {
  return format(new Date(date), DATE_FORMATS[formatType]);
}

/**
 * Format a date relative to now (e.g., "5 minutes ago", "2 hours ago").
 */
export function formatRelative(date: string | Date): string {
  return formatDistanceToNow(new Date(date), { addSuffix: true });
}

/**
 * Format a date showing time-only if today, otherwise compact with date.
 * Useful for event tables where today's events don't need the date.
 */
export function formatSmartTimestamp(
  date: string | Date,
  includeSeconds = true
): string {
  const d = new Date(date);
  if (isToday(d)) {
    return format(d, DATE_FORMATS.timeOnly);
  }
  return format(d, includeSeconds ? DATE_FORMATS.compactWithSeconds : DATE_FORMATS.compact);
}

/**
 * Format a duration in minutes to human-readable string.
 * @param minutes - Duration in minutes (can be null)
 * @returns Formatted string like "2h 30m" or empty string if null/0
 */
export function formatDuration(minutes: number | null): string {
  if (minutes === null || minutes === 0) return '';
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const mins = minutes % 60;
  if (mins === 0) return `${hours}h`;
  return `${hours}h ${mins}m`;
}

/**
 * Format a duration in seconds to human-readable string.
 * @param seconds - Duration in seconds
 * @returns Formatted string like "2h 30m" or "45s"
 */
export function formatDurationSeconds(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = Math.floor(seconds % 60);

  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m ${secs}s`;
  return `${secs}s`;
}

/**
 * Human-friendly relative time (compact version).
 * Returns "just now", "5m ago", "2h ago", "3d ago", or localized date.
 */
export function formatRelativeCompact(dateStr: string): string {
  const date = new Date(dateStr);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMins = Math.floor(diffMs / 60000);
  const diffHours = Math.floor(diffMins / 60);
  const diffDays = Math.floor(diffHours / 24);

  if (diffMins < 1) return 'just now';
  if (diffMins < 60) return `${diffMins}m ago`;
  if (diffHours < 24) return `${diffHours}h ago`;
  if (diffDays < 7) return `${diffDays}d ago`;
  return date.toLocaleDateString();
}
