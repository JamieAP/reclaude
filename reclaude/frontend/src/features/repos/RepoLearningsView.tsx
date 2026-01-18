import { useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { Learning } from '../../api';
import { formatDate } from '../../utils/formatUtils';

interface RepoLearningsViewProps {
  learnings: Learning[];
}

const ROW_HEIGHT = 48;

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
            <span
              className="inline-block px-2 py-1 rounded text-xs font-medium"
              style={{ backgroundColor: 'var(--ctp-pink)', color: 'var(--ctp-crust)' }}
            >
              learning
            </span>
            <span className="text-sm font-mono" style={{ color: 'var(--ctp-subtext0)' }}>
              {formatDate(learning.timestamp, 'full')}
            </span>
          </div>
          <button
            onClick={onClose}
            className="text-lg px-2"
            style={{ color: 'var(--ctp-subtext0)' }}
          >
            x
          </button>
        </div>

        <div className="p-6">
          <div
            className="p-4 rounded font-mono text-sm whitespace-pre-wrap"
            style={{ backgroundColor: 'var(--ctp-surface0)', color: 'var(--ctp-text)' }}
          >
            {learning.content}
          </div>

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

export function RepoLearningsView({ learnings }: RepoLearningsViewProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const [selectedLearning, setSelectedLearning] = useState<Learning | null>(null);

  const sortedLearnings = useMemo(() => {
    return [...learnings].sort(
      (a, b) => new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime()
    );
  }, [learnings]);

  const rowVirtualizer = useVirtualizer({
    count: sortedLearnings.length,
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
              const learning = sortedLearnings[virtualRow.index];
              return (
                <div
                  key={`l-${learning.id}`}
                  style={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    width: '100%',
                    height: `${virtualRow.size}px`,
                    transform: `translateY(${virtualRow.start}px)`,
                  }}
                >
                  <div
                    className="flex items-center px-4 py-3 gap-4 border-b cursor-pointer transition-colors"
                    style={{ borderColor: 'var(--ctp-surface1)' }}
                    onClick={() => setSelectedLearning(learning)}
                    onMouseEnter={(e) => (e.currentTarget.style.backgroundColor = 'var(--ctp-surface1)')}
                    onMouseLeave={(e) => (e.currentTarget.style.backgroundColor = 'transparent')}
                  >
                    <div className="w-36 flex-shrink-0 text-sm font-mono" style={{ color: 'var(--ctp-subtext1)' }}>
                      {formatDate(learning.timestamp, 'compactWithSeconds')}
                    </div>
                    <div className="w-28 flex-shrink-0">
                      <span
                        className="inline-block px-2 py-1 rounded text-xs font-medium whitespace-nowrap"
                        style={{ backgroundColor: 'var(--ctp-pink)', color: 'var(--ctp-crust)' }}
                      >
                        learning
                      </span>
                    </div>
                    <div className="flex-1 min-w-0 text-sm truncate" style={{ color: 'var(--ctp-text)' }}>
                      {learning.content.replace(/\n/g, ' ').slice(0, 120)}
                      {learning.content.length > 120 && '...'}
                    </div>
                    <div className="w-4 flex-shrink-0 text-xs" style={{ color: 'var(--ctp-overlay0)' }}>
                      &gt;
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>

        {sortedLearnings.length === 0 && (
          <div className="p-8 text-center" style={{ color: 'var(--ctp-subtext0)' }}>
            No learnings found
          </div>
        )}
      </div>

      {selectedLearning && (
        <LearningDetailModal
          learning={selectedLearning}
          onClose={() => setSelectedLearning(null)}
        />
      )}
    </>
  );
}
