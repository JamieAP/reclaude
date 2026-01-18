import { useRef, useState, useEffect, useMemo } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { SemanticEvent } from '../api';
import { formatSmartTimestamp } from '../utils/formatUtils';
import { getEventTypeColor } from '../utils/eventTypeColors';
import { EventDetailModal } from './EventDetailModal';

interface VirtualizedEventTableProps {
  events: SemanticEvent[];
  hasMore: boolean;
  onLoadMore?: () => void;
  selectedIndex?: number;
  onOpenEvent?: (index: number) => void;
}

const ROW_HEIGHT = 48;

interface ToolMeta {
  tool: string;
  success: boolean;
  duration: string;
}

// Display item can be a single event or a collapsed group
type DisplayItem =
  | { kind: 'event'; event: SemanticEvent; originalIndex: number }
  | { kind: 'group'; events: SemanticEvent[]; startIndex: number; tools: string[] };

function parseToolMeta(event: SemanticEvent): ToolMeta | null {
  if (event.event_type !== 'tool_use') return null;

  const lines = event.content.split('\n');
  const toolLine = lines.find((l) => l.startsWith('Tool:'));
  const successLine = lines.find((l) => l.startsWith('Success:'));
  const durationLine = lines.find((l) => l.startsWith('Duration:'));

  if (!toolLine) return null;

  return {
    tool: toolLine.replace('Tool:', '').trim(),
    success: successLine?.includes('True') ?? false,
    duration: durationLine?.replace('Duration:', '').trim() ?? '',
  };
}

function getEventPreview(event: SemanticEvent): string {
  if (event.event_type === 'tool_use') {
    const meta = parseToolMeta(event);
    if (meta) {
      // For Bash, extract command from input
      if (meta.tool === 'Bash') {
        const inputStart = event.content.indexOf('--- INPUT ---');
        const outputStart = event.content.indexOf('--- OUTPUT ---');
        if (inputStart !== -1 && outputStart !== -1) {
          const inputStr = event.content.slice(inputStart + 13, outputStart).trim();
          try {
            const input = JSON.parse(inputStr);
            const cmd = input.command || '';
            const shortCmd = cmd.length > 60 ? cmd.slice(0, 60) + '...' : cmd;
            return `${meta.tool}: ${shortCmd}`;
          } catch {
            // Fall through
          }
        }
      }
      return meta.tool;
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

// Group consecutive tool_use events
function groupEvents(events: SemanticEvent[]): DisplayItem[] {
  const items: DisplayItem[] = [];
  let i = 0;

  while (i < events.length) {
    const event = events[i];

    // Check for consecutive tool_use events (3+ to group)
    if (event.event_type === 'tool_use') {
      const groupStart = i;
      const group: SemanticEvent[] = [event];
      const tools: string[] = [];
      const meta = parseToolMeta(event);
      if (meta) tools.push(meta.tool);

      i++;
      while (i < events.length && events[i].event_type === 'tool_use') {
        group.push(events[i]);
        const m = parseToolMeta(events[i]);
        if (m) tools.push(m.tool);
        i++;
      }

      // Only group if 3+ consecutive tool_use
      if (group.length >= 3) {
        items.push({ kind: 'group', events: group, startIndex: groupStart, tools });
      } else {
        // Add individually
        for (let j = 0; j < group.length; j++) {
          items.push({ kind: 'event', event: group[j], originalIndex: groupStart + j });
        }
      }
    } else {
      items.push({ kind: 'event', event, originalIndex: i });
      i++;
    }
  }

  return items;
}

export function VirtualizedEventTable({
  events,
  hasMore,
  onLoadMore,
  selectedIndex = -1,
}: VirtualizedEventTableProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const [selectedEvent, setSelectedEvent] = useState<SemanticEvent | null>(null);
  const [expandedGroups, setExpandedGroups] = useState<Set<number>>(new Set());

  // Group events
  const groupedItems = useMemo(() => groupEvents(events), [events]);

  // Flatten for virtualization based on expanded state
  const displayRows = useMemo(() => {
    const rows: { item: DisplayItem; eventIndex: number; isGroupChild?: boolean }[] = [];

    for (const item of groupedItems) {
      if (item.kind === 'event') {
        rows.push({ item, eventIndex: item.originalIndex });
      } else {
        // Group
        const isExpanded = expandedGroups.has(item.startIndex);
        if (isExpanded) {
          // Show all events in group
          item.events.forEach((event, idx) => {
            rows.push({
              item: { kind: 'event', event, originalIndex: item.startIndex + idx },
              eventIndex: item.startIndex + idx,
              isGroupChild: idx > 0,
            });
          });
        } else {
          // Show collapsed group header
          rows.push({ item, eventIndex: item.startIndex });
        }
      }
    }

    return rows;
  }, [groupedItems, expandedGroups]);

  const rowVirtualizer = useVirtualizer({
    count: displayRows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 10,
  });

  // Scroll to selected index when it changes
  useEffect(() => {
    if (selectedIndex >= 0) {
      // Find the display row for this event index
      const rowIdx = displayRows.findIndex((r) => r.eventIndex === selectedIndex);
      if (rowIdx >= 0) {
        rowVirtualizer.scrollToIndex(rowIdx, { align: 'auto' });
      }
    }
  }, [selectedIndex, displayRows, rowVirtualizer]);

  const toggleGroup = (startIndex: number) => {
    setExpandedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(startIndex)) {
        next.delete(startIndex);
      } else {
        next.add(startIndex);
      }
      return next;
    });
  };

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
              const { item, eventIndex, isGroupChild } = displayRows[virtualRow.index];
              const isSelected = eventIndex === selectedIndex;

              return (
                <div
                  key={`${item.kind}-${item.kind === 'group' ? item.startIndex : item.event.id}`}
                  style={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    width: '100%',
                    height: `${virtualRow.size}px`,
                    transform: `translateY(${virtualRow.start}px)`,
                  }}
                >
                  {item.kind === 'event' ? (
                    <EventRow
                      event={item.event}
                      isSelected={isSelected}
                      isGroupChild={isGroupChild}
                      onClick={() => setSelectedEvent(item.event)}
                    />
                  ) : (
                    <GroupRow
                      group={item}
                      isExpanded={expandedGroups.has(item.startIndex)}
                      onToggle={() => toggleGroup(item.startIndex)}
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
              Load more events...
            </button>
          )}
        </div>

        {events.length === 0 && (
          <div className="p-8 text-center" style={{ color: 'var(--ctp-subtext0)' }}>
            No events found
          </div>
        )}
      </div>

      {/* Detail modal */}
      {selectedEvent && (
        <EventDetailModal
          event={selectedEvent}
          onClose={() => setSelectedEvent(null)}
        />
      )}
    </>
  );
}

function GroupRow({
  group,
  isExpanded,
  onToggle,
}: {
  group: Extract<DisplayItem, { kind: 'group' }>;
  isExpanded: boolean;
  onToggle: () => void;
}) {
  // Count tools
  const toolCounts = group.tools.reduce(
    (acc, tool) => {
      acc[tool] = (acc[tool] || 0) + 1;
      return acc;
    },
    {} as Record<string, number>
  );

  // Count successes
  const successCount = group.events.filter((e) => {
    const meta = parseToolMeta(e);
    return meta?.success;
  }).length;

  const firstEvent = group.events[0];

  // Summary of tools
  const toolSummary = Object.entries(toolCounts)
    .map(([tool, count]) => (count > 1 ? `${tool} x${count}` : tool))
    .join(', ');

  return (
    <div
      className="flex items-center px-4 gap-4 border-b cursor-pointer transition-colors h-full overflow-hidden"
      style={{
        borderColor: 'var(--ctp-surface1)',
        backgroundColor: 'var(--ctp-surface1)',
        borderLeft: '3px solid var(--ctp-teal)',
      }}
      onClick={onToggle}
      onMouseEnter={(e) => (e.currentTarget.style.backgroundColor = 'var(--ctp-surface2)')}
      onMouseLeave={(e) => (e.currentTarget.style.backgroundColor = 'var(--ctp-surface1)')}
    >
      <div className="w-36 flex-shrink-0 text-sm font-mono" style={{ color: 'var(--ctp-subtext1)' }}>
        {formatSmartTimestamp(firstEvent.timestamp)}
      </div>
      <div className="w-28 flex-shrink-0">
        <span
          className="inline-block px-2 py-1 rounded text-xs font-medium whitespace-nowrap"
          style={{ backgroundColor: 'var(--ctp-teal)', color: 'var(--ctp-crust)' }}
        >
          {group.events.length} tools
        </span>
      </div>
      <div className="flex-1 min-w-0 text-sm truncate" style={{ color: 'var(--ctp-text)' }}>
        {toolSummary}
      </div>
      <div className="w-24 flex-shrink-0 flex items-center justify-end gap-2">
        <span className="text-sm" style={{ color: 'var(--ctp-green)' }}>
          {successCount}✓
        </span>
        {successCount < group.events.length && (
          <span className="text-sm" style={{ color: 'var(--ctp-red)' }}>
            {group.events.length - successCount}✗
          </span>
        )}
      </div>
      <div className="w-4 flex-shrink-0 text-xs" style={{ color: 'var(--ctp-overlay0)' }}>
        {isExpanded ? '▼' : '▶'}
      </div>
    </div>
  );
}

function EventRow({
  event,
  isSelected,
  isGroupChild,
  onClick,
}: {
  event: SemanticEvent;
  isSelected?: boolean;
  isGroupChild?: boolean;
  onClick: () => void;
}) {
  const toolMeta = parseToolMeta(event);

  return (
    <div
      className="flex items-center px-4 gap-4 border-b cursor-pointer transition-colors h-full overflow-hidden"
      style={{
        borderColor: 'var(--ctp-surface1)',
        backgroundColor: isSelected ? 'var(--ctp-surface1)' : 'transparent',
        borderLeft: isSelected ? '3px solid var(--ctp-blue)' : '3px solid transparent',
        paddingLeft: isGroupChild ? '2rem' : undefined,
      }}
      onClick={onClick}
      onMouseEnter={(e) => {
        if (!isSelected) e.currentTarget.style.backgroundColor = 'var(--ctp-surface1)';
      }}
      onMouseLeave={(e) => {
        if (!isSelected) e.currentTarget.style.backgroundColor = 'transparent';
      }}
    >
      <div
        className="w-36 flex-shrink-0 text-sm font-mono"
        style={{ color: 'var(--ctp-subtext1)' }}
      >
        {formatSmartTimestamp(event.timestamp)}
      </div>
      <div className="w-28 flex-shrink-0">
        <EventTypeBadge type={event.event_type} />
      </div>
      <div
        className="flex-1 min-w-0 text-sm truncate"
        style={{ color: 'var(--ctp-text)' }}
      >
        {getEventPreview(event)}
      </div>
      {/* Status + Duration for tool_use, right-aligned */}
      <div className="w-24 flex-shrink-0 flex items-center justify-end gap-2">
        {toolMeta && (
          <>
            <span
              className="text-sm"
              style={{ color: toolMeta.success ? 'var(--ctp-green)' : 'var(--ctp-red)' }}
            >
              {toolMeta.success ? '✓' : '✗'}
            </span>
            <span
              className="text-xs font-mono"
              style={{ color: 'var(--ctp-subtext0)' }}
            >
              {toolMeta.duration}
            </span>
          </>
        )}
      </div>
      <div className="w-4 flex-shrink-0 text-xs" style={{ color: 'var(--ctp-overlay0)' }}>
        ▶
      </div>
    </div>
  );
}

function EventTypeBadge({ type }: { type: string }) {
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
