// Original ink cast for QuickWay: everyday tools of a chat and coding connector,
// drawn as white bodies with black contours, dot eyes and one solid black patch.
// Every figure lives in a 100 × 100 symbol so the crowd and vignettes share one vocabulary.

const line = 'stroke="#000" stroke-width="3.2" stroke-linecap="round" stroke-linejoin="round"';
const eye = (x: number, y: number, r = 3.6, fill = '#000') => `<circle cx="${x}" cy="${y}" r="${r}" fill="${fill}"/>`;

const cast: Record<string, string> = {
  // Speech bubble with a black tuft and a waving arm.
  bubble: `<path d="M81 40c7-2 11-7 12-14" fill="none" ${line}/>
    <path d="M20 30c0-9 6-15 15-15h31c9 0 15 6 15 15v22c0 9-6 15-15 15H46L30 80l3-13c-8-1-13-7-13-15Z" fill="#fff" ${line}/>
    <path d="M60 15c1-8 9-12 16-8-1 7-8 10-16 8Z" fill="#000" ${line}/>
    ${eye(41, 38)}${eye(60, 38)}<path d="M44 49c4 4 10 4 14 0" fill="none" ${line}/>`,
  // Walking monitor: black screen, white eyes, antenna.
  bot: `<path d="M39 70l-3 16h-7M61 70l3 16h7M50 27V14" fill="none" ${line}/>
    <circle cx="50" cy="11" r="5" fill="#000" ${line}/>
    <rect x="21" y="27" width="58" height="45" rx="13" fill="#fff" ${line}/>
    <rect x="30" y="35" width="40" height="28" rx="8" fill="#000"/>
    ${eye(42, 47, 3.4, '#fff')}${eye(58, 47, 3.4, '#fff')}<path d="M46 55h8" stroke="#fff" stroke-width="2.6" stroke-linecap="round"/>`,
  // Plug with a curly cord.
  plug: `<path d="M50 76c0 9-15 6-15 17" fill="none" ${line}/>
    <rect x="35" y="13" width="9" height="20" rx="3" fill="#000" ${line}/><rect x="56" y="13" width="9" height="20" rx="3" fill="#000" ${line}/>
    <path d="M25 32h50v18c0 16-11 27-25 27S25 66 25 50Z" fill="#fff" ${line}/>
    ${eye(41, 48)}${eye(59, 48)}<path d="M44 58c4 3 8 3 12 0" fill="none" ${line}/>`,
  // Key wearing a beanie; the teeth are solid.
  key: `<rect x="50" y="43" width="39" height="11" rx="4" fill="#fff" ${line}/>
    <path d="M71 54v10h7V54M82 54v7h7v-7" fill="#000" ${line}/>
    <circle cx="33" cy="51" r="21" fill="#fff" ${line}/>
    <path d="M14 45c2-13 10-20 19-20s17 7 19 20c-6-3-12-4-19-4s-13 1-19 4Z" fill="#000" ${line}/>
    ${eye(26, 52)}${eye(40, 52)}<circle cx="33" cy="61" r="2.6" fill="#000"/>`,
  // Folder with a black tab, on two short legs.
  folder: `<path d="M36 79v10h-6M64 79v10h6" fill="none" ${line}/>
    <path d="M15 30c0-4 3-7 7-7h18l7 7h31c4 0 7 3 7 7v36c0 4-3 7-7 7H22c-4 0-7-3-7-7Z" fill="#fff" ${line}/>
    <path d="M15 30c0-4 3-7 7-7h18l7 7Z" fill="#000" ${line}/><path d="M15 39h70" fill="none" ${line}/>
    ${eye(40, 56)}${eye(60, 56)}<path d="M46 65c3 2 5 2 8 0" fill="none" ${line}/>`,
  // Little Q in outline form, sharing the app icon's capsule eyes.
  q: `<path d="M50 17c-20 0-33 13-33 32s13 32 33 32c5 0 10-1 14-3l9 6c3 2 7 0 7-4v-8c3-6 4-14 4-22 0-20-13-33-34-33Z" fill="#fff" ${line}/>
    <rect x="39" y="39" width="8" height="15" rx="4" fill="#000"/><rect x="54" y="39" width="8" height="15" rx="4" fill="#000"/>`,
  // Thinking spark with a tiny solid companion.
  spark: `<path d="M50 12c4 22 12 30 34 34-22 4-30 12-34 34-4-22-12-30-34-34 22-4 30-12 34-34Z" fill="#fff" ${line}/>
    <path d="M81 9c1 6 3 8 9 9-6 1-8 3-9 9-1-6-3-8-9-9 6-1 8-3 9-9Z" fill="#000" ${line}/>
    ${eye(44, 45, 3.2)}${eye(56, 45, 3.2)}`,
  // Cloud carrying a solid bolt: the connection itself.
  cloud: `<path d="M51 70l-7 12h8l-5 11 12-16h-8l5-7Z" fill="#000" ${line}/>
    <path d="M28 70c-9 0-15-6-15-14s6-14 14-14c1-10 9-17 19-17 10 0 17 6 19 15 9 0 17 6 17 15s-7 15-16 15Z" fill="#fff" ${line}/>
    ${eye(41, 52)}${eye(58, 52)}<path d="M45 61c3 2 6 2 9 0" fill="none" ${line}/>`,
  // Coffee mug for long sessions; the coffee is solid.
  mug: `<path d="M72 44h5c6 0 10 4 10 10s-4 10-10 10h-6M36 16c-3 4 3 7 0 11M50 14c-3 4 3 7 0 11" fill="none" ${line}/>
    <path d="M22 34h52v34c0 9-7 16-16 16H38c-9 0-16-7-16-16Z" fill="#fff" ${line}/>
    <path d="M22 34h52v9H22Z" fill="#000" ${line}/>
    ${eye(39, 58)}${eye(57, 58)}<path d="M45 67c2 2 4 2 6 0" fill="none" ${line}/>`,
};

export const castNames = Object.keys(cast);

export const sprite = `<svg class="sprite" width="0" height="0" aria-hidden="true" focusable="false"><defs>${Object.entries(cast).map(([name, body]) => `<symbol id="ch-${name}" viewBox="0 0 100 100" overflow="visible">${body}</symbol>`).join('')}</defs></svg>`;

export const figure = (name: string, cls = '') => `<svg class="figure ${cls}" viewBox="0 0 100 100" aria-hidden="true" focusable="false"><use href="#ch-${name}"/></svg>`;

// Deterministic per-cell randomness: resizing adds or removes edge figures without reshuffling the rest.
function cellRandom(row: number, col: number, salt: number) {
  let h = Math.imul(row * 374761393 + col * 668265263 + salt * 2147483647, 1274126177);
  h = Math.imul(h ^ (h >>> 13), 1103515245);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}

/** Fills `host` with an edge-to-edge staggered crowd sized to the host. */
export function drawCrowd(host: HTMLElement) {
  const { width, height } = host.getBoundingClientRect();
  if (!width || !height) return;
  const size = 118, advance = 58, rowAdvance = 84;
  const cols = Math.ceil(width / advance) + 2, rows = Math.ceil(height / rowAdvance) + 2;
  const uses: string[] = [];
  for (let r = -1; r < rows; r++) {
    const row: [number, string][] = [];
    for (let c = -1; c < cols; c++) {
      const pick = castNames[Math.floor(cellRandom(r, c, 1) * castNames.length)];
      const scale = 0.84 + cellRandom(r, c, 2) * 0.32;
      const s = size * scale;
      const x = c * advance + (r & 1) * advance / 2 + (cellRandom(r, c, 3) - 0.5) * 18 - s / 2;
      const y = r * rowAdvance + (cellRandom(r, c, 4) - 0.5) * 16 - s / 2;
      const angle = (cellRandom(r, c, 5) - 0.5) * 26;
      const flip = cellRandom(r, c, 6) > 0.5 ? -1 : 1;
      row.push([cellRandom(r, c, 7), `<use href="#ch-${pick}" width="100" height="100" transform="translate(${(x + s / 2).toFixed(1)} ${(y + s / 2).toFixed(1)}) rotate(${angle.toFixed(1)}) scale(${(flip * scale * size / 100).toFixed(3)} ${(scale * size / 100).toFixed(3)}) translate(-50 -50)"/>`]);
    }
    // Vary stacking within a row; later rows still overlap earlier ones.
    row.sort((a, b) => a[0] - b[0]).forEach(([, use]) => uses.push(use));
  }
  host.innerHTML = `<svg width="${Math.ceil(width)}" height="${Math.ceil(height)}" viewBox="0 0 ${Math.ceil(width)} ${Math.ceil(height)}" aria-hidden="true" focusable="false">${uses.join('')}</svg>`;
}
