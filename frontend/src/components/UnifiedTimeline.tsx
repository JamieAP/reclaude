import { useRef, useState, useMemo } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { SemanticEvent } from '../api';
import { formatDate } from '../utils/formatUtils';
import { getEventTypeColor } from '../utils/eventTypeColors';
import { EventDetailModal } from './EventDetailModal';

interface UnifiedTimelineProps {
  events: SemanticEvent[];
  hasMore?: boolean;
  onLoadMore?: () => void;
}

const ROW_HEIGHT = 48;

function getEventPreview(event: SemanticEvent): string {
  if (event.event_type === 'tool_use') {
    const lines = event.content.split('\n');
    const toolLine = lines.find((l) => l.startsWith('Tool:'));
    const successLine = lines.find((l) => l.startsWith('Success:'));
    const durationLine = lines.find((l) => l.startsWith('Duration:'));

    if (toolLine) {
      const tool = toolLine.replace('Tool:', '').trim();
      const success = successLine?.includes('True') ? '✓' : '✗';
      const duration = durationLine?.replace('Duration:', '').trim() ?? '';

      // For Bash, extract command from input
      if (tool === 'Bash') {
        const inputStart = event.content.indexOf('--- INPUT ---');
        const outputStart = event.content.indexOf('--- OUTPUT ---');
        if (inputStart !== -1 && outputStart !== -1) {
          const inputStr = event.content.slice(inputStart + 13, outputStart).trim();
          try {
            const input = JSON.parse(inputStr);
            const cmd = input.command || '';
            const shortCmd = cmd.length > 50 ? cmd.slice(0, 50) + '...' : cmd;
            return `${tool}: ${shortCmd} ${success} ${duration}`;
          } catch {
            // Fall through to default
          }
        }
      }

      return `${tool} ${success} ${duration}`;
    }
  }

  // File diff: show relative path and +/- stats
  if (event.event_type === 'file_diff') {
    const filePath = event.metadata?.file_path as string | undefined;
    const repoRoot = event.metadata?.repo_root as string | undefined;
    const linesAdded = event.metadata?.lines_added as number | undefined;
    const linesRemoved = event.metadata?.lines_removed as number | undefined;
    const operation = event.metadata?.operation as string | undefined;

    if (filePath) {
      // Compute relative path from repo root
      let displayPath = filePath;
      if (repoRoot && filePath.startsWith(repoRoot + '/')) {
        displayPath = filePath.slice(repoRoot.length + 1);
      } else if (repoRoot && filePath.startsWith(repoRoot)) {
        displayPath = filePath.slice(repoRoot.length);
      }
      const stats = [];
      if (linesAdded !== undefined && linesAdded > 0) stats.push(`+${linesAdded}`);
      if (linesRemoved !== undefined && linesRemoved > 0) stats.push(`-${linesRemoved}`);
      const statsStr = stats.length > 0 ? ` (${stats.join('/')})` : '';
      const opStr = operation === 'write' ? ' [new]' : '';
      return `${displayPath}${statsStr}${opStr}`;
    }
  }

  return event.content.replace(/\n/g, ' ').slice(0, 120) + (event.content.length > 120 ? '...' : '');
}

export function UnifiedTimeline({
  events,
  hasMore,
  onLoadMore,
}: UnifiedTimelineProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const [selectedEvent, setSelectedEvent] = useState<SemanticEvent | null>(null);

  // Sort by timestamp
  const timeline = useMemo(() => {
    return [...events].sort(
      (a, b) => new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime()
    );
  }, [events]);

  const rowVirtualizer = useVirtualizer({
    count: timeline.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 10,
  });

  return (
    <>
      <div
        className="rounded-lg overflow-hidden flex flex-col"
        style={{ backgroundColor: 'var(--ctp-surface0)', height: 'calc(100vh - 300px)' }}
      >
        {/* Header */}
        <div
          className="flex items-center px-4 py-3 border-b gap-4"
          style={{ backgroundColor: 'var(--ctp-surface1)', borderColor: 'var(--ctp-surface0)' }}
        >
          <div className="w-36 flex-shrink-0 text-sm font-medium" style={{ color: 'var(--ctp-subtext0)' }}>
            Timestamp
          </div>
          <div className="w-28 flex-shrink-0 text-sm font-medium" style={{ color: 'var(--ctp-subtext0)' }}>
            Type
          </div>
          <div className="flex-1 min-w-0 text-sm font-medium" style={{ color: 'var(--ctp-subtext0)' }}>
            Content
          </div>
          <div className="w-4 flex-shrink-0" />
        </div>

        {/* Virtualized body */}
        <div ref={parentRef} className="flex-1 overflow-auto">
          <div
            style={{
              height: `${rowVirtualizer.getTotalSize()}px`,
              width: '100%',
              position: 'relative',
            }}
          >
            {rowVirtualizer.getVirtualItems().map((virtualRow) => {
              const event = timeline[virtualRow.index];

              return (
                <div
                  key={`e-${event.id}`}
                  style={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    width: '100%',
                    height: `${virtualRow.size}px`,
                    transform: `translateY(${virtualRow.start}px)`,
                  }}
                >
                  <EventRow
                    event={event}
                    onClick={() => setSelectedEvent(event)}
                  />
                </div>
              );
            })}
          </div>

          {/* Load more indicator */}
          {hasMore && onLoadMore && (
            <button
              onClick={onLoadMore}
              className="w-full py-3 text-sm text-center hover:bg-opacity-50"
              style={{ color: 'var(--ctp-blue)', backgroundColor: 'var(--ctp-surface1)' }}
            >
              Load more...
            </button>
          )}
        </div>

        {timeline.length === 0 && (
          <div className="p-8 text-center" style={{ color: 'var(--ctp-subtext0)' }}>
            No items found
          </div>
        )}
      </div>

      {/* Event detail modal */}
      {selectedEvent && (
        <EventDetailModal
          event={selectedEvent}
          onClose={() => setSelectedEvent(null)}
        />
      )}
    </>
  );
}

function EventRow({
  event,
  onClick,
}: {
  event: SemanticEvent;
  onClick: () => void;
}) {
  return (
    <div
      className="flex items-center px-4 py-3 gap-4 border-b cursor-pointer transition-colors"
      style={{ borderColor: 'var(--ctp-surface1)' }}
      onClick={onClick}
      onMouseEnter={(e) => (e.currentTarget.style.backgroundColor = 'var(--ctp-surface1)')}
      onMouseLeave={(e) => (e.currentTarget.style.backgroundColor = 'transparent')}
    >
      <div className="w-36 flex-shrink-0 text-sm font-mono" style={{ color: 'var(--ctp-subtext1)' }}>
        {formatDate(event.timestamp, 'compactWithSeconds')}
      </div>
      <div className="w-28 flex-shrink-0">
        <TypeBadge type={event.event_type} />
      </div>
      <div className="flex-1 min-w-0 text-sm truncate" style={{ color: 'var(--ctp-text)' }}>
        {getEventPreview(event)}
      </div>
      <div className="w-4 flex-shrink-0 text-xs" style={{ color: 'var(--ctp-overlay0)' }}>
        ▶
      </div>
    </div>
  );
}

function TypeBadge({ type }: { type: string }) {
  const { bg, text } = getEventTypeColor(type);

  return (
    <span
      className="inline-block px-2 py-1 rounded text-xs font-medium whitespace-nowrap"
      style={{ backgroundColor: bg, color: text }}
    >
      {type}
    </span>
  );
}
