/** Lightweight syntax highlighting for diff viewer lines. */

interface Token {
  text: string
  type: 'keyword' | 'string' | 'comment' | 'number' | 'type' | 'plain'
}

const KEYWORDS = new Set([
  // JS/TS
  'const', 'let', 'var', 'function', 'return', 'if', 'else', 'for', 'while', 'do',
  'switch', 'case', 'break', 'continue', 'throw', 'try', 'catch', 'finally',
  'new', 'delete', 'typeof', 'instanceof', 'void', 'in', 'of',
  'class', 'extends', 'super', 'this', 'import', 'export', 'from', 'default',
  'async', 'await', 'yield', 'static', 'get', 'set',
  'true', 'false', 'null', 'undefined',
  // Rust
  'fn', 'let', 'mut', 'pub', 'struct', 'enum', 'impl', 'trait', 'type', 'where',
  'use', 'mod', 'crate', 'self', 'Self', 'match', 'loop', 'move',
  'as', 'ref', 'unsafe', 'async', 'dyn', 'macro_rules',
  // Python
  'def', 'class', 'import', 'from', 'return', 'if', 'elif', 'else', 'for', 'while',
  'try', 'except', 'finally', 'with', 'as', 'yield', 'lambda', 'pass', 'raise',
  'True', 'False', 'None', 'and', 'or', 'not', 'is', 'in',
  // Go
  'func', 'package', 'import', 'type', 'struct', 'interface', 'map', 'chan',
  'go', 'defer', 'select', 'range', 'nil',
])

const TYPE_PATTERN = /^[A-Z][a-zA-Z0-9_]*$/

// Tokenize a single line of code
const TOKEN_RE = /("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|`(?:[^`\\]|\\.)*`)|(\/{2}.*|#\s.*)|(\b\d[\d_.]*(?:e[+-]?\d+)?\b)|(\b[a-zA-Z_]\w*\b)/g

export function tokenize(line: string): Token[] {
  const tokens: Token[] = []
  let lastIndex = 0

  TOKEN_RE.lastIndex = 0
  let m: RegExpExecArray | null
  while ((m = TOKEN_RE.exec(line)) !== null) {
    if (m.index > lastIndex) {
      tokens.push({ text: line.slice(lastIndex, m.index), type: 'plain' })
    }

    if (m[1]) {
      tokens.push({ text: m[0], type: 'string' })
    } else if (m[2]) {
      tokens.push({ text: m[0], type: 'comment' })
    } else if (m[3]) {
      tokens.push({ text: m[0], type: 'number' })
    } else if (m[4]) {
      const word = m[0]
      if (KEYWORDS.has(word)) {
        tokens.push({ text: word, type: 'keyword' })
      } else if (TYPE_PATTERN.test(word)) {
        tokens.push({ text: word, type: 'type' })
      } else {
        tokens.push({ text: word, type: 'plain' })
      }
    }

    lastIndex = TOKEN_RE.lastIndex
  }

  if (lastIndex < line.length) {
    tokens.push({ text: line.slice(lastIndex), type: 'plain' })
  }

  return tokens
}

export const TOKEN_CLASSES: Record<Token['type'], string> = {
  keyword: 'text-hl-keyword',
  string: 'text-hl-string',
  comment: 'text-hl-comment',
  number: 'text-hl-number',
  type: 'text-hl-type',
  plain: '',
}
