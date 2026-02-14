import { useMemo, useState } from 'react';
import { Link } from 'react-router-dom';
import type { SessionInfo, SemanticEvent } from '../api/types';
import { format, subDays, differenceInMinutes } from 'date-fns';

interface SessionTimelineProps {
  sessions: SessionInfo[];
  events: SemanticEvent[];
  height?: number;
}

type TimeScale = '24h' | '7d' | '30d';

interface SessionActivity {
  session: SessionInfo;
  firstEvent: Date;
  lastEvent: Date;
  eventsByHour: Map<number, number>; // hour bucket -> count
}

function getSessionColor(index: number): string {
  const colors = [
    'var(--ctp-blue)',
    'var(--ctp-green)',
    'var(--ctp-peach)',
    'var(--ctp-mauve)',
    'var(--ctp-teal)',
    'var(--ctp-pink)',
    'var(--ctp-yellow)',
    'var(--ctp-lavender)',
  ];
  return colors[index % colors.length];
}

export function SessionTimeline({ sessions, events, height = 200 }: SessionTimelineProps) {
  const [scale, setScale] = useState<TimeScale>('7d');

  // Compute time range
  const { startTime, endTime, totalHours } = useMemo(() => {
    const now = new Date();
    let start: Date;
    switch (scale) {
      case '24h':
        start = subDays(now, 1);
        break;
      case '7d':
        start = subDays(now, 7);
        break;
      case '30d':
        start = subDays(now, 30);
        break;
    }
    return {
      startTime: start,
      endTime: now,
      totalHours: differenceInMinutes(now, start) / 60,
    };
  }, [scale]);

  // Group events by session and compute activity
  const sessionActivities = useMemo<SessionActivity[]>(() => {
    // Create a map of session_id -> events
    const eventsBySession = new Map<string, SemanticEvent[]>();
    for (const event of events) {
      if (!event.session_id) continue;
      const existing = eventsBySession.get(event.session_id) || [];
      existing.push(event);
      eventsBySession.set(event.session_id, existing);
    }

    // Build activity data for each session
    const activities: SessionActivity[] = [];
    for (const session of sessions) {
      const sessionEvents = eventsBySession.get(session.session_id) || [];
      if (sessionEvents.length === 0) continue;

      // Filter events within time range
      const relevantEvents = sessionEvents.filter((e) => {
        const t = new Date(e.timestamp);
        return t >= startTime && t <= endTime;
      });

      if (relevantEvents.length === 0) continue;

      const timestamps = relevantEvents.map((e) => new Date(e.timestamp));
      const firstEvent = new Date(Math.min(...timestamps.map((t) => t.getTime())));
      const lastEvent = new Date(Math.max(...timestamps.map((t) => t.getTime())));

      // Bucket events by hour
      const eventsByHour = new Map<number, number>();
      for (const event of relevantEvents) {
        const t = new Date(event.timestamp);
        const hourBucket = Math.floor(differenceInMinutes(t, startTime) / 60);
        eventsByHour.set(hourBucket, (eventsByHour.get(hourBucket) || 0) + 1);
      }

      activities.push({
        session,
        firstEvent,
        lastEvent,
        eventsByHour,
      });
    }

    // Sort by first event time (most recent first)
    return activities.sort((a, b) => b.lastEvent.getTime() - a.lastEvent.getTime());
  }, [sessions, events, startTime, endTime]);

  const chartPadding = { top: 30, right: 20, bottom: 30, left: 100 };
  const chartWidth = 900;
  const rowHeight = 24;
  const chartHeight = Math.max(height, chartPadding.top + chartPadding.bottom + sessionActivities.length * rowHeight);
  const plotWidth = chartWidth - chartPadding.left - chartPadding.right;

  // Time axis labels
  const timeLabels = useMemo(() => {
    const labels: { x: number; label: string }[] = [];
    const numLabels = scale === '24h' ? 8 : scale === '7d' ? 7 : 10;

    for (let i = 0; i <= numLabels; i++) {
      const ratio = i / numLabels;
      const time = new Date(startTime.getTime() + ratio * (endTime.getTime() - startTime.getTime()));
      labels.push({
        x: chartPadding.left + ratio * plotWidth,
        label: scale === '24h' ? format(time, 'HH:mm') : format(time, 'MMM d'),
      });
    }
    return labels;
  }, [startTime, endTime, scale, plotWidth]);

  if (sessionActivities.length === 0) {
    return (
      <div
        className="rounded-lg p-6 text-center"
        style={{ backgroundColor: 'var(--ctp-surface0)', color: 'var(--ctp-subtext0)' }}
      >
        No session activity in the selected time range
      </div>
    );
  }

  return (
    <div
      className="rounded-lg p-4"
      style={{ backgroundColor: 'var(--ctp-surface0)' }}
    >
      {/* Header */}
      <div className="flex items-center justify-between mb-4">
        <h3 className="text-sm font-medium" style={{ color: 'var(--ctp-subtext0)' }}>
          Session Activity Timeline
        </h3>
        <div className="flex gap-1">
          {(['24h', '7d', '30d'] as const).map((s) => (
            <button
              key={s}
              onClick={() => setScale(s)}
              className="px-2 py-1 text-xs rounded transition-colors"
              style={{
                backgroundColor: scale === s ? 'var(--ctp-green)' : 'var(--ctp-surface1)',
                color: scale === s ? 'var(--ctp-crust)' : 'var(--ctp-text)',
              }}
            >
              {s}
            </button>
          ))}
        </div>
      </div>

      {/* Timeline SVG */}
      <svg
        viewBox={`0 0 ${chartWidth} ${chartHeight}`}
        className="w-full"
        style={{ height: Math.min(chartHeight, 400) }}
      >
        {/* Time axis at top */}
        <line
          x1={chartPadding.left}
          y1={chartPadding.top - 10}
          x2={chartWidth - chartPadding.right}
          y2={chartPadding.top - 10}
          stroke="var(--ctp-surface2)"
        />
        {timeLabels.map((label, i) => (
          <text
            key={i}
            x={label.x}
            y={chartPadding.top - 15}
            textAnchor="middle"
            fill="var(--ctp-subtext0)"
            fontSize="10"
          >
            {label.label}
          </text>
        ))}

        {/* Grid lines */}
        {timeLabels.map((label, i) => (
          <line
            key={i}
            x1={label.x}
            y1={chartPadding.top}
            x2={label.x}
            y2={chartHeight - chartPadding.bottom}
            stroke="var(--ctp-surface1)"
            strokeDasharray="2,4"
          />
        ))}

        {/* Session rows */}
        {sessionActivities.map((activity, rowIndex) => {
          const y = chartPadding.top + rowIndex * rowHeight;
          const color = getSessionColor(rowIndex);

          // Calculate bar position based on first/last event
          const startRatio = differenceInMinutes(activity.firstEvent, startTime) / (totalHours * 60);
          const endRatio = differenceInMinutes(activity.lastEvent, startTime) / (totalHours * 60);
          const barX = chartPadding.left + Math.max(0, startRatio) * plotWidth;
          const barWidth = Math.max(4, (endRatio - Math.max(0, startRatio)) * plotWidth);

          return (
            <g key={activity.session.session_id}>
              {/* Row background on hover */}
              <rect
                x={0}
                y={y}
                width={chartWidth}
                height={rowHeight}
                fill="transparent"
                className="hover:fill-[var(--ctp-surface1)]"
                style={{ cursor: 'pointer' }}
              />

              {/* Session label */}
              <Link to={`/events?session_id=${activity.session.session_id}`}>
                <text
                  x={chartPadding.left - 8}
                  y={y + rowHeight / 2 + 4}
                  textAnchor="end"
                  fill="var(--ctp-lavender)"
                  fontSize="11"
                  className="cursor-pointer hover:underline"
                >
                  {activity.session.session_id.slice(0, 8)}
                </text>
              </Link>

              {/* Activity bar */}
              <Link to={`/events?session_id=${activity.session.session_id}`}>
                <rect
                  x={barX}
                  y={y + 4}
                  width={barWidth}
                  height={rowHeight - 8}
                  fill={color}
                  rx={3}
                  opacity={0.8}
                  className="cursor-pointer hover:opacity-100"
                />
              </Link>

              {/* Event count label */}
              <text
                x={barX + barWidth + 6}
                y={y + rowHeight / 2 + 4}
                fill="var(--ctp-subtext0)"
                fontSize="10"
              >
                {activity.session.event_count}
              </text>
            </g>
          );
        })}
      </svg>

      {/* Legend */}
      <div className="flex items-center justify-between mt-3 text-xs" style={{ color: 'var(--ctp-subtext0)' }}>
        <span>{sessionActivities.length} active sessions</span>
        <span>Click a session to view its events</span>
      </div>
    </div>
  );
}
