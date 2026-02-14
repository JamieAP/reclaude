import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api } from '../../api';

const DAYS_OPTIONS = [7, 14, 30] as const;

export function PersonasPage() {
  const [days, setDays] = useState<number>(7);

  const { data: usage, isLoading: usageLoading, error: usageError } = useQuery({
    queryKey: ['personas-usage', days],
    queryFn: () => api.personas.usage(days),
  });

  const { data: timeline } = useQuery({
    queryKey: ['personas-timeline', days],
    queryFn: () => api.personas.timeline(days),
  });

  if (usageLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <span style={{ color: 'var(--ctp-subtext0)' }}>Loading persona usage...</span>
      </div>
    );
  }

  if (usageError) {
    return (
      <div
        className="p-4 rounded-lg"
        style={{ backgroundColor: 'var(--ctp-surface0)', color: 'var(--ctp-red)' }}
      >
        Error loading personas: {(usageError as Error).message}
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full gap-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-medium" style={{ color: 'var(--ctp-text)' }}>
          Agent & Persona Usage
        </h1>
        <span className="text-sm" style={{ color: 'var(--ctp-subtext0)' }}>
          {usage?.total_task_calls ?? 0} total Task calls
        </span>
      </div>

      {/* Time range filter */}
      <div
        className="rounded-lg p-4"
        style={{ backgroundColor: 'var(--ctp-surface0)' }}
      >
        <label className="block text-xs mb-2" style={{ color: 'var(--ctp-subtext0)' }}>
          Time Range
        </label>
        <div className="flex gap-1 flex-wrap">
          {DAYS_OPTIONS.map((d) => (
            <button
              key={d}
              onClick={() => setDays(d)}
              className="px-3 py-1 rounded text-xs transition-colors"
              style={{
                backgroundColor: days === d ? 'var(--ctp-mauve)' : 'var(--ctp-surface1)',
                color: days === d ? 'var(--ctp-crust)' : 'var(--ctp-text)',
              }}
            >
              {d} days
            </button>
          ))}
        </div>
      </div>

      {/* Usage sections */}
      <div className="flex-1 overflow-auto space-y-4">
        {/* Custom Personas */}
        {usage?.custom_personas && usage.custom_personas.length > 0 && (
          <div
            className="rounded-lg p-4"
            style={{ backgroundColor: 'var(--ctp-surface0)' }}
          >
            <h2 className="text-sm font-medium mb-3" style={{ color: 'var(--ctp-mauve)' }}>
              Custom Personas
            </h2>
            <div className="space-y-2">
              {usage.custom_personas.map((p) => (
                <div
                  key={p.agent_type}
                  className="flex items-center justify-between p-2 rounded"
                  style={{ backgroundColor: 'var(--ctp-surface1)' }}
                >
                  <div className="flex items-center gap-2">
                    <span
                      className="text-xs px-2 py-1 rounded font-mono"
                      style={{
                        backgroundColor: 'var(--ctp-mauve)',
                        color: 'var(--ctp-crust)',
                      }}
                    >
                      {p.persona || p.agent_type.split(':').pop()}
                    </span>
                    <span className="text-xs" style={{ color: 'var(--ctp-overlay0)' }}>
                      {p.agent_type}
                    </span>
                  </div>
                  <div className="flex items-center gap-4 text-xs">
                    <span style={{ color: 'var(--ctp-text)' }}>
                      {p.count} calls
                    </span>
                    {p.avg_duration_ms && (
                      <span style={{ color: 'var(--ctp-subtext0)' }}>
                        ~{Math.round(p.avg_duration_ms / 1000)}s avg
                      </span>
                    )}
                    <span style={{ color: p.success_count === p.count ? 'var(--ctp-green)' : 'var(--ctp-yellow)' }}>
                      {Math.round((p.success_count / p.count) * 100)}% success
                    </span>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Built-in Agents */}
        {usage?.builtin_agents && usage.builtin_agents.length > 0 && (
          <div
            className="rounded-lg p-4"
            style={{ backgroundColor: 'var(--ctp-surface0)' }}
          >
            <h2 className="text-sm font-medium mb-3" style={{ color: 'var(--ctp-teal)' }}>
              Built-in Agents
            </h2>
            <div className="space-y-2">
              {usage.builtin_agents.map((a) => (
                <div
                  key={a.agent_type}
                  className="flex items-center justify-between p-2 rounded"
                  style={{ backgroundColor: 'var(--ctp-surface1)' }}
                >
                  <span
                    className="text-xs px-2 py-1 rounded font-mono"
                    style={{
                      backgroundColor: 'var(--ctp-teal)',
                      color: 'var(--ctp-crust)',
                    }}
                  >
                    {a.agent_type}
                  </span>
                  <div className="flex items-center gap-4 text-xs">
                    <span style={{ color: 'var(--ctp-text)' }}>
                      {a.count} calls
                    </span>
                    {a.avg_duration_ms && (
                      <span style={{ color: 'var(--ctp-subtext0)' }}>
                        ~{Math.round(a.avg_duration_ms / 1000)}s avg
                      </span>
                    )}
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Timeline */}
        {timeline && timeline.length > 0 && (
          <div
            className="rounded-lg p-4"
            style={{ backgroundColor: 'var(--ctp-surface0)' }}
          >
            <h2 className="text-sm font-medium mb-3" style={{ color: 'var(--ctp-peach)' }}>
              Daily Activity
            </h2>
            <div className="space-y-2">
              {timeline.map((day) => (
                <div key={day.date} className="flex items-start gap-3">
                  <span
                    className="text-xs font-mono w-20 flex-shrink-0"
                    style={{ color: 'var(--ctp-subtext0)' }}
                  >
                    {day.date}
                  </span>
                  <div className="flex flex-wrap gap-1">
                    {Object.entries(day.agents).map(([agent, count]) => (
                      <span
                        key={agent}
                        className="text-xs px-2 py-0.5 rounded"
                        style={{
                          backgroundColor: agent.includes(':') ? 'var(--ctp-mauve)' : 'var(--ctp-surface1)',
                          color: agent.includes(':') ? 'var(--ctp-crust)' : 'var(--ctp-text)',
                        }}
                      >
                        {agent.includes(':') ? agent.split(':').pop() : agent}: {count}
                      </span>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Empty state */}
        {(!usage?.builtin_agents?.length && !usage?.custom_personas?.length) && (
          <div
            className="text-center p-8 rounded-lg"
            style={{
              backgroundColor: 'var(--ctp-surface0)',
              color: 'var(--ctp-subtext0)',
            }}
          >
            No agent/persona usage found in the last {days} days.
            <br />
            <span className="text-xs">Task tool calls with subagent_type will appear here.</span>
          </div>
        )}
      </div>
    </div>
  );
}
