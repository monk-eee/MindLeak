// Copies the vendored Cytoscape bundle into media/ so the webview loads it
// locally (offline-first, no CDN). Runs as part of `npm run compile`.
const fs = require("fs");
const path = require("path");

const src = path.join(__dirname, "..", "node_modules", "cytoscape", "dist", "cytoscape.min.js");
const destDir = path.join(__dirname, "..", "media", "vendor");
const dest = path.join(destDir, "cytoscape.min.js");

if (!fs.existsSync(src)) {
  console.error(
    "cytoscape not found in node_modules — run `npm install` before compiling."
  );
  process.exit(1);
}

fs.mkdirSync(destDir, { recursive: true });
fs.copyFileSync(src, dest);
console.log("Vendored cytoscape.min.js -> media/vendor/cytoscape.min.js");
