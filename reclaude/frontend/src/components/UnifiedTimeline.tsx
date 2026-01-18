import { useRef, useState, useMemo } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { TimelineEntry, SemanticEvent, Learning } from '../api';
import { formatDate } from '../utils/formatUtils';
import { getEventTypeColor } from '../utils/eventTypeColors';
import { EventDetailModal } from './EventDetailModal';

interface UnifiedTimelineProps {
  events: SemanticEvent[];
  learnings: Learning[];
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
  learnings,
  hasMore,
  onLoadMore,
}: UnifiedTimelineProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const [selectedEvent, setSelectedEvent] = useState<SemanticEvent | null>(null);
  const [selectedLearning, setSelectedLearning] = useState<Learning | null>(null);

  // Merge and sort by timestamp
  const timeline = useMemo(() => {
    const entries: TimelineEntry[] = [
      ...events.map((e) => ({ kind: 'event' as const, data: e })),
      ...learnings.map((l) => ({ kind: 'learning' as const, data: l })),
    ];
    return entries.sort(
      (a, b) => new Date(b.data.timestamp).getTime() - new Date(a.data.timestamp).getTime()
    );
  }, [events, learnings]);

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
              const entry = timeline[virtualRow.index];
              const entryId = entry.kind === 'event' ? `e-${entry.data.id}` : `l-${entry.data.id}`;

              return (
                <div
                  key={entryId}
                  style={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    width: '100%',
                    height: `${virtualRow.size}px`,
                    transform: `translateY(${virtualRow.start}px)`,
                  }}
                >
                  {entry.kind === 'event' ? (
                    <EventRow
                      event={entry.data}
                      onClick={() => setSelectedEvent(entry.data)}
                    />
                  ) : (
                    <LearningRow
                      learning={entry.data}
                      onClick={() => setSelectedLearning(entry.data)}
                    />
                  )}
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

      {/* Learning detail modal */}
      {selectedLearning && (
        <LearningDetailModal
          learning={selectedLearning}
          onClose={() => setSelectedLearning(null)}
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

function LearningRow({
  learning,
  onClick,
}: {
  learning: Learning;
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
        {formatDate(learning.timestamp, 'compactWithSeconds')}
      </div>
      <div className="w-28 flex-shrink-0">
        <TypeBadge type="learning" />
      </div>
      <div className="flex-1 min-w-0 text-sm truncate" style={{ color: 'var(--ctp-text)' }}>
        {learning.content.replace(/\n/g, ' ').slice(0, 120)}
        {learning.content.length > 120 && '...'}
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

function LearningDetailModal({
  learning,
  onClose,
}: {
  learning: Learning;
  onClose: () => void;
}) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4"
      style={{ backgroundColor: 'rgba(0, 0, 0, 0.7)' }}
      onClick={onClose}
    >
      <div
        className="rounded-lg max-w-3xl w-full max-h-[80vh] overflow-auto shadow-2xl"
        style={{ backgroundColor: 'var(--ctp-base)', border: '1px solid var(--ctp-surface1)' }}
        onClick={(e) => e.stopPropagation()}
      >
        <div
          className="flex items-center justify-between px-6 py-4 border-b"
          style={{ borderColor: 'var(--ctp-surface0)' }}
        >
          <div className="flex items-center gap-3">
            <TypeBadge type="learning" />
            <span className="text-sm font-mono" style={{ color: 'var(--ctp-subtext0)' }}>
              {formatDate(learning.timestamp, 'full')}
            </span>
          </div>
          <button
            onClick={onClose}
            className="text-lg px-2"
            style={{ color: 'var(--ctp-subtext0)' }}
          >
            ×
          </button>
        </div>

        <div className="p-6">
          <div
            className="p-4 rounded font-mono text-sm whitespace-pre-wrap"
            style={{ backgroundColor: 'var(--ctp-surface0)', color: 'var(--ctp-text)' }}
          >
            {learning.content}
          </div>

          {/* Metadata */}
          <div className="mt-4 grid grid-cols-2 gap-4 text-sm">
            {learning.repo_name && (
              <div>
                <span style={{ color: 'var(--ctp-subtext0)' }}>Repo:</span>{' '}
                <span style={{ color: 'var(--ctp-text)' }}>{learning.repo_name}</span>
              </div>
            )}
            {learning.branch && (
              <div>
                <span style={{ color: 'var(--ctp-subtext0)' }}>Branch:</span>{' '}
                <span style={{ color: 'var(--ctp-text)' }}>{learning.branch}</span>
              </div>
            )}
            {learning.cwd && (
              <div className="col-span-2">
                <span style={{ color: 'var(--ctp-subtext0)' }}>Directory:</span>{' '}
                <span className="font-mono" style={{ color: 'var(--ctp-text)' }}>{learning.cwd}</span>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
