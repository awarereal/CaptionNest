// Renders brand SVGs to PNG (used to generate app icons with `tauri icon`).
// Usage: node scripts/brand/render.mjs <in.svg> <out.png> [width]
import { readFileSync, writeFileSync } from "node:fs";
import { Resvg } from "@resvg/resvg-js";

const [input, output, width = "1024"] = process.argv.slice(2);
const png = new Resvg(readFileSync(input), {
  fitTo: { mode: "width", value: Number(width) },
  font: { loadSystemFonts: false },
}).render().asPng();
writeFileSync(output, png);
console.log(`wrote ${output}`);
