import { useEffect, useCallback, useState } from 'react';

interface UseKeyboardNavigationOptions {
  itemCount: number;
  onSelect?: (index: number) => void;
  onOpen?: (index: number) => void;
  onFocusSearch?: () => void;
  enabled?: boolean;
}

export function useKeyboardNavigation({
  itemCount,
  onSelect,
  onOpen,
  onFocusSearch,
  enabled = true,
}: UseKeyboardNavigationOptions) {
  const [selectedIndex, setSelectedIndex] = useState<number>(-1);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      // Don't handle if typing in an input
      if (
        e.target instanceof HTMLInputElement ||
        e.target instanceof HTMLTextAreaElement ||
        e.target instanceof HTMLSelectElement
      ) {
        // Allow Escape to blur input
        if (e.key === 'Escape') {
          (e.target as HTMLElement).blur();
        }
        return;
      }

      switch (e.key) {
        case 'j':
        case 'ArrowDown':
          e.preventDefault();
          setSelectedIndex((prev) => {
            const next = Math.min(prev + 1, itemCount - 1);
            onSelect?.(next);
            return next;
          });
          break;

        case 'k':
        case 'ArrowUp':
          e.preventDefault();
          setSelectedIndex((prev) => {
            const next = Math.max(prev - 1, 0);
            onSelect?.(next);
            return next;
          });
          break;

        case 'Enter':
          e.preventDefault();
          if (selectedIndex >= 0 && selectedIndex < itemCount) {
            onOpen?.(selectedIndex);
          }
          break;

        case '/':
          e.preventDefault();
          onFocusSearch?.();
          break;

        case 'g':
          // gg to go to top
          if (e.repeat) return;
          const handleG = (e2: KeyboardEvent) => {
            if (e2.key === 'g') {
              setSelectedIndex(0);
              onSelect?.(0);
            }
            window.removeEventListener('keydown', handleG);
          };
          window.addEventListener('keydown', handleG);
          setTimeout(() => window.removeEventListener('keydown', handleG), 500);
          break;

        case 'G':
          // G to go to bottom
          e.preventDefault();
          setSelectedIndex(itemCount - 1);
          onSelect?.(itemCount - 1);
          break;

        case 'Escape':
          e.preventDefault();
          setSelectedIndex(-1);
          onSelect?.(-1);
          break;
      }
    },
    [itemCount, selectedIndex, onSelect, onOpen, onFocusSearch]
  );

  useEffect(() => {
    if (!enabled) return;
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [enabled, handleKeyDown]);

  // Reset selection when item count changes significantly
  useEffect(() => {
    if (selectedIndex >= itemCount) {
      setSelectedIndex(itemCount - 1);
    }
  }, [itemCount, selectedIndex]);

  return {
    selectedIndex,
    setSelectedIndex,
  };
}
