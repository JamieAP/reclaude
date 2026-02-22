/**
 * Semantic color mapping for event type badges.
 * Uses "Ink and Parchment" palette with semantic clustering:
 * - Human input: warm (terracotta family)
 * - AI output: cool (slate family)
 * - Tools: earthy (moss/olive family)
 * - System: neutral (gray family)
 */

export const EVENT_TYPE_COLORS: Record<string, { bg: string; text: string }> = {
  // Human input - warm terracotta family
  user_prompt: { bg: 'var(--rc-badge-user-prompt-bg)', text: 'var(--rc-badge-user-prompt-text)' },

  // AI response - cool slate family
  assistant: { bg: 'var(--rc-badge-assistant-bg)', text: 'var(--rc-badge-assistant-text)' },
  plan: { bg: 'var(--rc-badge-plan-bg)', text: 'var(--rc-badge-plan-text)' },
  thinking: { bg: 'var(--rc-badge-thinking-bg)', text: 'var(--rc-badge-thinking-text)' },

  // Tool execution - earthy green family
  tool_use: { bg: 'var(--rc-badge-tool-bg)', text: 'var(--rc-badge-tool-text)' },
  tool_result: { bg: 'var(--rc-badge-tool-bg)', text: 'var(--rc-badge-tool-text)' },
  file_diff: { bg: 'var(--rc-badge-file-diff-bg)', text: 'var(--rc-badge-file-diff-text)' },

  // Session lifecycle - neutral
  session_start: { bg: 'var(--rc-badge-session-bg)', text: 'var(--rc-badge-session-text)' },
  session_end: { bg: 'var(--rc-badge-session-bg)', text: 'var(--rc-badge-session-text)' },
  compaction: { bg: 'var(--rc-badge-compaction-bg)', text: 'var(--rc-badge-compaction-text)' },
  subagent_stop: { bg: 'var(--rc-badge-session-bg)', text: 'var(--rc-badge-session-text)' },

  // System & meta - neutral
  sys_msg: { bg: 'var(--rc-badge-notification-bg)', text: 'var(--rc-badge-notification-text)' },
  notification: { bg: 'var(--rc-badge-notification-bg)', text: 'var(--rc-badge-notification-text)' },
  plan_file: { bg: 'var(--rc-badge-plan-bg)', text: 'var(--rc-badge-plan-text)' },
  permission_request: { bg: 'var(--rc-badge-permission-bg)', text: 'var(--rc-badge-permission-text)' },

  // Special
  error: { bg: 'var(--rc-badge-error-bg)', text: 'var(--rc-badge-error-text)' },
};

export const DEFAULT_EVENT_COLOR = { bg: 'var(--rc-surface-elevated)', text: 'var(--rc-text-secondary)' };

export function getEventTypeColor(type: string): { bg: string; text: string } {
  return EVENT_TYPE_COLORS[type] ?? DEFAULT_EVENT_COLOR;
}
