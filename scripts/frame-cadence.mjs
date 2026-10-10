#!/usr/bin/env node
// Reduce a fidget log run under FIDGET_TRACE_FRAMES and FIDGET_TRACE_CADENCE
// to the numbers #426 asks for. scripts/bench-frame-cadence-macos.sh writes the
// log and calls this with its sample window.
//
// Usage: node scripts/frame-cadence.mjs LOG [--from UNIX_MS] [--to UNIX_MS] [--json OUT]
//        node scripts/frame-cadence.mjs compare --a JSON... --b JSON...

import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { interpolate } from "../src/interpolate.js";

const EDGES = [0, 10, 14, 18, 20, 25, 34, 50];
// A 60 Hz frame is 16.7 ms, so a gap past this is a vsync the loop missed.
export const DROP_MS = 20;

const round = (value, places) => Number(value.toFixed(places));
const mean = (values) => values.reduce((sum, v) => sum + v, 0) / values.length;
// Nearest rank, so every reported percentile is a value that was measured.
const percentile = (sorted, p) => sorted[Math.ceil(p * sorted.length) - 1];
const deltas = (times) => times.slice(1).map((at, i) => at - times[i]);

function histogram(frameDeltas, tickDeltas) {
  const bin = (value) => EDGES.findLastIndex((edge) => value >= edge);
  return EDGES.map((edge, i) => ({
    bin: i + 1 < EDGES.length ? `${edge}-${EDGES[i + 1]}` : `${edge}+`,
    frames: frameDeltas.filter((d) => bin(d) === i).length,
    ticks: tickDeltas.filter((d) => bin(d) === i).length,
  }));
}

// A gap equal to DROP_MS is still one frame. Only a longer gap missed a vsync.
function summarizeGaps(gaps) {
  return {
    fps: gaps.length ? round(1000 / mean(gaps), 1) : null,
    drops: gaps.filter((d) => d > DROP_MS).length,
    histogram: histogram(gaps, []).map(({ bin, frames }) => ({ bin, count: frames })),
  };
}

export function summarizeTimestamps(timestamps) {
  return summarizeGaps(deltas(timestamps));
}

// Release process_log::init prefixes stderr with epoch seconds and keeps the
// terminal quiet. The trace line after that stamp is what the bench counts.
const PROCESS_LOG_LINE = /^\d+ ((?:frame|cadence-ticks|cadence|overlay|director):.*)$/;

export function unwrapProcessLog(text) {
  return text
    .split("\n")
    .map((line) => line.match(PROCESS_LOG_LINE)?.[1] ?? line)
    .join("\n");
}

// Each count covers the second before its stamp, so the first one in the
// window reaches back before it and is left out.
function countedHz(counts) {
  if (counts.length < 2) return null;
  const ticks = counts.slice(1).reduce((n, c) => n + c.ticks, 0);
  return round((ticks * 1000) / (counts.at(-1).at - counts[0].at), 1);
}

/**
 * @param {string} log
 * @param {{from?: number, to?: number}} window - Unix ms, inclusive
 */
export function analyze(log, { from = -Infinity, to = Infinity }) {
  log = unwrapProcessLog(log);
  const inWindow = (at) => at >= from && at <= to;
  const ticks = [];
  const placements = [];
  const counts = [];
  const overlays = new Map();
  for (const line of log.split("\n")) {
    const [kind, ...fields] = line.split(" ");
    if (kind === "frame:") {
      const placement = { at: Number(fields[0]), pos: fields[2] };
      placements.push(placement);
      if (inWindow(placement.at)) ticks.push(placement);
    }
    if (kind === "cadence-ticks:" && inWindow(Number(fields[0]))) {
      counts.push({ at: Number(fields[0]), ticks: Number(fields[1]) });
    }
    if (kind !== "cadence:") continue;
    const [label, now, rearmed, previous, latest] = fields;
    if (!inWindow(Number(now))) continue;
    if (!overlays.has(label)) overlays.set(label, []);
    overlays.get(label).push({
      now: Number(now),
      rearmed: rearmed === "1",
      previous: previous === "-" ? null : Number(previous),
      latest: Number(latest),
    });
  }

  // ponytail: an arrival reads the Engine position traced at or before it, by
  // wall clock, for one Instance. A still sprite draws where it stands, so it
  // has no lag to measure; counting it reports the 250 ms resend as lag.
  const engineAt = (at) => placements.findLast((p) => p.at <= at)?.pos;

  // A frame after one that did not ask for it is the loop starting again on an
  // arrival. The gap before it is time with nothing to draw, not a dropped frame.
  const frameDeltas = [];
  let restarts = 0;
  const lags = [];
  for (const frames of overlays.values()) {
    frames.forEach((frame, i) => {
      if (i > 0 && frames[i - 1].rearmed) frameDeltas.push(frame.now - frames[i - 1].now);
      if (i > 0 && !frames[i - 1].rearmed) restarts++;
      if (frame.previous === null) return;
      if (engineAt(frame.previous) === engineAt(frame.latest)) return;
      // Interpolating the arrival times themselves gives the moment whose
      // placement is on screen, by the renderer's own arithmetic.
      const shown = interpolate(
        { x: frame.previous, y: 0, at: frame.previous },
        { x: frame.latest, y: 0, at: frame.latest },
        frame.now,
      ).x;
      lags.push({
        ms: frame.now - shown,
        samples: (frame.now - shown) / (frame.latest - frame.previous),
        held: shown === frame.latest,
      });
    });
  }

  const tickDeltas = deltas(ticks.map((tick) => tick.at));
  const display = summarizeGaps(frameDeltas);
  // A moving tick waits toward the 16 ms deadline and a still one keeps the
  // wake-based pace, so the blended rate describes neither. A tick moved when
  // its traced position differs from the tick before it.
  const steps = ticks.slice(1).map((tick, i) => ({ gap: tick.at - ticks[i].at, moved: tick.pos !== ticks[i].pos }));
  const pace = (moved) => {
    const gaps = steps.filter((step) => step.moved === moved).map((step) => step.gap);
    return gaps.length
      ? { ticks: gaps.length, gapMs: round(mean(gaps), 1), hz: round(1000 / mean(gaps), 1) }
      : { ticks: 0, gapMs: null, hz: null };
  };
  const byMs = lags.map((l) => l.ms).sort((a, b) => a - b);
  const bySamples = lags.map((l) => l.samples).sort((a, b) => a - b);
  return {
    frames: [...overlays.values()].reduce((n, series) => n + series.length, 0),
    fps: display.fps,
    drops: display.drops,
    restarts,
    ticks: ticks.length,
    tickHz: tickDeltas.length ? round(1000 / mean(tickDeltas), 1) : null,
    moving: pace(true),
    still: pace(false),
    countedHz: countedHz(counts),
    lag: lags.length
      ? {
          p50Ms: round(percentile(byMs, 0.5), 1),
          p95Ms: round(percentile(byMs, 0.95), 1),
          p50Samples: round(percentile(bySamples, 0.5), 2),
          p95Samples: round(percentile(bySamples, 0.95), 2),
          held: lags.filter((l) => l.held).length,
        }
      : null,
    histogram: histogram(frameDeltas, tickDeltas),
  };
}

export function report(result) {
  const na = (value, unit = "") => (value === null ? "N/A" : `${value}${unit}`);
  const { lag } = result;
  const rows = [
    ["Display frames", result.frames],
    ["Mean fps", na(result.fps)],
    [`Dropped (>${DROP_MS} ms)`, result.drops],
    ["Loop restarts", result.restarts],
    ["Engine ticks", `${result.ticks} (${na(result.tickHz, " Hz")})`],
    ["Engine ticks/s, loop counter", na(result.countedHz)],
    ...["moving", "still"].map((kind) => {
      const { ticks, gapMs, hz } = result[kind];
      return [`Engine ticks, ${kind}`, `${ticks} (${na(gapMs === null ? null : `${gapMs} ms, ${hz} Hz`)})`];
    }),
    ["Interpolation lag p50, moving", lag ? `${lag.p50Ms} ms (${lag.p50Samples} samples)` : "N/A"],
    ["Interpolation lag p95, moving", lag ? `${lag.p95Ms} ms (${lag.p95Samples} samples)` : "N/A"],
    ["Moving frames held at the latest placement", lag ? lag.held : "N/A"],
  ];
  return [
    "| Metric | Value |",
    "|---|---|",
    ...rows.map(([name, value]) => `| ${name} | ${value} |`),
    "",
    "| Delta ms | Display frames | Engine ticks |",
    "|---|---|---|",
    ...result.histogram.map((b) => `| ${b.bin} | ${b.frames} | ${b.ticks} |`),
  ].join("\n");
}

const COMPARED = [
  ["Mean fps", (r) => r.fps],
  [`Dropped (>${DROP_MS} ms)`, (r) => r.drops],
  ["Engine ticks/s, loop counter", (r) => r.countedHz],
  ["Engine ticks, still (Hz)", (r) => r.still.hz],
  ["Engine ticks, still (ms)", (r) => r.still.gapMs],
  ["Engine ticks, moving (Hz)", (r) => r.moving.hz],
  ["Engine ticks, moving (ms)", (r) => r.moving.gapMs],
  ["Interpolation lag p95, moving (ms)", (r) => r.lag?.p95Ms ?? null],
  ["Moving frames held at the latest placement", (r) => r.lag?.held ?? null],
];

/**
 * Each side's mean over its rounds, the rounds themselves, and B minus A.
 * @param {ReturnType<typeof analyze>[]} a
 * @param {ReturnType<typeof analyze>[]} b
 */
export function compare(a, b) {
  const na = (value) => (value === null ? "N/A" : `${value}`);
  const side = (results, metric) => {
    const values = results.map(metric);
    const measured = values.filter((v) => v !== null);
    return { mean: measured.length ? round(mean(measured), 1) : null, values };
  };
  const rows = COMPARED.map(([name, metric]) => {
    const [left, right] = [side(a, metric), side(b, metric)];
    const diff = left.mean === null || right.mean === null ? null : round(right.mean - left.mean, 1);
    const cell = ({ mean, values }) => `${na(mean)} (${values.map(na).join(", ")})`;
    return `| ${name} | ${cell(left)} | ${cell(right)} | ${na(diff)} |`;
  });
  return ["| Metric | A | B | B - A |", "|---|---|---|---|", ...rows].join("\n");
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [path, ...args] = process.argv.slice(2);
  if (path === "unwrap") {
    const file = args[0];
    if (!file || args.length !== 1) {
      console.error("usage: node scripts/frame-cadence.mjs unwrap PROCESS_LOG");
      process.exit(2);
    }
    const text = unwrapProcessLog(readFileSync(file, "utf8"));
    process.stdout.write(text.endsWith("\n") || text.length === 0 ? text : `${text}\n`);
    process.exit(0);
  }
  if (path === "compare") {
    const sides = { "--a": [], "--b": [] };
    let files;
    let stray = false;
    for (const arg of args) {
      if (arg in sides) files = sides[arg];
      else if (files) files.push(JSON.parse(readFileSync(arg, "utf8")));
      else stray = true;
    }
    if (stray || !sides["--a"].length || !sides["--b"].length) {
      console.error("usage: node scripts/frame-cadence.mjs compare --a JSON... --b JSON...");
      process.exit(2);
    }
    console.log(compare(sides["--a"], sides["--b"]));
    process.exit(0);
  }
  if (!path) {
    console.error("usage: node scripts/frame-cadence.mjs LOG [--from UNIX_MS] [--to UNIX_MS] [--json OUT]");
    process.exit(2);
  }
  const flag = (name) => {
    const i = args.indexOf(name);
    return i === -1 ? undefined : args[i + 1];
  };
  const ms = (name) => (flag(name) === undefined ? undefined : Number(flag(name)));
  const result = analyze(readFileSync(path, "utf8"), { from: ms("--from"), to: ms("--to") });
  if (flag("--json")) writeFileSync(flag("--json"), JSON.stringify(result));
  console.log(report(result));
}
