// Turns the single-file build (dist-single/player5.html) into a page
// fragment for hosts that wrap content in their own <html>/<head>/<body>
// (e.g. a published Claude artifact): title and styles first, then the
// body content, then the module script. Run after `npm run build:single`.
//
//   npm run build:artifact   ->  dist-single/player5-artifact.html

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const src = join(here, "../dist-single/player5.html");
const out = join(here, "../dist-single/player5-artifact.html");
const html = readFileSync(src, "utf8");

const headStart = html.indexOf("<head>");
const headEnd = html.lastIndexOf("</head>");
const bodyStart = html.indexOf(">", html.indexOf("<body", headEnd)) + 1;
const bodyEnd = html.lastIndexOf("</body>");
if (headStart < 0 || headEnd < 0 || bodyStart <= 0 || bodyEnd < 0) {
  throw new Error("unexpected single-file layout");
}
const head = html.slice(headStart + 6, headEnd);

function blocks(tag) {
  const found = [];
  let i = 0;
  for (;;) {
    const start = head.indexOf(`<${tag}`, i);
    if (start < 0) break;
    const end = head.indexOf(`</${tag}>`, start);
    if (end < 0) throw new Error(`unterminated <${tag}>`);
    found.push(head.slice(start, end + tag.length + 3));
    i = end + tag.length + 3;
  }
  return found;
}

const title = head.match(/<title>[^<]*<\/title>/)?.[0];
if (!title) throw new Error("no <title>");
// The host's skeleton may not set a viewport: without one a phone lays the
// page out 980 px wide and shrinks it. A <meta> in <body> still applies.
const metas = ["viewport", "color-scheme"].map((name) => {
  const tag = head.match(new RegExp(`<meta name="${name}"[^>]*>`))?.[0];
  if (!tag) throw new Error(`no <meta name="${name}">`);
  return tag;
});
const styles = blocks("style");
const scripts = blocks("script");
if (styles.length === 0 || scripts.length === 0) throw new Error("missing style or script");
const body = html.slice(bodyStart, bodyEnd).trim();

const fragment = [title, ...metas, ...styles, body, ...scripts].join("\n");
if (fragment.slice(0, 8192).indexOf("<title>") < 0) throw new Error("title not in the first 8 KB");
writeFileSync(out, fragment);
console.log(`wrote ${out} (${(fragment.length / 1024).toFixed(0)} KB)`);
