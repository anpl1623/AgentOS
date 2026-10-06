/**
 * What the argument block shows, worked out without React.
 *
 * The arguments on an approval card are the one thing on screen that will
 * actually run, and they are written by a model that may have read a hostile
 * page. Two tricks turn exact text into misleading text. Direction controls
 * (Trojan Source) reorder what a person reads without changing what a program
 * receives, so `rm -rf ⟨RLO⟩fdp.` is drawn as something harmless. Invisible
 * characters hide a tail: a zero-width run, a tag sequence or a stack of
 * variation selectors carries data no one can see. Neither may reach the
 * operator as the character itself, so every such code point is drawn as a
 * visible escape, counted, and written as `\uXXXX` when copied.
 *
 * Folding a long value is a convenience for a decision already made. While a
 * request is pending nothing is folded; see {@link FOLD_AT}.
 */

// Every control (Cc) and format (Cf) character, which covers the bidi
// embeddings, overrides and isolates, the marks, the zero-width space, joiners,
// word joiner, byte-order mark and the tag block. Then every default-ignorable
// code point, which is Unicode's own list of what a renderer may draw as
// nothing: the combining grapheme joiner, the Hangul and Khmer fillers, the
// Mongolian and the ordinary variation selectors, and the unassigned ranges
// held for more of the same (U+2065, U+FFF0–FFF8, U+E0000–E0FFF), which a
// webview draws at zero width today. A run of variation selectors after one
// visible glyph, or of unassigned tags, is the known way to smuggle bytes, and
// an approval card is not the place to tell that from an ordinary emoji.
// Last, the line and paragraph separators: JSON lets them stand raw inside a
// string, and they break a line without being a line feed.
const HIDDEN = /[\p{Cc}\p{Cf}\p{Default_Ignorable_Code_Point}\u2028\u2029]/u;

/**
 * Whether one code point is invisible or changes direction.
 *
 * Line feed and tab are not: they are how text is laid out, they show as what
 * they are, and the views wrap rather than hide what follows them.
 */
export function isHidden(char: string): boolean {
  return char !== "\n" && char !== "\t" && HIDDEN.test(char);
}

/** A run of ordinary text, or one hidden code point. */
export type Segment = { kind: "text"; text: string } | { kind: "hidden"; codePoint: number };

/** Split text into what can be drawn as itself and what must be drawn as an escape. */
export function segments(text: string): Segment[] {
  const out: Segment[] = [];
  let run = "";
  for (const char of text) {
    if (!isHidden(char)) {
      run += char;
      continue;
    }
    if (run !== "") out.push({ kind: "text", text: run });
    run = "";
    out.push({ kind: "hidden", codePoint: char.codePointAt(0) ?? 0 });
  }
  if (run !== "") out.push({ kind: "text", text: run });
  return out;
}

/** How many hidden code points the text holds. */
export function countHidden(text: string): number {
  let count = 0;
  for (const char of text) if (isHidden(char)) count += 1;
  return count;
}

/** The visible stand-in for a hidden code point, e.g. `⟨U+202E⟩`. */
export function escapeLabel(codePoint: number): string {
  return `⟨U+${codePoint.toString(16).toUpperCase().padStart(4, "0")}⟩`;
}

/** The text as the views draw it, with every hidden code point as its label. */
export function visibleText(text: string): string {
  return segments(text)
    .map((segment) => (segment.kind === "text" ? segment.text : escapeLabel(segment.codePoint)))
    .join("");
}

/** The warning above the block, or `null` when there is nothing to warn of. */
export function hiddenWarning(count: number): string | null {
  if (count === 0) return null;
  const subject =
    count === 1
      ? "1 invisible or direction-changing character is"
      : `${count} invisible or direction-changing characters are`;
  return `${subject} shown as ⟨U+…⟩. The text around ${count === 1 ? "it" : "them"} may not run the way it reads.`;
}

function jsonEscape(char: string): string {
  let out = "";
  for (let index = 0; index < char.length; index += 1) {
    out += `\\u${char.charCodeAt(index).toString(16).toUpperCase().padStart(4, "0")}`;
  }
  return out;
}

/**
 * The arguments as JSON with nothing invisible in them, for the clipboard.
 *
 * Inside a string literal each hidden code point becomes its `\uXXXX` escape
 * (a surrogate pair above the BMP), which decodes to the same character, so
 * the parsed value is unchanged. The text is edited in place rather than
 * parsed and re-serialised: a round trip through `JSON.parse` would round an
 * integer past 2^53, and the copy would then name a different number from the
 * one that runs. Outside strings valid JSON can only hold its own whitespace;
 * anything else there is escaped too, so even malformed text pastes clean.
 */
export function copyableJson(text: string): string {
  let out = "";
  let inString = false;
  let escaped = false;
  for (const char of text) {
    if (inString) {
      if (isHidden(char)) {
        // Valid JSON never has one straight after a backslash; malformed text
        // may, and the clipboard stays clean whatever that does to it.
        out += jsonEscape(char);
        escaped = false;
        continue;
      }
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === '"') inString = false;
      out += char;
      continue;
    }
    if (char === '"') inString = true;
    out += char !== "\r" && isHidden(char) ? jsonEscape(char) : char;
  }
  return out;
}

// ---------------------------------------------------------------------------
// Fields
// ---------------------------------------------------------------------------

/** One top-level argument. */
export interface ArgumentField {
  /** The key, decoded. */
  key: string;
  /** A string value, decoded. */
  string: boolean;
  /**
   * What the Fields view draws: a string's contents, or any other value
   * exactly as the JSON text writes it, with its indentation brought in.
   */
  text: string;
}

/** How the arguments can be shown. */
export type ArgumentView =
  /** A JSON object: one line per key, with Raw one click away. */
  | { kind: "fields"; fields: ArgumentField[] }
  /** Anything else — malformed, or a value with no keys to list. Raw only. */
  | { kind: "raw"; why: string };

function isJsonSpace(char: string | undefined): boolean {
  return char === " " || char === "\t" || char === "\n" || char === "\r";
}

function skipSpace(text: string, at: number): number {
  let index = at;
  while (isJsonSpace(text[index])) index += 1;
  return index;
}

function stringEnd(text: string, start: number): number {
  let index = start + 1;
  while (index < text.length) {
    const char = text[index];
    if (char === "\\") index += 2;
    else if (char === '"') return index + 1;
    else index += 1;
  }
  return text.length;
}

function valueEnd(text: string, start: number): number {
  const first = text[start];
  if (first === '"') return stringEnd(text, start);
  if (first === "{" || first === "[") {
    let depth = 0;
    let index = start;
    while (index < text.length) {
      const char = text[index];
      if (char === '"') {
        index = stringEnd(text, index);
        continue;
      }
      if (char === "{" || char === "[") depth += 1;
      else if (char === "}" || char === "]") {
        depth -= 1;
        if (depth === 0) return index + 1;
      }
      index += 1;
    }
    return text.length;
  }
  let index = start;
  while (index < text.length) {
    const char = text[index];
    if (isJsonSpace(char) || char === "," || char === "}" || char === "]") break;
    index += 1;
  }
  return index;
}

/**
 * Bring a multi-line value's continuation lines in by its closing line's
 * indentation, so a nested object reads from the left margin of its field.
 * Only whitespace that every such line shares is removed.
 */
function dedent(source: string): string {
  const lines = source.split("\n");
  if (lines.length < 2) return source;
  const indent = /^ */.exec(lines[lines.length - 1] ?? "")?.[0] ?? "";
  if (indent === "" || !lines.slice(1).every((line) => line.startsWith(indent))) return source;
  return [lines[0], ...lines.slice(1).map((line) => line.slice(indent.length))].join("\n");
}

/**
 * Read the arguments for the Fields view.
 *
 * The keys are listed in the order the text gives them, and each non-string
 * value is shown as its own source text rather than re-serialised, for the
 * reason {@link copyableJson} gives. A key the text repeats is listed each
 * time: the view shows what was sent, not what a parser chose to keep.
 */
export function classifyArguments(json: string): ArgumentView {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    return { kind: "raw", why: "These arguments are not valid JSON; they are shown as sent." };
  }
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
    return { kind: "raw", why: "These arguments have no named fields; they are shown as sent." };
  }

  // JSON.parse accepted the text, so the scan below walks well-formed input.
  const fields: ArgumentField[] = [];
  let index = skipSpace(json, 0) + 1;
  for (;;) {
    index = skipSpace(json, index);
    if (json[index] !== '"') break;
    const keyEnd = stringEnd(json, index);
    const key = JSON.parse(json.slice(index, keyEnd)) as string;
    index = skipSpace(json, skipSpace(json, keyEnd) + 1);
    const end = valueEnd(json, index);
    const source = json.slice(index, end);
    const string = source.startsWith('"');
    fields.push({ key, string, text: string ? (JSON.parse(source) as string) : dedent(source) });
    index = skipSpace(json, end);
    if (json[index] !== ",") break;
    index += 1;
  }
  return { kind: "fields", fields };
}

// ---------------------------------------------------------------------------
// Size and folding
// ---------------------------------------------------------------------------

/**
 * How many characters of a string a decided view shows before folding it.
 * A pending view never folds: hiding the tail of what is about to run is the
 * attack the block exists to defeat.
 */
export const FOLD_AT = 240;

/** Above this many bytes or lines, the block says it scrolls. */
export const LARGE_BYTES = 2_000;
export const LARGE_LINES = 24;

/** The UTF-8 size of some text. */
export function byteSize(text: string): number {
  return new TextEncoder().encode(text).length;
}

/** A size as `N.N kB`. */
export function kilobytes(bytes: number): string {
  return `${(bytes / 1000).toFixed(1)} kB`;
}

/** The first `FOLD_AT` code points of a long string, or `null` when it is short. */
export function folded(text: string): string | null {
  const chars = Array.from(text);
  return chars.length > FOLD_AT ? chars.slice(0, FOLD_AT).join("") : null;
}

/**
 * How many lines the Fields view draws: each value's decoded text, where a
 * string's `\n` escapes have become line breaks. The JSON text can be one
 * line while the view it produces is hundreds.
 */
export function drawnLines(fields: readonly ArgumentField[]): number {
  return fields.reduce((sum, field) => sum + field.text.split("\n").length, 0);
}

/**
 * The line above a large payload saying how large it is and that it scrolls.
 *
 * `lines` is what the view on screen draws, when that differs from the JSON
 * text's own line count, as it does for Fields; the note is about what the
 * operator has to scroll through, not about the encoding.
 */
export function sizeNote(json: string, lines = json.split("\n").length): string | null {
  const bytes = byteSize(json);
  if (bytes <= LARGE_BYTES && lines <= LARGE_LINES) return null;
  return `${kilobytes(bytes)} over ${lines} ${lines === 1 ? "line" : "lines"}, all of it shown. Scroll to read to the end.`;
}

/** A blank stretch at least this many line breaks deep is called out. */
export const BLANK_LINES = 3;
/** A run of spaces and tabs at least this long is called out. */
export const BLANK_RUN = 80;
const BLANK_BREAKS = new RegExp(`\\n(?:[ \\t\\r]*\\n){${BLANK_LINES - 1},}`);
const BLANK_SPACES = new RegExp(`[ \\t]{${BLANK_RUN},}`);

/**
 * Whether some text holds a stretch of blank space long enough to push what
 * follows it out of sight: several line breaks with nothing between them, or
 * a long run of spaces, which wraps into blank lines of its own. Either way a
 * reader sees a short value and then nothing, and the rest of it is below.
 */
export function hasBlankStretch(text: string): boolean {
  return BLANK_BREAKS.test(text) || BLANK_SPACES.test(text);
}


/** The warning for a blank stretch, or `null` when there is none. */
export function blankWarning(texts: readonly string[]): string | null {
  if (!texts.some(hasBlankStretch)) return null;
  return "A value has a long stretch of blank space with more after it. Scroll to read to the end.";
}

/**
 * The marks drawn either side of a value in the Fields view.
 *
 * A string is drawn in quotes and nothing else is, so the string `"false"`
 * cannot be read as the boolean `false`, nor `"10"` as the number: a tool that
 * treats any non-empty string as true would act on the one while the card
 * read as the other.
 */
export function delimiters(field: Pick<ArgumentField, "string">): readonly [string, string] {
  return field.string ? ['"', '"'] : ["", ""];
}
