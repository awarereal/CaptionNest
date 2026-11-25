// Where dictated words go in a caption.

/** The caption text and selection when dictation started. */
export interface InsertTarget {
  start: number;
  end: number;
  text: string;
}

/**
 * Inserts `words` at the cursor recorded in `target`, replacing the selection only if the
 * caption hasn't changed since. Everything else in the caption is kept. A space is added
 * where the words would otherwise run into their neighbours.
 */
export function insertWords(current: string, target: InsertTarget, words: string): { text: string; caret: number } {
  let start: number;
  let end: number;
  if (current === target.text) {
    start = target.start;
    end = target.end;
  } else if (target.start <= current.length && current.slice(0, target.start) === target.text.slice(0, target.start)) {
    // Edited after the cursor meanwhile: insert at the cursor, replace nothing.
    start = end = target.start;
  } else {
    start = end = current.length;
  }
  const before = current.slice(0, start);
  const after = current.slice(end);
  const lead = before && !/\s$/.test(before) && !/^[\s,.!?।]/.test(words) ? " " : "";
  const trail = after && !/^[\s,.!?।]/.test(after) ? " " : "";
  return { text: before + lead + words + trail + after, caret: before.length + lead.length + words.length };
}
