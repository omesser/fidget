// Plots poll time against on-screen window count, as a dependency-free SVG,
// from the rows.tsv that `scripts/bench-window-list-macos.sh sweep` writes.

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const FRAME_US = 1e6 / 60;

const SERIES = [
  { label: "fidget idle poll, p95", prefix: "idle+", field: "p95_us", color: "#d97706", dash: "6 4" },
  { label: "fidget idle poll, median", prefix: "idle+", field: "median_us", color: "#d97706" },
  { label: "in-process microbench, median", prefix: "sweep-micro+", field: "median_us", color: "#2563eb" },
];

export function parseSweep(tsv) {
  const [header, ...lines] = tsv.trim().split("\n");
  const cols = header.split("\t");
  const rows = lines.map((line) => Object.fromEntries(line.split("\t").map((v, i) => [cols[i], v])));
  return SERIES.map((s) => ({
    ...s,
    points: rows
      .filter((r) => r.scenario.startsWith(s.prefix))
      .map((r) => ({ x: Number(r.windows), y: Number(r[s.field]) }))
      .filter((p) => Number.isFinite(p.x) && Number.isFinite(p.y))
      .sort((a, b) => a.x - b.x),
  })).filter((s) => s.points.length > 0);
}

export function renderSvg(series) {
  const w = 720;
  const h = 420;
  const m = { l: 64, r: 24, t: 24, b: 56 };
  const all = series.flatMap((s) => s.points);
  const xMax = Math.ceil(Math.max(...all.map((p) => p.x)) / 50) * 50;
  const yMaxMs = Math.ceil(Math.max(FRAME_US, ...all.map((p) => p.y)) / 1000 / 2) * 2;
  const x = (v) => m.l + (v / xMax) * (w - m.l - m.r);
  const y = (us) => h - m.b - (us / 1000 / yMaxMs) * (h - m.t - m.b);
  const out = [
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" font-family="system-ui, sans-serif" font-size="12">`,
    `<rect width="${w}" height="${h}" fill="#fff"/>`,
  ];
  for (let ms = 0; ms <= yMaxMs; ms += 2) {
    out.push(`<line x1="${m.l}" x2="${w - m.r}" y1="${y(ms * 1000)}" y2="${y(ms * 1000)}" stroke="#e5e7eb"/>`);
    out.push(`<text x="${m.l - 8}" y="${y(ms * 1000) + 4}" text-anchor="end">${ms}</text>`);
  }
  for (let n = 0; n <= xMax; n += 50) {
    out.push(`<text x="${x(n)}" y="${h - m.b + 18}" text-anchor="middle">${n}</text>`);
  }
  out.push(
    `<line x1="${m.l}" x2="${w - m.r}" y1="${y(FRAME_US)}" y2="${y(FRAME_US)}" stroke="#dc2626" stroke-dasharray="2 3"/>`,
    `<text x="${w - m.r}" y="${y(FRAME_US) - 6}" text-anchor="end" fill="#dc2626">one 60 Hz frame (16.7 ms)</text>`,
    `<text x="${(m.l + w - m.r) / 2}" y="${h - 12}" text-anchor="middle">on-screen windows (fidget's options)</text>`,
    `<text transform="translate(16 ${(m.t + h - m.b) / 2}) rotate(-90)" text-anchor="middle">ms per poll</text>`,
  );
  series.forEach((s, i) => {
    const d = s.points.map((p) => `${x(p.x).toFixed(1)},${y(p.y).toFixed(1)}`).join(" ");
    const dash = s.dash ? ` stroke-dasharray="${s.dash}"` : "";
    out.push(`<polyline points="${d}" fill="none" stroke="${s.color}" stroke-width="2"${dash}/>`);
    for (const p of s.points) {
      out.push(`<circle cx="${x(p.x).toFixed(1)}" cy="${y(p.y).toFixed(1)}" r="3" fill="${s.color}"/>`);
    }
    const ly = y(FRAME_US) + 28 + i * 18;
    out.push(`<line x1="${m.l + 12}" x2="${m.l + 36}" y1="${ly}" y2="${ly}" stroke="${s.color}" stroke-width="2"${dash}/>`);
    out.push(`<text x="${m.l + 42}" y="${ly + 4}">${s.label}</text>`);
  });
  out.push("</svg>");
  return `${out.join("\n")}\n`;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [input, output] = process.argv.slice(2);
  if (!input || !output) {
    console.error("usage: node scripts/plot-window-list-sweep.mjs rows.tsv out.svg");
    process.exit(2);
  }
  const series = parseSweep(readFileSync(input, "utf8"));
  if (series.length === 0) {
    console.error(`${input} has no sweep-micro+ or idle+ rows with numbers`);
    process.exit(1);
  }
  writeFileSync(output, renderSvg(series));
}
