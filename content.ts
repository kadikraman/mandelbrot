// The explainer, at three levels. Prose marks maths with $…$, bold with **…** and
// code with `…`; `runs` turns it into the runs the page draws.

export type Run = { id: string; text: string; kind: string };
export type Para = { id: string; runs: Run[] };
export type CodeLine = { id: string; indent: number; runs: Run[] };
export type Claim = { id: string; stmt: Run[]; why: Run[] };
export type Source = { id: string; text: string; href: string };
export type Theme = { id: string; index: number; name: string; swatch: string };
export type Section = { id: string; title: string; paras: Para[] };
export type Level = {
  id: string;
  index: number;
  label: string;
  formula: string;
  sections: Section[];
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

function sections(list: [string, string[]][]): Section[] {
  return list.map(([title, paras], i) => ({
    id: `s${i}`,
    title,
    paras: paras.map((p, j) => ({ id: `p${j}`, runs: runs(p, `s${i}p${j}r`) })),
  }));
}

function level(index: number, id: string, label: string, rest: Partial<Level>): Level {
  return { id, index, label, formula: '', sections: [], control: 'none', codeFile: '', codeLang: '', code: [], claims: [], sources: [], ...rest };
}

export const LEVELS: Level[] = [
  level(0, 'five', 'Like I’m 5', {
    sections: sections([
      ['What it is', [
        'Every spot on this picture has its own number. Take a spot’s number and do one small sum with it, again and again, each time using the last answer.',
        'For some spots the answer stays small forever. For others it runs away and gets huge. The black shape is every spot whose answer stays small. The colours show how fast the others run away.',
      ]],
      ['Why it’s cool', [
        'The rule is tiny: one sum, done again and again. But the edge of the black shape never gets simpler. Zoom in anywhere on the edge and you find spirals, seahorses and lightning.',
        'You even find **tiny copies of the whole shape**, hiding in the edge. Each copy has its own edge, full of its own copies. Nobody drew any of it. It all comes out of that one small sum.',
      ]],
      ['Where the colours come from', [
        'Each coloured dot is painted by how many turns its answer took to run away. Dots that run away at once get one colour, dots that take a few more turns get the next, and so on around a loop of colours, like a rainbow that starts again.',
        'Near the edge, dots take more and more turns before they run away, so the colours change quickly there. That is why the edge looks so busy and bright. Dots right next to the edge are painted darker, so the thinnest threads still show. The black dots never ran away, so they get no colour at all.',
        'The **Colours** button picks a different set of paints. The rule for which dot gets which paint stays the same.',
      ]],
      ['How this page draws it', [
        'Your computer’s graphics chip plays the game for every dot on the screen at once, millions of dots, many times a second.',
        'When you zoom very deep, the numbers get too small for the chip. So the page works out one spot very carefully, and every other dot keeps track of how far it is from that spot. Small numbers like that fit.',
      ]],
    ]),
  }),
  level(1, 'some', 'Some maths', {
    formula: 'z → z² + c',
    sections: sections([
      ['What it is', [
        'Pick a complex number $c$: a point on the plane, with an across part and an up part. Start with $z = 0$ and apply $z → z² + c$ again and again.',
        'If $|z|$ ever passes 2, it **escapes** to infinity. The Mandelbrot set is every $c$ for which $z$ never escapes. Each pixel is one $c$: black pixels stay, and the colour of the others shows how many steps they took to escape.',
      ]],
      ['Why it’s cool', [
        'The rule fits in one line, yet the edge of the set has detail at every scale. Zoom in and new shapes keep appearing, and small copies of the whole set turn up all along the edge.',
        'The set is also all **one piece**: every speck is joined to the rest by thin filaments. That is far from obvious from the pictures, and it took until 1982 to prove.',
      ]],
      ['Where the colours come from', [
        'A pixel’s colour comes from its **escape count** $n$: how many steps $z$ took to escape. Neighbouring pixels have close counts, so whole bands of pixels share a count. A plain count would give flat stripes.',
        'So the page smooths it. It keeps iterating until $|z| > 256$, then uses $n − log₂(log₂|z|²)$, a count with a fractional part that changes smoothly from pixel to pixel. That number picks a colour from a palette that loops, so the colours cycle again and again as the count grows.',
        'Pixels very close to the set are darkened by an estimate of their distance to it, so filaments thinner than a pixel stay visible. Black pixels did not escape within the step limit.',
      ]],
      ['How this page draws it', [
        'The GPU runs the loop for every pixel until $z$ escapes or a step limit. Points near the edge take longer to decide, so the limit grows as you zoom.',
        'The GPU’s 32-bit numbers run out of digits at about $10⁶×$. Deeper, the page uses **perturbation**: the CPU computes one point’s orbit with as many digits as the zoom needs, and each pixel iterates only its small difference from that orbit.',
      ]],
    ]),
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
  }),
  level(2, 'pro', 'Pro', {
    formula: 'M = { c ∈ ℂ : fⁿ(0) bounded }',
    sections: sections([
      ['What it is', [
        '$M$ is the set of $c ∈ ℂ$ for which the orbit of the critical point 0 under $f(z) = z² + c$ is bounded. It is the connectedness locus of the quadratic family: $c ∈ M$ exactly when the Julia set of $f$ is connected, and for $c ∉ M$ the Julia set is a Cantor set.',
      ]],
      ['Why it’s cool', [
        '$M$ is compact and connected (Douady and Hubbard, 1982), yet its boundary has Hausdorff dimension 2 (Shishikura, 1998). Small copies of $M$ lie arbitrarily close to every boundary point, and $M$ turns up in many other families of maps (McMullen, 2000).',
        'Whether $M$ is locally connected is still open. A proof would show that hyperbolic maps are dense in the quadratic family.',
      ]],
      ['Where the colours come from', [
        'Colour is a function of the continuous escape time $ν = n + 4 − log₂ log₂|zₙ|²$, with bailout radius 256 so that the correction is accurate. Since $log|zₙ| ≈ 2ⁿ G(c)$, where $G$ is the Green’s function of $M$, $ν ≈ −log₂ G(c) + const$: the colour bands are level sets of the potential, which is why they are smooth.',
        'The palette index is $0.45 log₂(ν + 1) + ν/512$, fed to a periodic palette. The logarithm keeps bands wide near the whole set; the linear term keeps several bands in a deep view, where every $ν$ is large and close to the others.',
        'The colour is multiplied by a shade from the exterior distance estimate $d ≈ |zₙ| ln|zₙ| / |zₙ′|$, measured in pixels, so filaments thinner than a pixel stay visible. Once the view rests, each pixel averages 16 jittered samples.',
      ]],
      ['How this page draws it', [
        '**Perturbation.** One reference orbit $Zₙ$ is computed in arbitrary precision, with bits scaled to the zoom. Each pixel iterates $δz ← 2Zₙδz + δz² + δc$ in f32 on the GPU.',
        '**Rebasing** (Zhuoran, 2021). When $|Zₙ + δz| < |δz|$, the pixel sets $δz ← Zₙ + δz$ and restarts the reference at 0. One reference then serves the whole view, with no glitch detection.',
        '**Bilinear approximation.** A table of steps $δz ← Aδz + Bδc$, each valid while $|δz| < r$, skips most iterations at depth.',
        'Pixels whose derivative $|dz/dz₁|$ falls below $10⁻³$ are taken as interior and stop early. Edges are shaded by the exterior distance estimate. f32’s exponent ends near $10⁻³⁸$, which is why the zoom stops near $10³⁴×$.',
      ]],
    ]),
    claims: [
      claim(1, '$M = {c ∈ ℂ : zₙ stays bounded}$, where $z₀ = 0$ and $zₙ₊₁ = zₙ² + c$.', 'This is the standard definition. Bounded means one disc holds every $zₙ$.'),
      claim(2, 'If $|zₙ| > 2$ at any step, the orbit escapes to infinity.', 'Suppose $|z| > 2$ and $|z| ≥ |c|$. Then $|z² + c| ≥ |z|² − |c| ≥ |z|(|z| − 1)$, so each step multiplies $|z|$ by more than 1, and the factor grows. If $|c| > 2$, the condition holds at $z₁ = c$. This is why the escape test is $|z|² > 4$.'),
      claim(3, '$M$ is closed, lies in the disc $|c| ≤ 2$, and is connected.', 'The first two follow from fact 2. Connectedness is much deeper: Douady and Hubbard proved it in 1982.'),
      claim(4, '$f$ has at most one attracting cycle. If it has one, the orbit of 0 falls into it.', 'Fatou showed that every attracting cycle of a polynomial attracts a critical point. Since $f′(z) = 2z$, the only critical point is 0.'),
      claim(5, 'The period of that cycle is the same across a whole hyperbolic component of $M$.', 'The cycle moves continuously with $c$ and cannot change its period while it stays attracting.'),
    ],
    sources: sources([
      ['Mandelbrot set, Wikipedia ↗', 'https://en.wikipedia.org/wiki/Mandelbrot_set'],
      ['A. Douady, J. H. Hubbard. Itération des polynômes quadratiques complexes. C. R. Acad. Sci. Paris 294, 1982.', ''],
      ['J. Milnor. Dynamics in One Complex Variable, 3rd ed. Princeton, 2006.', ''],
      ['M. Shishikura. The Hausdorff dimension of the boundary of the Mandelbrot set and Julia sets. Annals of Mathematics 147, 1998.', ''],
      ['C. McMullen. The Mandelbrot set is universal. In The Mandelbrot Set, Theme and Variations, Cambridge, 2000.', ''],
      ['C. Heiland-Allen. Deep zoom theory and practice (again), mathr.co.uk, 2022 ↗', 'https://mathr.co.uk/blog/2022-02-21_deep_zoom_theory_and_practice_again.html'],
    ]),
  }),
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
