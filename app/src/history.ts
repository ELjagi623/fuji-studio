import { useCallback, useEffect, useRef, useState } from "react";

/**
 * Undo / redo over a snapshot of app state. The snapshot's fields must be immutable
 * values (replaced, never mutated), so keeping old snapshots is just keeping references.
 *
 * Changes that arrive within MERGE_MS of each other become ONE step, so dragging a
 * slider or sweeping over photos to select them undoes in one go.
 */
const MERGE_MS = 500;
const LIMIT = 100;

export function useHistory<S extends Record<string, unknown>>(snap: S, restore: (s: S) => void) {
  const past = useRef<S[]>([]);
  const future = useRef<S[]>([]);
  const last = useRef(snap);
  const lastChange = useRef(0);
  const restoring = useRef(false);
  const [, rerender] = useState(0);

  const changed = (a: S, b: S) => Object.keys(a).some((k) => a[k] !== b[k]);

  useEffect(() => {
    if (!changed(snap, last.current)) return;
    if (restoring.current) {
      // this change is the undo/redo itself
      restoring.current = false;
      last.current = snap;
      return;
    }
    const now = Date.now();
    if (now - lastChange.current > MERGE_MS) {
      past.current.push(last.current);
      if (past.current.length > LIMIT) past.current.shift();
    }
    lastChange.current = now;
    future.current = [];
    last.current = snap;
    rerender((n) => n + 1);
  }, Object.values(snap));

  const step = useCallback(
    (from: React.MutableRefObject<S[]>, to: React.MutableRefObject<S[]>) => {
      const target = from.current.pop();
      if (!target) return false;
      to.current.push(last.current);
      last.current = target;
      lastChange.current = 0; // the next edit starts a new step
      restoring.current = true;
      restore(target);
      rerender((n) => n + 1);
      return true;
    },
    [restore],
  );

  const undo = useCallback(() => step(past, future), [step]);
  const redo = useCallback(() => step(future, past), [step]);

  return { undo, redo, canUndo: past.current.length > 0, canRedo: future.current.length > 0 };
}
