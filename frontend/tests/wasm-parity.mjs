import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const frontendRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repositoryRoot = path.resolve(frontendRoot, "..");
const wasmModule = await import(path.join(frontendRoot, "public/oxdraw_wasm.js"));

await wasmModule.default({
  module_or_path: fs.readFileSync(path.join(frontendRoot, "public/oxdraw_wasm_bg.wasm")),
});

const inputRoot = path.join(repositoryRoot, "tests/input");
const expectedRoot = path.join(repositoryRoot, "tests/expected");
const fixtures = fs.readdirSync(inputRoot).filter((name) => name.endsWith(".mmd")).sort();
const mismatches = [];

for (const fixture of fixtures) {
  const source = fs.readFileSync(path.join(inputRoot, fixture), "utf8");
  const expected = fs.readFileSync(
    path.join(expectedRoot, fixture.replace(/\.mmd$/, ".svg")),
    "utf8"
  );
  const actual = new wasmModule.WasmEditorCore(source, "white").renderSvg();

  if (actual !== expected) {
    mismatches.push(fixture);
  }
}

if (mismatches.length > 0) {
  throw new Error(`Browser WASM differs from native goldens: ${mismatches.join(", ")}`);
}

console.log(`${fixtures.length} browser fixtures match native goldens`);
