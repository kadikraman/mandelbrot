// The explainer's chapters. Prose marks maths with $…$, bold with **…** and
// code with `…`; `runs` turns it into the runs the page draws.

export type Run = { id: string; text: string; kind: string };
export type Para = { id: string; runs: Run[] };
export type CodeLine = { id: string; indent: number; runs: Run[] };
export type Claim = { id: string; stmt: Run[]; why: Run[] };
export type Source = { id: string; text: string; href: string };
export type Theme = { id: string; index: number; name: string; swatch: string };
export type Chapter = {
  id: string;
  number: number;
  title: string;
  formula: string;
  paras: Para[];
  control: string;
  codeFile: string;
  codeLang: string;
  code: CodeLine[];
  claims: Claim[];
  sources: Source[];
};

export function runs(prose: string, prefix = 'r'): Run[] {
  const out: Run[] = [];
  const pattern = /\$([^$]+)\$|\*\*([^*]+)\*\*|`([^`]+)`/g;
  let at = 0;
  for (const m of prose.matchAll(pattern)) {
    if (m.index > at) out.push({ id: '', text: prose.slice(at, m.index), kind: 'plain' });
    const [kind, text] = m[1] !== undefined ? ['math', m[1]] : m[2] !== undefined ? ['strong', m[2]] : ['code', m[3]];
    out.push({ id: '', text, kind });
    at = m.index + m[0].length;
  }
  if (at < prose.length) out.push({ id: '', text: prose.slice(at), kind: 'plain' });
  return out.map((r, i) => ({ ...r, id: `${prefix}${i}` }));
}

const KEYWORDS = new Set(['function', 'let', 'const', 'for', 'if', 'return', 'fn', 'mut', 'in', 'type', 'break', 'while']);

// Splits a line of JavaScript or Rust into comment (c), keyword (k), number (n),
// function name (f) and plain runs.
function highlight(line: string, prefix: string): Run[] {
  const out: Run[] = [];
  const push = (text: string, kind: string) => {
    const last = out.at(-1);
    if (last && last.kind === kind) last.text += text;
    else out.push({ id: '', text, kind });
  };
  const comment = line.indexOf('//');
  const code = comment < 0 ? line : line.slice(0, comment);
  for (const m of code.matchAll(/([A-Za-z_][A-Za-z0-9_]*)|(\d+(?:\.\d+)?)|([^A-Za-z0-9_]+)/g)) {
    if (m[1] !== undefined) {
      const next = code.slice(m.index + m[1].length);
      push(m[1], KEYWORDS.has(m[1]) ? 'k' : next.startsWith('(') && /\b(function|fn) $/.test(code.slice(0, m.index)) ? 'f' : 'plain');
    } else if (m[2] !== undefined) push(m[2], 'n');
    else push(m[3], 'plain');
  }
  if (comment >= 0) push(line.slice(comment), 'c');
  return out.map((r, i) => ({ ...r, id: `${prefix}${i}` }));
}

function code(source: string): CodeLine[] {
  return source.split('\n').map((line, i) => {
    const indent = line.length - line.trimStart().length;
    return { id: `l${i}`, indent: indent / 2, runs: highlight(line.trimStart(), `l${i}r`) };
  });
}

function claim(i: number, stmt: string, why: string): Claim {
  return { id: `c${i}`, stmt: runs(stmt, `c${i}s`), why: runs(why, `c${i}w`) };
}

function sources(list: [string, string][]): Source[] {
  return list.map(([text, href], i) => ({ id: `s${i}`, text, href }));
}

const WIKIPEDIA: [string, string] = ['Mandelbrot set, Wikipedia ↗', 'https://en.wikipedia.org/wiki/Mandelbrot_set'];

function chapter(number: number, title: string, rest: Partial<Chapter> = {}): Chapter {
  return {
    id: `ch${number}`,
    number,
    title,
    formula: '',
    paras: [{ id: 'p0', runs: runs('This chapter is still being written.') }],
    control: 'none',
    codeFile: '',
    codeLang: '',
    code: [],
    claims: [],
    sources: [],
    ...rest,
  };
}

export const CHAPTERS: Chapter[] = [
  chapter(1, 'One number and one rule', {
    formula: 'z → z² + c',
    paras: [
      'Pick a number and call it $c$. Start with $z = 0$. Now apply one rule, again and again: square $z$, then add $c$.',
      'For most values of $c$, $z$ soon grows without limit. We say it **escapes**. For some values, $z$ stays small forever, however long you keep going.',
      'The Mandelbrot set is every $c$ that stays. That is the whole definition. Each pixel on this page is one $c$: black pixels stay, coloured pixels escape.',
      'One small catch: $c$ is a point on a flat plane, not a number on a line. Complex numbers are a tidy way to do arithmetic with points, so “across” and “up” is all you need.',
    ].map((p, i) => ({ id: `p${i}`, runs: runs(p, `p${i}r`) })),
    control: 'steps',
    codeFile: 'escape-time.js',
    codeLang: 'JavaScript',
    code: code(`// Steps until c = (cx, cy) escapes, or -1.
function escapeTime(cx, cy, maxSteps) {
  let x = 0, y = 0;  // z = 0
  for (let n = 0; n < maxSteps; n++) {
    // z² + c, one coordinate at a time
    const nx = x * x - y * y + cx;
    y = 2 * x * y + cy;
    x = nx;
    // |z| > 2 means gone for good
    if (x*x + y*y > 4) return n;
  }
  return -1; // stayed: “in the set”
}`),
    claims: [
      claim(1, '$M = {c ∈ ℂ : zₙ stays bounded}$, where $z₀ = 0$ and $zₙ₊₁ = zₙ² + c$.', 'This is the standard definition. Bounded means one disc holds every $zₙ$.'),
      claim(2, 'If $|zₙ| > 2$ at any step, the orbit escapes to infinity.', 'Suppose $|z| > 2$ and $|z| ≥ |c|$. Then $|z² + c| ≥ |z|² − |c| ≥ |z|(|z| − 1)$, so each step multiplies $|z|$ by more than 1, and the factor grows. If $|c| > 2$, the condition holds at $z₁ = c$. This is why the code tests `x*x + y*y > 4`.'),
      claim(3, '$M$ is closed, lies in the disc $|c| ≤ 2$, and is connected.', 'The first two follow from fact 2. Connectedness is much deeper: Douady and Hubbard proved it in 1982.'),
    ],
    sources: sources([
      WIKIPEDIA,
      ['A. Douady, J. H. Hubbard. Itération des polynômes quadratiques complexes. C. R. Acad. Sci. Paris 294, 1982.', ''],
      ['J. Milnor. Dynamics in One Complex Variable, 3rd ed. Princeton, 2006.', ''],
    ]),
  }),
  chapter(2, 'Follow a point', {
    paras: [
      'Hold the pointer over any spot. The dots trace its **orbit**: the values of $z$, one step at a time, as the rule runs.',
      'Outside the set, the orbit spirals outwards. Once it leaves the circle of radius 2, it never comes back.',
      'Inside the set, the orbit settles down. Sometimes it closes in on one point. Sometimes it hops around a loop of 2, 3 or more points, forever.',
      'Near the edge, a point can wander for thousands of steps before it decides. That indecision is where all the detail lives.',
    ].map((p, i) => ({ id: `p${i}`, runs: runs(p, `p${i}r`) })),
    control: 'pin',
    codeFile: 'orbit.rs',
    codeLang: 'Rust',
    code: code(`/// Every z on the way, for one point c.
type C64 = Complex<f64>;
fn orbit(c: C64, max: usize) -> Vec<C64> {
  let mut z = C64::new(0.0, 0.0);
  let mut path = Vec::with_capacity(max);
  for _ in 0..max {
    z = z * z + c;
    path.push(z);
    if z.norm_sqr() > 4.0 {
      break; // escaped: no way back
    }
  }
  path
}`),
    claims: [
      claim(1, 'The orbit of 0 is $0, c, c² + c, (c² + c)² + c, …$', 'Each term is $f(z) = z² + c$ applied to the term before. Chapter 1’s test stops it as soon as $|z| > 2$.'),
      claim(2, '$f$ has at most one attracting cycle. If it has one, the orbit of 0 falls into it.', 'Fatou showed that every attracting cycle of a polynomial attracts a critical point. Since $f′(z) = 2z$, the only critical point is 0.'),
      claim(3, 'The period of that cycle is the same across a whole bulb of the set.', 'The cycle moves continuously with $c$ and cannot change its period while it stays attracting. Chapter 4 colours the bulbs by period.'),
    ],
    sources: sources([['J. Milnor. Dynamics in One Complex Variable, 3rd ed. Princeton, 2006. Section 8 covers Fatou’s theorem.', ''], WIKIPEDIA]),
  }),
  chapter(3, 'Colour by escape time'),
  chapter(4, 'The big cardioid and its bulbs'),
  chapter(5, 'Julia sets'),
  chapter(6, 'The deep zoom'),
  chapter(7, 'How this page works'),
];

// The picture's colour themes, in the order of the shader's palettes. Each
// swatch is the palette sampled once around its loop.
export const THEMES: Theme[] = [
  ['Warm', 'rgb(255, 231, 167), rgb(191, 114, 42), rgb(64, 11, 3), rgb(0, 24, 88), rgb(64, 141, 213), rgb(191, 244, 252), rgb(255, 231, 167)'],
  ['Classic', 'rgb(0, 8, 99), rgb(33, 107, 204), rgb(237, 255, 255), rgb(255, 171, 0), rgb(0, 3, 0), rgb(0, 8, 99)'],
  ['Ice', 'rgb(3, 5, 20), rgb(8, 43, 92), rgb(38, 140, 191), rgb(191, 242, 255), rgb(26, 76, 128), rgb(3, 5, 20)'],
  ['Magma', 'rgb(5, 3, 15), rgb(82, 18, 120), rgb(184, 54, 120), rgb(250, 135, 79), rgb(252, 242, 191), rgb(5, 3, 15)'],
  ['Ink', 'rgb(242, 237, 224), rgb(178, 176, 168), rgb(89, 89, 89), rgb(153, 150, 145), rgb(224, 219, 209), rgb(242, 237, 224)'],
  ['Cool', 'rgb(88, 167, 167), rgb(3, 42, 42), rgb(42, 3, 3), rgb(167, 88, 88), rgb(252, 213, 213), rgb(213, 252, 252), rgb(88, 167, 167)'],
].map(([name, stops], index) => ({ id: name.toLowerCase(), index, name, swatch: `linear-gradient(90deg, ${stops})` }));
