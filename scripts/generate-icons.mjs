#!/usr/bin/env node
/**
 * Generates every app icon asset from a single source glyph.
 *
 * The source of truth is the command slash: its contour in
 * `assets/brand/sync-glyph.svg`, and the liquid-glass rendition of it — a
 * gradient body under a lit rim and a convex sheen — in
 * `assets/brand/sync-glyph-liquid-glass.svg`. This script re-draws that glyph
 * on the two plates the platforms expect and writes the bundle assets Tauri
 * references from `src-tauri/tauri.conf.json`:
 *
 *   - macOS (`icon.icns`) uses Apple's icon grid: an 824x824 squircle centred
 *     on a 1024x1024 canvas. macOS does not mask app icons, so the rounding and
 *     the surrounding padding have to be baked in.
 *   - Everything else (Linux PNGs, Windows `.ico` and Store tiles) uses a
 *     full-bleed squircle, because those platforms draw the icon edge to edge.
 *
 * Small renditions drop back to flat ink; see `FLAT_MAX_SIZE`.
 *
 * Run with `pnpm icons` after changing the glyph.
 */

import { Buffer } from "node:buffer";
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const ICONS_DIR = join(ROOT, "src-tauri", "icons");
const APP_DIR = join(ROOT, "src", "app");

/** Glyph path and its bounding box, lifted verbatim from the source SVG. */
const GLYPH = {
  d: "M312 139 H340 C347 139 351 146 348 152 L224 367 C221 372 218 374 212 374 H184 C177 374 173 367 176 361 L300 146 C303 141 306 139 312 139 Z",
  x0: 176,
  y0: 139,
  x1: 348,
  y1: 374,
};

const INK = "#17191D";
const PLATE = "#FFFFFF";

/**
 * The liquid-glass rendition, lifted from `assets/brand/sync-glyph-liquid-glass.svg`:
 * the same contour moulded out of dark glass rather than laid down as flat ink.
 * Four passes stack into it — a shadow cast on the plate, a lit outer rim, the
 * gradient body under a soft specular highlight, and a convex sheen across the
 * face.
 *
 * The gradients and filters are stated in the glyph's own coordinates, so the
 * group transform that places the glyph on a plate scales the whole treatment
 * with it and no rendition needs its own numbers.
 */
const GLASS_DEFS = `  <defs>
    <linearGradient id="slash" x1="340" y1="139" x2="184" y2="374" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#202326"/>
      <stop offset="0.38" stop-color="#292C2F"/>
      <stop offset="1" stop-color="#4B4E51"/>
    </linearGradient>
    <linearGradient id="outerRim" x1="184" y1="374" x2="340" y2="139" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#B5B9BB" stop-opacity="0.78"/>
      <stop offset="0.48" stop-color="#85898C" stop-opacity="0.60"/>
      <stop offset="1" stop-color="#C4C7C9" stop-opacity="0.72"/>
    </linearGradient>
    <linearGradient id="convexSheen" x1="176" y1="245" x2="350" y2="272" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#FFFFFF" stop-opacity="0.26"/>
      <stop offset="0.18" stop-color="#FFFFFF" stop-opacity="0.08"/>
      <stop offset="0.48" stop-color="#FFFFFF" stop-opacity="0"/>
      <stop offset="0.80" stop-color="#000000" stop-opacity="0.08"/>
      <stop offset="1" stop-color="#FFFFFF" stop-opacity="0.10"/>
    </linearGradient>
    <filter id="slashShadow" x="-20%" y="-20%" width="160%" height="170%" color-interpolation-filters="sRGB">
      <feDropShadow dx="7" dy="9" stdDeviation="8" flood-color="#111216" flood-opacity="0.21"/>
    </filter>
    <filter id="outerRimSoft" x="-10%" y="-10%" width="120%" height="120%" color-interpolation-filters="sRGB">
      <feGaussianBlur stdDeviation="0.9"/>
    </filter>
    <filter id="liquidSurface" x="-12%" y="-12%" width="124%" height="124%" color-interpolation-filters="sRGB">
      <feGaussianBlur in="SourceGraphic" stdDeviation="0.72" result="softBase"/>
      <feGaussianBlur in="SourceAlpha" stdDeviation="3.2" result="softAlpha"/>
      <feSpecularLighting in="softAlpha" surfaceScale="6" specularConstant="0.055" specularExponent="18" lighting-color="#FFFFFF" result="specular">
        <feDistantLight azimuth="225" elevation="58"/>
      </feSpecularLighting>
      <feComposite in="specular" in2="SourceAlpha" operator="in" result="clippedSpecular"/>
      <feBlend in="softBase" in2="clippedSpecular" mode="screen"/>
    </filter>
  </defs>`;

const glassGlyph = () =>
  [
    `<path d="${GLYPH.d}" fill="#25282B" filter="url(#slashShadow)"/>`,
    `<path d="${GLYPH.d}" fill="none" stroke="url(#outerRim)" stroke-width="6" stroke-linejoin="round" filter="url(#outerRimSoft)"/>`,
    `<path d="${GLYPH.d}" fill="url(#slash)" stroke-linejoin="round" filter="url(#liquidSurface)"/>`,
    `<path d="${GLYPH.d}" fill="url(#convexSheen)" opacity="0.46"/>`,
  ]
    .map((layer) => `    ${layer}`)
    .join("\n");

/**
 * The size at or below which the glyph is drawn as flat ink instead.
 *
 * The glass treatment is built out of gradients and blurs a few pixels wide.
 * Below about two dozen pixels there is no room left for them: the shadow and
 * the rim close over the body, and a black slash reads as a grey smear. Apple
 * simplifies its own icons at these sizes for the same reason. Above the
 * threshold the moulding is what the icon is; at or below it, legibility is.
 */
const FLAT_MAX_SIZE = 32;

/**
 * Apple's icon geometry, expressed against a 1024pt canvas: the plate is 824pt
 * wide with a 185.4pt continuous corner. Verified against the shipped system
 * icon shape on macOS 26 to within 0.1% of the masked area.
 */
const APPLE_GRID = { canvas: 1024, plate: 824, radius: 185.4 };
const CORNER_SMOOTHING = 0.6;
const RADIUS_RATIO = APPLE_GRID.radius / APPLE_GRID.plate;

/**
 * Glyph height as a share of the plate. Small renditions get a larger, and so
 * optically heavier, slash: at 16px the nominal stroke thins out to a grey
 * smudge. The correction fades out logarithmically between the two anchor
 * sizes, so neighbouring renditions never jump in weight.
 */
const GLYPH_RATIO = 0.57;
const GLYPH_RATIO_SMALL = 0.66;
const SMALL_SIZE = 16;
const NOMINAL_SIZE = 64;

function glyphRatioFor(size) {
  if (size <= SMALL_SIZE) return GLYPH_RATIO_SMALL;
  if (size >= NOMINAL_SIZE) return GLYPH_RATIO;
  const t = Math.log2(size / SMALL_SIZE) / Math.log2(NOMINAL_SIZE / SMALL_SIZE);
  return GLYPH_RATIO_SMALL + (GLYPH_RATIO - GLYPH_RATIO_SMALL) * t;
}

const toRadians = (degrees) => (degrees * Math.PI) / 180;

/**
 * Builds a squircle path with Apple-style continuous corners (the same
 * construction Figma exposes as "corner smoothing"): each corner is a shortened
 * arc flanked by two Bézier segments that ease the curvature into the straight
 * edge, instead of the abrupt curvature jump of a plain rounded rectangle.
 */
function squirclePath({ x, y, size, radius, smoothing = CORNER_SMOOTHING }) {
  const reach = (1 + smoothing) * radius;
  const arcMeasure = 90 * (1 - smoothing);
  const arc = Math.sin(toRadians(arcMeasure / 2)) * radius * Math.SQRT2;
  const alpha = (90 - arcMeasure) / 2;
  const beta = 45 * smoothing;
  const c = radius * Math.tan(toRadians(alpha / 2)) * Math.cos(toRadians(beta));
  const d = c * Math.tan(toRadians(beta));
  const b = (reach - arc - c - d) / 3;
  const a = 2 * b;
  const n = (value) => Number(value.toFixed(4));

  return [
    `M ${n(x + size - reach)} ${n(y)}`,
    `c ${n(a)} 0 ${n(a + b)} 0 ${n(a + b + c)} ${n(d)}`,
    `a ${n(radius)} ${n(radius)} 0 0 1 ${n(arc)} ${n(arc)}`,
    `c ${n(d)} ${n(c)} ${n(d)} ${n(b + c)} ${n(d)} ${n(a + b + c)}`,
    `L ${n(x + size)} ${n(y + size - reach)}`,
    `c 0 ${n(a)} 0 ${n(a + b)} ${n(-d)} ${n(a + b + c)}`,
    `a ${n(radius)} ${n(radius)} 0 0 1 ${n(-arc)} ${n(arc)}`,
    `c ${n(-c)} ${n(d)} ${n(-(b + c))} ${n(d)} ${n(-(a + b + c))} ${n(d)}`,
    `L ${n(x + reach)} ${n(y + size)}`,
    `c ${n(-a)} 0 ${n(-(a + b))} 0 ${n(-(a + b + c))} ${n(-d)}`,
    `a ${n(radius)} ${n(radius)} 0 0 1 ${n(-arc)} ${n(-arc)}`,
    `c ${n(-d)} ${n(-c)} ${n(-d)} ${n(-(b + c))} ${n(-d)} ${n(-(a + b + c))}`,
    `L ${n(x)} ${n(y + reach)}`,
    `c 0 ${n(-a)} 0 ${n(-(a + b))} ${n(d)} ${n(-(a + b + c))}`,
    `a ${n(radius)} ${n(radius)} 0 0 1 ${n(arc)} ${n(-arc)}`,
    `c ${n(c)} ${n(-d)} ${n(b + c)} ${n(-d)} ${n(a + b + c)} ${n(-d)}`,
    "Z",
  ].join(" ");
}

/**
 * @param {object} options
 * @param {"apple"|"full-bleed"} options.plate  Icon geometry to draw on.
 * @param {number} [options.canvas]             Output viewBox size.
 * @param {number} [options.glyphRatio]         Glyph height / plate height.
 * @param {boolean} [options.glass]             Draw the glass rendition, not flat ink.
 */
function iconSvg({ plate, canvas = APPLE_GRID.canvas, glyphRatio = GLYPH_RATIO, glass = true }) {
  const plateSize = plate === "apple" ? (canvas * APPLE_GRID.plate) / APPLE_GRID.canvas : canvas;
  const inset = (canvas - plateSize) / 2;
  const path = squirclePath({
    x: inset,
    y: inset,
    size: plateSize,
    radius: plateSize * RADIUS_RATIO,
  });

  const glyphHeight = GLYPH.y1 - GLYPH.y0;
  const glyphWidth = GLYPH.x1 - GLYPH.x0;
  const scale = (plateSize * glyphRatio) / glyphHeight;
  const tx = canvas / 2 - (GLYPH.x0 + glyphWidth / 2) * scale;
  const ty = canvas / 2 - (GLYPH.y0 + glyphHeight / 2) * scale;

  return `<svg xmlns="http://www.w3.org/2000/svg" width="${canvas}" height="${canvas}" viewBox="0 0 ${canvas} ${canvas}">
${glass ? `${GLASS_DEFS}\n` : ""}  <path d="${path}" fill="${PLATE}"/>
  <g transform="translate(${tx.toFixed(3)} ${ty.toFixed(3)}) scale(${scale.toFixed(6)})">
${glass ? glassGlyph() : `    <path d="${GLYPH.d}" fill="${INK}"/>`}
  </g>
</svg>
`;
}

/**
 * The menu bar wants a stencil, not an icon.
 *
 * macOS draws a template image by its alpha channel alone and throws the colour
 * away, recolouring it for a light or dark bar. The application icon cannot be
 * used that way: its plate is opaque edge to edge, so the silhouette of it is a
 * filled square — which is exactly what the tray showed. This draws the glyph
 * by itself on nothing, which is what the alpha channel has to hold.
 *
 * The glyph is heavier here than on a plate. At bar size the nominal stroke
 * thins to a smudge, and unlike an application icon there is no plate behind it
 * to carry the weight.
 */
const TRAY_GLYPH_RATIO = 0.72;

function trayTemplateSvg(canvas) {
  const glyphHeight = GLYPH.y1 - GLYPH.y0;
  const glyphWidth = GLYPH.x1 - GLYPH.x0;
  const scale = (canvas * TRAY_GLYPH_RATIO) / glyphHeight;
  const tx = canvas / 2 - (GLYPH.x0 + glyphWidth / 2) * scale;
  const ty = canvas / 2 - (GLYPH.y0 + glyphHeight / 2) * scale;

  return `<svg xmlns="http://www.w3.org/2000/svg" width="${canvas}" height="${canvas}" viewBox="0 0 ${canvas} ${canvas}">
  <g transform="translate(${tx.toFixed(3)} ${ty.toFixed(3)}) scale(${scale.toFixed(6)})">
    <path d="${GLYPH.d}" fill="#000000"/>
  </g>
</svg>
`;
}

/** Renders one icon at `size`, picking the rendition and glyph weight that size can carry. */
function render(plate, size) {
  const svg = iconSvg({ plate, glyphRatio: glyphRatioFor(size), glass: size > FLAT_MAX_SIZE });
  return sharp(Buffer.from(svg), { density: 512 })
    .resize(size, size, { kernel: "lanczos3" })
    .png({ compressionLevel: 9 })
    .toBuffer();
}

/**
 * Packs PNG frames into an `.ico`. Windows Vista and later read PNG-compressed
 * frames directly, so the frames go in untouched rather than as BMP+mask.
 */
function buildIco(frames) {
  const HEADER = 6;
  const ENTRY = 16;
  const header = Buffer.alloc(HEADER);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // type: icon
  header.writeUInt16LE(frames.length, 4);

  let offset = HEADER + ENTRY * frames.length;
  const entries = frames.map(({ size, png }) => {
    const entry = Buffer.alloc(ENTRY);
    entry.writeUInt8(size >= 256 ? 0 : size, 0); // 0 means 256
    entry.writeUInt8(size >= 256 ? 0 : size, 1);
    entry.writeUInt8(0, 2); // palette colours
    entry.writeUInt8(0, 3); // reserved
    entry.writeUInt16LE(1, 4); // colour planes
    entry.writeUInt16LE(32, 6); // bits per pixel
    entry.writeUInt32LE(png.length, 8);
    entry.writeUInt32LE(offset, 12);
    offset += png.length;
    return entry;
  });

  return Buffer.concat([header, ...entries, ...frames.map((frame) => frame.png)]);
}

/** macOS `.icns` renditions, keyed by the filename `iconutil` expects. */
const ICNS_RENDITIONS = [
  ["icon_16x16.png", 16],
  ["icon_16x16@2x.png", 32],
  ["icon_32x32.png", 32],
  ["icon_32x32@2x.png", 64],
  ["icon_128x128.png", 128],
  ["icon_128x128@2x.png", 256],
  ["icon_256x256.png", 256],
  ["icon_256x256@2x.png", 512],
  ["icon_512x512.png", 512],
  ["icon_512x512@2x.png", 1024],
];

const PNG_RENDITIONS = [
  ["32x32.png", 32],
  ["128x128.png", 128],
  ["128x128@2x.png", 256],
  ["icon.png", 1024],
  ["Square30x30Logo.png", 30],
  ["Square44x44Logo.png", 44],
  ["Square71x71Logo.png", 71],
  ["Square89x89Logo.png", 89],
  ["Square107x107Logo.png", 107],
  ["Square142x142Logo.png", 142],
  ["Square150x150Logo.png", 150],
  ["Square284x284Logo.png", 284],
  ["Square310x310Logo.png", 310],
  ["StoreLogo.png", 50],
];

const ICO_SIZES = [16, 24, 32, 48, 64, 128, 256];

async function main() {
  mkdirSync(ICONS_DIR, { recursive: true });

  // Vector masters, kept next to the glyph so the shapes can be inspected and
  // handed to designers without re-running this script.
  const brandDir = join(ROOT, "assets", "brand");
  writeFileSync(join(brandDir, "sync-icon-macos.svg"), iconSvg({ plate: "apple" }));
  writeFileSync(join(brandDir, "sync-icon.svg"), iconSvg({ plate: "full-bleed" }));

  for (const [name, size] of PNG_RENDITIONS) {
    writeFileSync(join(ICONS_DIR, name), await render("full-bleed", size));
  }

  const frames = [];
  for (const size of ICO_SIZES) {
    frames.push({ size, png: await render("full-bleed", size) });
  }
  writeFileSync(join(ICONS_DIR, "icon.ico"), buildIco(frames));

  if (process.platform === "darwin") {
    const iconset = join(ICONS_DIR, "icon.iconset");
    rmSync(iconset, { recursive: true, force: true });
    mkdirSync(iconset, { recursive: true });
    for (const [name, size] of ICNS_RENDITIONS) {
      writeFileSync(join(iconset, name), await render("apple", size));
    }
    execFileSync("iconutil", ["-c", "icns", iconset, "-o", join(ICONS_DIR, "icon.icns")]);
    rmSync(iconset, { recursive: true, force: true });
  } else {
    console.warn("skipping icon.icns: iconutil is only available on macOS");
  }

  // Browser tab icon for `next dev`; Next.js picks up `app/icon.svg` by convention.
  // The menu bar item, at both densities. 22pt is the height of the bar's
  // content; the doubled rendition is what a Retina display draws.
  for (const [name, size] of [
    ["trayTemplate.png", 22],
    ["trayTemplate@2x.png", 44],
  ]) {
    writeFileSync(
      join(ICONS_DIR, name),
      await sharp(Buffer.from(trayTemplateSvg(size)), { density: 512 })
        .resize(size, size, { kernel: "lanczos3" })
        .png({ compressionLevel: 9 })
        .toBuffer(),
    );
  }

  writeFileSync(join(APP_DIR, "icon.svg"), iconSvg({ plate: "full-bleed", canvas: 512 }));

  console.log(`wrote ${PNG_RENDITIONS.length} PNGs, icon.ico, icon.icns and app/icon.svg`);
}

await main();
