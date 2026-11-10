// The README's pictures: the hero, how a bounty pays, and what a claim cannot do.
// Each is an SVG in a light and a dark version, in the palette of github.com/hey-research-lab, with no dependency.
// Every picture is complete at rest: animation only adds, because GitHub mobile and thumbnails may not play it,
// and it stops for readers who ask for reduced motion.
//   node scripts/visuals.mjs        writes assets/*-light.svg and assets/*-dark.svg
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const FONT = "-apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif";
const MONO = "ui-monospace,SFMono-Regular,'SF Mono',Menlo,Consolas,monospace";
export const THEMES = {
  dark: { bg: '#0B0E11', panel: '#14181D', line: '#272D35', ink: '#E9ECF0', sec: '#94A0AD', faint: '#3A424D', lime: '#C2FB03', limeSoft: 'rgba(194,251,3,0.16)', markInk: '#E9ECF0' },
  light: { bg: '#F5F6F8', panel: '#FFFFFF', line: '#DDE1E7', ink: '#0B0E11', sec: '#535B66', faint: '#C5CBD3', lime: '#4F7A00', limeSoft: 'rgba(79,122,0,0.14)', markInk: '#0B0E11' },
};
// The mark's stroke is the brand lime in both themes, as on heyresearch.xyz.
const BRAND_LIME = '#C2FB03';
const REDUCED = '@media (prefers-reduced-motion: reduce){*{animation:none!important}}';

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

/** Greedy word wrap to at most `max` characters a line. */
export function wrap(text, max) {
  const lines = [''];
  for (const word of String(text).split(/\s+/)) {
    const next = `${lines.at(-1)} ${word}`.trim();
    if (next.length > max && lines.at(-1)) lines.push(word);
    else lines[lines.length - 1] = next;
  }
  return lines;
}
const textLines = (lines, { x, y, step, size, fill, weight = 400, family = FONT, anchor = 'start' }) =>
  lines.map((line, i) => `<text x="${x}" y="${y + i * step}" text-anchor="${anchor}" font-family="${family}" font-size="${size}" font-weight="${weight}" fill="${fill}">${esc(line)}</text>`).join('');

/** The HEY mark (the letter He: a hooked bar and stem, and the lime stroke), drawn in a 136 x 128 box. */
function mark(x, y, height, theme) {
  const t = THEMES[theme];
  const s = (height / 128).toFixed(4);
  return `<g transform="translate(${x} ${y}) scale(${s})">
<path d="M26 21C27 14 31 8.5 35.5 4.5C35 10.5 37.5 16.5 44 16.5L101 16.5C106.5 16.5 110.5 20.5 110.5 26L110.5 96C110.5 106 103 114 93 118.5L93.8 45L35 45C29.5 45 27.2 41.5 27 37Z" fill="${t.markInk}"/>
<path d="M49 54.5C42 57.5 34.5 62 32.5 70.5L30 118C38.5 115 46 108 46.2 98C46.4 88 43 80.5 43.2 72C43.4 64 46 58.5 49 54.5Z" fill="${BRAND_LIME}"/>
</g>`;
}

/** A small check mark in a circle, centred on (x, y). */
const check = (x, y, t, cls = '', style = '') =>
  `<g class="${cls}" style="${style}"><circle cx="${x}" cy="${y}" r="9" fill="${t.limeSoft}"/><path d="M${x - 4} ${y}L${x - 1} ${y + 3.2}L${x + 4.5} ${y - 3.5}" fill="none" stroke="${t.lime}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></g>`;

// ---------------------------------------------------------------- the hero

/** "Proof of Ship on NEAR": the joint mark in type, the headline, and the checks a claim passes before it pays. */
export function hero(theme) {
  const t = THEMES[theme];
  const w = 1200, h = 360;
  const px = 744, py = 40, pw = 416, ph = h - 2 * py;
  const checks = [
    ['domain', 'chainId 397 · salt of this escrow'],
    ['signer', 'env::ecrecover = HEY attester'],
    ['window', 'from bounty start to deadline'],
    ['fresh', 'signed less than a day ago'],
    ['ships', 'count ≥ the bounty’s target'],
  ];
  const rowY = (i) => py + 68 + i * 32;
  const rows = checks.map(([key, value], i) => {
    const y = rowY(i);
    return `<text x="${px + 24}" y="${y + 4}" font-family="${MONO}" font-size="13" fill="${t.sec}">${esc(key)}</text>
<text x="${px + 96}" y="${y + 4}" font-family="${MONO}" font-size="13" fill="${t.ink}">${esc(value)}</text>
${check(px + pw - 30, y, t, 'ok', `animation-delay:${(0.5 + i * 0.55).toFixed(2)}s`)}`;
  }).join('\n');
  const payY = rowY(checks.length) - 6;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" role="img" aria-label="Proof of Ship on NEAR, by HEY Research Lab. Get paid for shipping, on evidence anyone can check. A claim pays the builder in USDC only after the escrow checks HEY's signed ship count on NEAR.">
<style>.ok{animation:ok 7s ease-out infinite}@keyframes ok{0%{opacity:.15}8%,100%{opacity:1}}.pay{animation:pay 7s ease-out infinite}@keyframes pay{0%,40%{opacity:.4}50%,100%{opacity:1}}.cur{animation:blink 1.1s steps(1) infinite}@keyframes blink{50%{opacity:0}}${REDUCED}</style>
<rect width="${w}" height="${h}" rx="14" fill="${t.bg}"/>
${mark(56, 44, 34, theme)}
<text x="102" y="68" font-family="${MONO}" font-size="14" letter-spacing="3" fill="${t.sec}">HEY RESEARCH LAB</text>
<text x="300" y="68" font-family="${MONO}" font-size="15" fill="${t.sec}">×</text>
<text x="324" y="68" font-family="${MONO}" font-size="14" font-weight="700" letter-spacing="3" fill="${t.ink}">NEAR</text>
<text x="56" y="124" font-family="${MONO}" font-size="14" letter-spacing="3" fill="${t.lime}">BUILT AT NEAR HACKATHON<tspan class="cur"> ▌</tspan></text>
<text x="56" y="184" font-family="${FONT}" font-size="50" font-weight="800" letter-spacing="-1.8" fill="${t.ink}">Proof of Ship</text>
<text x="56" y="240" font-family="${FONT}" font-size="50" font-weight="800" letter-spacing="-1.8" fill="${t.lime}">on NEAR</text>
<text x="56" y="286" font-family="${FONT}" font-size="20" fill="${t.sec}">Get paid for shipping, on evidence anyone can check.</text>
<text x="56" y="318" font-family="${MONO}" font-size="13" fill="${t.sec}">USDC escrow · EIP-712 · env::ecrecover · eth-implicit payout</text>
<rect x="${px}" y="${py}" width="${pw}" height="${ph}" rx="12" fill="${t.panel}" stroke="${t.line}"/>
<circle cx="${px + 22}" cy="${py + 22}" r="4" fill="${t.faint}"/><circle cx="${px + 36}" cy="${py + 22}" r="4" fill="${t.faint}"/><circle cx="${px + 50}" cy="${py + 22}" r="4" fill="${t.faint}"/>
<text x="${px + pw - 20}" y="${py + 26}" text-anchor="end" font-family="${MONO}" font-size="12" fill="${t.sec}">claim(id, attestation, signature)</text>
<line x1="${px}" x2="${px + pw}" y1="${py + 42}" y2="${py + 42}" stroke="${t.line}"/>
${rows}
<g class="pay"><rect x="${px + 18}" y="${payY - 4}" width="${pw - 36}" height="44" rx="10" fill="${t.limeSoft}" stroke="${t.lime}"/>
<text x="${px + 34}" y="${payY + 23}" font-family="${MONO}" font-size="13" font-weight="700" fill="${t.lime}">PAID</text>
<text x="${px + 84}" y="${payY + 23}" font-family="${MONO}" font-size="13" fill="${t.ink}">USDC → the builder’s 0x… account</text></g>
</svg>
`;
}

// ---------------------------------------------------------------- how a bounty pays

/** Four steps, then the bounty's clock: opened, the deadline, the 3-day claim grace, refunds. */
export function flow(theme) {
  const t = THEMES[theme];
  const w = 1200, h = 480;
  const steps = [
    ['Fund', 'A funder opens a USDC bounty behind a project with a target, say 3 ships in 14 days, and funds it.', 'create · ft_transfer_call'],
    ['Count', 'HEY counts the project’s ships, each with its public source, and signs the count as EIP-712.', 'GET …/proof-of-ship/attestation'],
    ['Verify', 'Anyone submits the claim. The escrow rebuilds the digest and recovers the signer on NEAR.', 'env::keccak256 · env::ecrecover'],
    ['Pay', 'The builder is paid at its eth-implicit account, the address HEY signed, or into NEAR Intents.', 'ft_transfer → 0x… | intents.near'],
  ];
  const left = 40, top = 100, cw = 250, gap = 40, ch = 200;
  const cx = (i) => left + i * (cw + gap);
  const cards = steps.map(([title, body, code], i) => {
    const x = cx(i);
    return `<g><rect x="${x}" y="${top}" width="${cw}" height="${ch}" rx="12" fill="${t.panel}" stroke="${t.line}"/>
<text x="${x + 20}" y="${top + 34}" font-family="${MONO}" font-size="13" letter-spacing="2" fill="${t.lime}">0${i + 1}</text>
<text x="${x + 20}" y="${top + 66}" font-family="${FONT}" font-size="21" font-weight="700" fill="${t.ink}">${esc(title)}</text>
${textLines(wrap(body, 31), { x: x + 20, y: top + 94, step: 20, size: 14.5, fill: t.sec })}
<line x1="${x + 20}" x2="${x + cw - 20}" y1="${top + ch - 38}" y2="${top + ch - 38}" stroke="${t.line}"/>
<text x="${x + 20}" y="${top + ch - 16}" font-family="${MONO}" font-size="11.5" fill="${t.ink}">${esc(code)}</text></g>`;
  }).join('\n');
  const wires = [0, 1, 2].map((i) => {
    const x1 = cx(i) + cw + 4, x2 = cx(i + 1) - 4, y = top + ch / 2;
    return `<path class="w" style="animation-delay:${(i * 0.4).toFixed(1)}s" d="M${x1} ${y}H${x2 - 6}" stroke="${t.lime}" stroke-width="2"/><path d="M${x2 - 8} ${y - 5}L${x2} ${y}L${x2 - 8} ${y + 5}" fill="none" stroke="${t.lime}" stroke-width="2" stroke-linejoin="round"/>`;
  }).join('');
  // The clock: opened, the deadline (the last moment a ship's window may end), the deadline plus 3 days (the last
  // claim), then refunds. Not to scale: the window is 1 to 365 days, the grace always 3.
  const ty = 380, a = left + 16, d = 760, g = 960, r = w - 56;
  const tick = (x, label, note, anchor) => `<line x1="${x}" x2="${x}" y1="${ty - 8}" y2="${ty + 8}" stroke="${t.ink}" stroke-width="2"/><text x="${x}" y="${ty + 30}" text-anchor="${anchor}" font-family="${MONO}" font-size="13" font-weight="600" fill="${t.ink}">${esc(label)}</text><text x="${x}" y="${ty + 50}" text-anchor="${anchor}" font-family="${FONT}" font-size="13" fill="${t.sec}">${esc(note)}</text>`;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" role="img" aria-label="How a bounty pays: a funder opens and funds a USDC bounty; HEY counts ships with sources and signs EIP-712; the contract verifies with env::ecrecover on NEAR; the builder is paid at its eth-implicit account or via NEAR Intents. A claim is accepted until the deadline plus 3 days; after that each funder refunds exactly what it put in.">
<style>.w{stroke-dasharray:6 6;animation:flow 1.6s linear infinite}@keyframes flow{to{stroke-dashoffset:-24}}.dot{animation:travel 9s ease-in-out infinite}@keyframes travel{0%,8%{transform:translateX(0)}85%,100%{transform:translateX(${g - a}px)}}${REDUCED}</style>
<rect width="${w}" height="${h}" rx="14" fill="${t.bg}"/>
<text x="40" y="50" font-family="${MONO}" font-size="13" letter-spacing="2" fill="${t.lime}">HOW A BOUNTY PAYS</text>
<text x="40" y="76" font-family="${FONT}" font-size="15" fill="${t.sec}">Four steps on NEAR. Nobody has to trust the funder to press pay, and the submitter cannot redirect anything.</text>
${cards}
${wires}
<line x1="${a}" x2="${d}" y1="${ty}" y2="${ty}" stroke="${t.lime}" stroke-width="3"/>
<line x1="${d}" x2="${g}" y1="${ty}" y2="${ty}" stroke="${t.lime}" stroke-width="3" stroke-dasharray="6 5"/>
<line x1="${g}" x2="${r}" y1="${ty}" y2="${ty}" stroke="${t.faint}" stroke-width="3"/>
${tick(a, 'Opened', 'the window starts', 'start')}
${tick(d, 'Deadline', 'last moment a counted window may end', 'middle')}
${tick(g, '+ 3 days', 'last claim', 'middle')}
<text x="${r}" y="${ty + 30}" text-anchor="end" font-family="${MONO}" font-size="13" font-weight="600" fill="${t.ink}">Refunds</text>
<text x="${r}" y="${ty + 50}" text-anchor="end" font-family="${FONT}" font-size="13" fill="${t.sec}">each funder, exactly</text>
<circle class="dot" cx="${a}" cy="${ty}" r="6" fill="${t.lime}"/>
<text x="40" y="${h - 14}" font-family="${MONO}" font-size="12" fill="${t.sec}">Not to scale: a bounty runs 1 to 365 days. A verified bounty is owed to its builder and is never refunded.</text>
</svg>
`;
}

// ---------------------------------------------------------------- what a claim cannot do

/** The refusals, each a unit test, with the signing domain on top and the refunds below. */
export function security(theme) {
  const t = THEMES[theme];
  const cards = [
    ['Replay', 'The same claim again, before or after the payout.', 'ERR_ALREADY_CLAIMED'],
    ['High-s twin', 'The other valid signature of the same key (EIP-2), refused before ecrecover.', 'ERR_HIGH_S'],
    ['Robinhood Chain signature', 'A count signed for chain 4663 with a verifyingContract.', 'ERR_WRONG_SIGNER'],
    ['Testnet on mainnet', 'A chain 398 signature sent to a chain 397 escrow.', 'ERR_WRONG_SIGNER'],
    ['Another escrow', 'A signature salted with another escrow’s account id.', 'ERR_WRONG_SIGNER'],
    ['Wrong attester', 'Any key but the one address fixed at init.', 'ERR_WRONG_SIGNER'],
    ['Tampered count', 'Ships raised after HEY signed.', 'ERR_WRONG_SIGNER'],
    ['Below target', 'Fewer ships than the bounty’s min_ships.', 'ERR_NOT_ENOUGH_SHIPS'],
    ['Stale or future', 'Signed more than a day ago, or dated ahead of the block.', 'ERR_STALE_ATTESTATION'],
    ['Wrong window', 'Not from the bounty’s start, or ending after the deadline or issuedAt.', 'ERR_WRONG_WINDOW'],
    ['Wrong builder or project', 'Another payout address, or another HEY slug.', 'ERR_WRONG_BUILDER · _PROJECT'],
    ['Too late', 'After the deadline plus the 3-day claim grace.', 'ERR_CLAIM_CLOSED'],
  ];
  const w = 1200, cols = 4, gap = 14, cw = (w - 48 - gap * (cols - 1)) / cols, chH = 128;
  const top = 168;
  const rowsN = Math.ceil(cards.length / cols);
  const refundY = top + rowsN * (chH + gap) + 6;
  const h = refundY + 120;
  const grid = cards.map(([title, body, code], i) => {
    const x = 24 + (i % cols) * (cw + gap), y = top + Math.floor(i / cols) * (chH + gap);
    return `<g><rect x="${x.toFixed(1)}" y="${y}" width="${cw.toFixed(1)}" height="${chH}" rx="10" fill="${t.panel}" stroke="${t.line}"/>
<text x="${(x + 18).toFixed(1)}" y="${y + 32}" font-family="${FONT}" font-size="17" font-weight="700" fill="${t.ink}">${esc(title)}</text>
${textLines(wrap(body, 34), { x: (x + 18).toFixed(1), y: y + 56, step: 18, size: 13.5, fill: t.sec })}
<rect class="p" style="animation-delay:-${(i * 0.45).toFixed(2)}s" x="${(x + 18).toFixed(1)}" y="${y + chH - 18}" width="5" height="5" rx="2.5" fill="${t.lime}"/>
<text x="${(x + 30).toFixed(1)}" y="${y + chH - 12}" font-family="${MONO}" font-size="11.5" fill="${t.lime}">${esc(code)}</text></g>`;
  }).join('\n');
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" role="img" aria-label="What a claim cannot do. Refused, each by a unit test: replay, a high-s twin signature, a Robinhood Chain signature, a testnet signature on mainnet, another escrow's signature, a wrong attester, a tampered count, below target, a stale or future attestation, a wrong window, a wrong builder or project, and a claim after the deadline plus 3 days. Not claimed in time, each funder refunds exactly what it put in.">
<style>.p{animation:pulse 3.5s ease-in-out infinite}@keyframes pulse{50%{opacity:.35}}${REDUCED}</style>
<rect width="${w}" height="${h}" rx="14" fill="${t.bg}"/>
<text x="40" y="50" font-family="${MONO}" font-size="13" letter-spacing="2" fill="${t.lime}">WHAT A CLAIM CANNOT DO</text>
<text x="40" y="76" font-family="${FONT}" font-size="15" fill="${t.sec}">The escrow trusts one EVM key, fixed at init. Every refusal below is a unit test, and none of it has an admin override.</text>
<rect x="24" y="96" width="${w - 48}" height="56" rx="10" fill="${t.panel}" stroke="${t.line}"/>
<text x="42" y="120" font-family="${MONO}" font-size="12" letter-spacing="1.5" fill="${t.sec}">SIGNING DOMAIN</text>
<text x="42" y="140" font-family="${MONO}" font-size="13" fill="${t.ink}">name "HEY Proof of Ship" · version "1" · chainId <tspan fill="${t.lime}">397</tspan> (testnet 398) · salt <tspan fill="${t.lime}">keccak256(escrow account id)</tspan></text>
${grid}
<rect x="24" y="${refundY}" width="${w - 48}" height="76" rx="10" fill="${t.limeSoft}" stroke="${t.lime}"/>
<text x="42" y="${refundY + 32}" font-family="${FONT}" font-size="17" font-weight="700" fill="${t.ink}">Nothing shipped in time? Every funder takes back exactly what it put in.</text>
<text x="42" y="${refundY + 56}" font-family="${FONT}" font-size="13.5" fill="${t.sec}">refund(id) opens after the deadline plus 3 days. Refunding twice, refunding early or refunding a verified bounty is refused; so is funding with another token or after the deadline.</text>
<text x="40" y="${h - 16}" font-family="${MONO}" font-size="12" fill="${t.sec}">50 unit tests (19 named "refused") and 3 sandbox tests on a local NEAR node, run by CI on every push.</text>
</svg>
`;
}

// ---------------------------------------------------------------- write them

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const out = join(dirname(fileURLToPath(import.meta.url)), '..', 'assets');
  mkdirSync(out, { recursive: true });
  for (const theme of ['light', 'dark']) {
    writeFileSync(join(out, `hero-${theme}.svg`), hero(theme));
    writeFileSync(join(out, `flow-${theme}.svg`), flow(theme));
    writeFileSync(join(out, `security-${theme}.svg`), security(theme));
  }
  console.log(`wrote ${out}/{hero,flow,security}-{light,dark}.svg`);
}
