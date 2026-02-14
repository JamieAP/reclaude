import { useEffect, useMemo } from 'react';
import { format } from 'date-fns';
import type { SemanticEvent } from '../api';
import { getEventTypeColor } from '../utils/eventTypeColors';

interface ParsedToolUse {
  toolName: string;
  success: boolean;
  duration: string;
  input: Record<string, unknown> | null;
  output: string;
  bashCommand?: string;
}

function parseToolUse(content: string): ParsedToolUse | null {
  try {
    const lines = content.split('\n');
    const toolLine = lines.find((l) => l.startsWith('Tool:'));
    const successLine = lines.find((l) => l.startsWith('Success:'));
    const durationLine = lines.find((l) => l.startsWith('Duration:'));

    if (!toolLine) return null;

    const toolName = toolLine.replace('Tool:', '').trim();
    const success = successLine?.includes('True') ?? false;
    const duration = durationLine?.replace('Duration:', '').trim() ?? '';

    // Extract INPUT section
    const inputStart = content.indexOf('--- INPUT ---');
    const outputStart = content.indexOf('--- OUTPUT ---');
    let input: Record<string, unknown> | null = null;
    let output = '';
    let bashCommand: string | undefined;

    if (inputStart !== -1 && outputStart !== -1) {
      const inputStr = content.slice(inputStart + 13, outputStart).trim();
      try {
        input = JSON.parse(inputStr);
        // Extract bash command if this is a Bash tool
        if (toolName === 'Bash' && input && 'command' in input) {
          bashCommand = String(input.command);
        }
      } catch {
        // Not valid JSON
      }
      output = content.slice(outputStart + 14).trim();
    }

    return { toolName, success, duration, input, output, bashCommand };
  } catch {
    return null;
  }
}

interface EventDetailModalProps {
  event: SemanticEvent;
  onClose: () => void;
}

export function EventDetailModal({ event, onClose }: EventDetailModalProps) {
  // Close on Escape key
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onClose]);

  const isFileDiff = event.event_type === 'file_diff';
  const isToolUse = event.event_type === 'tool_use';
  const parsedTool = useMemo(
    () => (isToolUse ? parseToolUse(event.content) : null),
    [isToolUse, event.content]
  );

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4"
      style={{ backgroundColor: 'rgba(0, 0, 0, 0.6)' }}
      onClick={onClose}
    >
      <div
        className="w-full max-w-4xl max-h-[90vh] rounded-lg flex flex-col overflow-hidden shadow-2xl"
        style={{
          backgroundColor: 'var(--ctp-base)',
          border: '1px solid var(--ctp-surface1)',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div
          className="flex items-center justify-between px-6 py-4 border-b"
          style={{
            backgroundColor: 'var(--ctp-mantle)',
            borderColor: 'var(--ctp-surface0)',
          }}
        >
          <div className="flex items-center gap-3">
            <EventTypeBadge type={event.event_type} />
            <span className="text-sm font-mono" style={{ color: 'var(--ctp-subtext0)' }}>
              ID: {event.id}
            </span>
          </div>
          <button
            onClick={onClose}
            className="text-xl px-2 hover:opacity-70"
            style={{ color: 'var(--ctp-overlay0)' }}
          >
            ×
          </button>
        </div>

        {/* Body */}
        <div className="flex-1 overflow-auto p-6 space-y-6">
          {/* Timestamp and Session */}
          <div className="flex gap-6">
            <div>
              <div className="text-xs mb-1" style={{ color: 'var(--ctp-subtext0)' }}>
                Timestamp
              </div>
              <div className="text-sm font-mono" style={{ color: 'var(--ctp-text)' }}>
                {format(new Date(event.timestamp), 'PPpp')}
              </div>
            </div>
            {event.session_id && (
              <div>
                <div className="text-xs mb-1" style={{ color: 'var(--ctp-subtext0)' }}>
                  Session
                </div>
                <div className="text-sm font-mono" style={{ color: 'var(--ctp-lavender)' }}>
                  {event.session_id.slice(0, 16)}...
                </div>
              </div>
            )}
          </div>

          {/* Content */}
          <div>
            <div className="text-xs mb-2" style={{ color: 'var(--ctp-subtext0)' }}>
              Content
            </div>
            {isFileDiff ? (
              <DiffViewer content={event.content} />
            ) : isToolUse && parsedTool ? (
              <ToolUseViewer tool={parsedTool} />
            ) : (
              <div
                className="p-4 rounded text-sm font-mono whitespace-pre-wrap overflow-auto max-h-80"
                style={{
                  backgroundColor: 'var(--ctp-surface0)',
                  color: 'var(--ctp-text)',
                }}
              >
                {event.content}
              </div>
            )}
          </div>

          {/* Metadata */}
          {Object.keys(event.metadata).length > 0 && (
            <div>
              <div className="text-xs mb-2" style={{ color: 'var(--ctp-subtext0)' }}>
                Metadata
              </div>
              <MetadataViewer metadata={event.metadata} />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function ToolUseViewer({ tool }: { tool: ParsedToolUse }) {
  return (
    <div className="space-y-4">
      {/* Header with tool info */}
      <div
        className="flex items-center gap-4 p-4 rounded"
        style={{ backgroundColor: 'var(--ctp-surface0)' }}
      >
        <div className="flex items-center gap-2">
          <span
            className="px-2 py-1 rounded text-sm font-medium"
            style={{ backgroundColor: 'var(--ctp-teal)', color: 'var(--ctp-crust)' }}
          >
            {tool.toolName}
          </span>
        </div>
        <div className="flex items-center gap-4 text-sm">
          <span
            className="flex items-center gap-1"
            style={{ color: tool.success ? 'var(--ctp-green)' : 'var(--ctp-red)' }}
          >
            <span>{tool.success ? '✓' : '✗'}</span>
            {tool.success ? 'Success' : 'Failed'}
          </span>
          {tool.duration && (
            <span style={{ color: 'var(--ctp-subtext0)' }}>
              {tool.duration}
            </span>
          )}
        </div>
      </div>

      {/* Bash command (if present) */}
      {tool.bashCommand && (
        <div>
          <div className="text-xs mb-2" style={{ color: 'var(--ctp-subtext0)' }}>
            Command
          </div>
          <div
            className="p-4 rounded text-sm font-mono overflow-auto"
            style={{ backgroundColor: 'var(--ctp-surface0)', color: 'var(--ctp-peach)' }}
          >
            $ {tool.bashCommand}
          </div>
        </div>
      )}

      {/* Input parameters */}
      {tool.input && !tool.bashCommand && (
        <div>
          <div className="text-xs mb-2" style={{ color: 'var(--ctp-subtext0)' }}>
            Input
          </div>
          <div
            className="p-4 rounded text-sm font-mono overflow-auto max-h-48"
            style={{ backgroundColor: 'var(--ctp-surface0)', color: 'var(--ctp-text)' }}
          >
            {Object.entries(tool.input).map(([key, value]) => (
              <div key={key} className="mb-2 last:mb-0">
                <span style={{ color: 'var(--ctp-blue)' }}>{key}:</span>{' '}
                <span style={{ color: 'var(--ctp-text)' }}>
                  {typeof value === 'string'
                    ? value.length > 200
                      ? value.slice(0, 200) + '...'
                      : value
                    : JSON.stringify(value)}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Output */}
      {tool.output && (
        <div>
          <div className="text-xs mb-2" style={{ color: 'var(--ctp-subtext0)' }}>
            Output
          </div>
          <div
            className="p-4 rounded text-sm font-mono whitespace-pre-wrap overflow-auto max-h-64"
            style={{ backgroundColor: 'var(--ctp-crust)', color: 'var(--ctp-text)' }}
          >
            {tool.output.slice(0, 2000)}
            {tool.output.length > 2000 && '...'}
          </div>
        </div>
      )}
    </div>
  );
}

function DiffViewer({ content }: { content: string }) {
  const lines = content.split('\n');

  return (
    <div
      className="rounded overflow-auto max-h-96 text-sm font-mono"
      style={{ backgroundColor: 'var(--ctp-crust)' }}
    >
      {lines.map((line, i) => {
        let bgColor = 'transparent';
        let textColor = 'var(--ctp-text)';

        if (line.startsWith('+') && !line.startsWith('+++')) {
          bgColor = 'rgba(166, 227, 161, 0.15)'; // Green tint
          textColor = 'var(--ctp-green)';
        } else if (line.startsWith('-') && !line.startsWith('---')) {
          bgColor = 'rgba(243, 139, 168, 0.15)'; // Red tint
          textColor = 'var(--ctp-red)';
        } else if (line.startsWith('@@')) {
          textColor = 'var(--ctp-blue)';
        } else if (line.startsWith('diff') || line.startsWith('index')) {
          textColor = 'var(--ctp-overlay0)';
        }

        return (
          <div
            key={i}
            className="px-4 py-0.5"
            style={{ backgroundColor: bgColor, color: textColor }}
          >
            <span className="inline-block w-10 text-right mr-4 select-none" style={{ color: 'var(--ctp-overlay0)' }}>
              {i + 1}
            </span>
            {line}
          </div>
        );
      })}
    </div>
  );
}

function MetadataViewer({ metadata }: { metadata: Record<string, unknown> }) {
  // Keys to show at the top in a condensed format
  const priorityKeys = ['repo_name', 'branch', 'remote_url', 'cwd'];
  const hiddenKeys = ['transcript_path', 'prompt_event_id']; // Less useful keys to hide

  const entries = Object.entries(metadata).filter(([key]) => !hiddenKeys.includes(key));
  const priorityEntries = entries.filter(([key]) => priorityKeys.includes(key));
  const otherEntries = entries.filter(([key]) => !priorityKeys.includes(key));

  const formatValue = (value: unknown): string => {
    if (typeof value === 'string') {
      // Truncate long strings
      return value.length > 60 ? value.slice(0, 60) + '...' : value;
    }
    if (typeof value === 'boolean') return value ? 'true' : 'false';
    if (typeof value === 'number') return String(value);
    return JSON.stringify(value);
  };

  return (
    <div
      className="p-4 rounded text-sm overflow-auto max-h-48"
      style={{ backgroundColor: 'var(--ctp-surface0)' }}
    >
      {/* Priority metadata in a compact row */}
      {priorityEntries.length > 0 && (
        <div className="flex flex-wrap gap-3 mb-3 pb-3 border-b" style={{ borderColor: 'var(--ctp-surface1)' }}>
          {priorityEntries.map(([key, value]) => (
            <div key={key} className="flex items-center gap-1">
              <span className="text-xs" style={{ color: 'var(--ctp-subtext0)' }}>{key}:</span>
              <span className="text-xs font-mono" style={{ color: 'var(--ctp-lavender)' }}>
                {formatValue(value)}
              </span>
            </div>
          ))}
        </div>
      )}

      {/* Other metadata in a grid */}
      <div className="grid grid-cols-1 gap-1">
        {otherEntries.map(([key, value]) => (
          <div key={key} className="flex gap-2 font-mono">
            <span className="text-xs flex-shrink-0" style={{ color: 'var(--ctp-blue)' }}>
              {key}:
            </span>
            <span className="text-xs truncate" style={{ color: 'var(--ctp-overlay1)' }}>
              {formatValue(value)}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

function EventTypeBadge({ type }: { type: string }) {
  const { bg, text } = getEventTypeColor(type);

  return (
    <span
      className="inline-block px-3 py-1 rounded text-sm font-medium"
      style={{ backgroundColor: bg, color: text }}
    >
      {type}
    </span>
  );
}
