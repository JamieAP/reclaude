import { useState, useEffect } from 'react';

/**
 * Custom hook for debounced state - useful for search inputs.
 * Returns the immediate value for UI binding and a debounced value for queries.
 */
export function useDebouncedState<T>(initialValue: T, delay: number = 300) {
  const [value, setValue] = useState(initialValue);
  const [debouncedValue, setDebouncedValue] = useState(initialValue);

  useEffect(() => {
    const timer = setTimeout(() => setDebouncedValue(value), delay);
    return () => clearTimeout(timer);
  }, [value, delay]);

  return { value, setValue, debouncedValue };
}
