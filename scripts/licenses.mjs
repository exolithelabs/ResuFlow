#!/usr/bin/env node
// Audit the licenses of everything ResuFlow ships and write THIRD_PARTY_LICENSES.md.
//
//   node scripts/licenses.mjs          regenerate THIRD_PARTY_LICENSES.md
//   node scripts/licenses.mjs --check  fail if it is out of date or a license is not allowed
//
// "Ships" means: Rust crates linked into the release binary for the platforms we build (normal
// dependencies only; build scripts and dev tools are not distributed), and the npm packages bundled
// into the interface. Each package is attributed with the license files it ships; one without any
// gets the standard text of the license it is used under. Identical texts are printed once.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = join(ROOT, "THIRD_PARTY_LICENSES.md");
const TARGETS = {
  "x86_64-unknown-linux-gnu": "Linux x86_64",
  "x86_64-pc-windows-msvc": "Windows x86_64",
  "aarch64-apple-darwin": "macOS Apple silicon",
  "x86_64-apple-darwin": "macOS Intel",
};
/** Packages bundled into the interface that are not runtime dependencies. */
const BUNDLED_TOOLS = { vite: "its module-preload helper is inlined into the bundle" };

// Licenses we accept, in the order we prefer to use a package under when it offers a choice.
// All are permissive except MPL-2.0, which is file-level copyleft: fine for unmodified crates,
// whose source stays available from the linked repository.
const ALLOWED = [
  "MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-3-Clause", "BSD-2-Clause", "ISC", "Zlib",
  "0BSD", "MIT-0", "Unicode-3.0", "Unicode-DFS-2016", "BSL-1.0", "CC0-1.0", "Unlicense", "CDLA-Permissive-2.0",
  "MPL-2.0",
];

// ---- license expressions ---------------------------------------------------------

/** Parse an SPDX expression (also the old `MIT/Apache-2.0` form) into a tree. */
function parse(expr) {
  const tokens = expr.replace(/\//g, " OR ").replace(/[()]/g, " $& ").trim().split(/\s+/);
  let i = 0;
  const atom = () => {
    if (tokens[i] === "(") {
      i++;
      const e = or();
      i++; // ")"
      return e;
    }
    let id = tokens[i++];
    if (tokens[i] === "WITH") id += ` WITH ${tokens[(i += 2) - 1]}`;
    return { id };
  };
  const and = () => {
    const all = [atom()];
    while (tokens[i] === "AND") i++, all.push(atom());
    return all.length === 1 ? all[0] : { and: all };
  };
  const or = () => {
    const any = [and()];
    while (tokens[i] === "OR") i++, any.push(and());
    return any.length === 1 ? any[0] : { or: any };
  };
  return or();
}

/** The licenses we use a package under: the most preferred allowed choice, or null if none is allowed. */
function elect(node) {
  if (node.id) return ALLOWED.includes(node.id) ? [node.id] : null;
  if (node.and) {
    const parts = node.and.map(elect);
    return parts.every(Boolean) ? [...new Set(parts.flat())] : null;
  }
  const options = node.or.map(elect).filter(Boolean);
  const rank = (ids) => Math.max(...ids.map((id) => ALLOWED.indexOf(id)));
  return options.sort((a, b) => rank(a) - rank(b) || a.length - b.length)[0] ?? null;
}

// ---- license files ---------------------------------------------------------------

const LICENSE_FILE = /^(licen[cs]e|copying|unlicense|copyright|notice)/i;
const SIGNATURES = [
  ["Apache-2.0", /Apache License[\s\S]{0,40}Version 2\.0/i],
  ["MPL-2.0", /Mozilla Public License,? Version 2\.0/i],
  ["MIT", /Permission is hereby granted, free of charge/i],
  ["BSD-3-Clause", /Neither the name/i],
  ["BSD-2-Clause", /Redistribution and use in source and binary forms/i],
  ["ISC", /Permission to use, copy, modify, and\/?o?r? distribute this software for any/i],
  ["Zlib", /provided ['‘]as-is['’], without any express or implied/i],
  ["Unicode-3.0", /UNICODE LICENSE V3/i],
  ["Unicode-DFS-2016", /UNICODE, INC\. LICENSE AGREEMENT/i],
  ["Unlicense", /This is free and unencumbered software/i],
  ["BSL-1.0", /Boost Software License/i],
  ["CC0-1.0", /CC0 1\.0 Universal/i],
  ["CDLA-Permissive-2.0", /Community Data License Agreement/i],
];
const kindOf = (text) => SIGNATURES.find(([, re]) => re.test(text.replace(/\s+/g, " ")))?.[0] ?? null;
const sameLicense = (kind, id) => kind === id || (kind === "Apache-2.0" && id.startsWith("Apache-2.0")) || (kind === "BSD-2-Clause" && id === "BSD-3-Clause") || (kind === "ISC" && id === "0BSD");

/**
 * The files to attribute a package with: its NOTICE files, plus the license files for the
 * licenses it is used under. A file whose license can't be told from its text is kept.
 */
function licenseFiles(dir, used) {
  if (!existsSync(dir)) return [];
  const files = readdirSync(dir, { withFileTypes: true })
    .filter((e) => e.isFile() && LICENSE_FILE.test(e.name))
    .map((e) => ({ name: e.name, text: readFileSync(join(dir, e.name), "utf8").replace(/\r\n/g, "\n").trim() }))
    .filter((f) => f.text);
  const notices = files.filter((f) => /^notice/i.test(f.name));
  const licenses = files.filter((f) => !/^notice/i.test(f.name)).map((f) => ({ ...f, kind: kindOf(f.text) }));
  const matching = licenses.filter((f) => !f.kind || used.some((id) => sameLicense(f.kind, id)));
  return [...(matching.length ? matching : licenses), ...notices];
}

const MIT_TEXT = (holders) => `MIT License

Copyright (c) ${holders}

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.`;

const BSD3_TEXT = (holders) => `BSD 3-Clause License

Copyright (c) ${holders}

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its
   contributors may be used to endorse or promote products derived from
   this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.`;

/** Standard texts for packages that ship no license file. Copyleft and long texts are taken
 *  verbatim from another package that ships them. */
function standardText(id, pkg, library) {
  const holders = pkg.authors.length ? pkg.authors.join(", ") : `the ${pkg.name} authors`;
  if (id === "MIT") return MIT_TEXT(holders);
  if (id === "BSD-3-Clause") return BSD3_TEXT(holders);
  if (id === "Apache-2.0") return readFileSync(join(ROOT, "LICENSE"), "utf8").trim();
  const found = library.get(id);
  if (!found) throw new Error(`${pkg.name}: ships no license file, and no standard ${id} text is known`);
  return found;
}

// ---- packages ----------------------------------------------------------------------

const repoUrl = (r) => (typeof r === "object" ? r?.url : r)?.replace(/^git\+/, "").replace(/\.git$/, "").replace(/^git:\/\//, "https://") || "";
const stripEmail = (a) => a.replace(/\s*<[^>]*>/g, "").trim();

function rustPackages() {
  const byId = new Map();
  const platforms = new Map();
  for (const [target, label] of Object.entries(TARGETS)) {
    const meta = JSON.parse(
      execFileSync("cargo", ["metadata", "--format-version", "1", "--locked", "--filter-platform", target, "--manifest-path", join(ROOT, "src-tauri/Cargo.toml")], {
        encoding: "utf8",
        maxBuffer: 1 << 28,
        stdio: ["ignore", "pipe", "inherit"],
      }),
    );
    const pkgs = new Map(meta.packages.map((p) => [p.id, p]));
    const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
    const seen = new Set();
    const stack = [meta.resolve.root];
    while (stack.length) {
      const id = stack.pop();
      if (seen.has(id)) continue;
      seen.add(id);
      for (const d of nodes.get(id).deps) if (d.dep_kinds.some((k) => k.kind === null)) stack.push(d.pkg);
    }
    seen.delete(meta.resolve.root);
    for (const id of seen) {
      const p = pkgs.get(id);
      byId.set(id, p);
      platforms.set(id, [...(platforms.get(id) ?? []), label]);
    }
  }
  return [...byId.entries()].map(([id, p]) => ({
    ecosystem: "rust",
    name: p.name,
    version: p.version,
    license: p.license ?? (p.license_file ? "LicenseRef-file" : ""),
    repository: p.repository || `https://crates.io/crates/${p.name}`,
    authors: (p.authors ?? []).map(stripEmail).filter(Boolean),
    dir: dirname(p.manifest_path),
    platforms: platforms.get(id).length === Object.keys(TARGETS).length ? "all" : platforms.get(id).join(", "),
  }));
}

function npmPackages() {
  const lock = JSON.parse(readFileSync(join(ROOT, "package-lock.json"), "utf8"));
  const tools = Object.keys(BUNDLED_TOOLS).map((n) => `node_modules/${n}`);
  return Object.entries(lock.packages)
    .filter(([path, entry]) => path && ((!entry.dev && !entry.optional) || tools.includes(path)))
    .map(([path]) => {
      const dir = join(ROOT, path);
      const pj = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
      const author = typeof pj.author === "string" ? pj.author : pj.author?.name;
      return {
        ecosystem: "npm",
        name: pj.name,
        version: pj.version,
        license: typeof pj.license === "string" ? pj.license : (pj.license?.type ?? ""),
        repository: repoUrl(pj.repository) || `https://www.npmjs.com/package/${pj.name}`,
        authors: [author, ...(pj.contributors ?? []).map((c) => (typeof c === "string" ? c : c.name))].filter(Boolean).map(stripEmail),
        dir,
        note: BUNDLED_TOOLS[pj.name],
      };
    });
}

// ---- report ------------------------------------------------------------------------

const byName = (a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version, undefined, { numeric: true });
const cell = (s) => String(s).replace(/\|/g, "\\|");
const digest = (s) => createHash("sha256").update(s.replace(/\s+/g, " ").trim()).digest("hex");

function build() {
  const packages = [...rustPackages().sort(byName), ...npmPackages().sort(byName)];
  const problems = [];

  // Long standard texts, for packages that ship none, borrowed from packages that do.
  const library = new Map();
  for (const p of packages)
    for (const f of licenseFiles(p.dir, ALLOWED)) if (f.kind && !library.has(f.kind) && !/copyright \(c\)|copyright 20/i.test(f.text.slice(0, 400))) library.set(f.kind, f.text);

  const texts = new Map(); // digest -> { text, users }
  for (const p of packages) {
    p.used = p.license ? elect(parse(p.license)) : null;
    if (!p.used) {
      problems.push(`${p.name} ${p.version}: license "${p.license || "none declared"}" is not on the allow list`);
      continue;
    }
    let files = licenseFiles(p.dir, p.used).map((f) => f.text);
    if (!files.length) files = p.used.map((id) => standardText(id, p, library));
    p.texts = files.map((text) => {
      const key = digest(text);
      if (!texts.has(key)) texts.set(key, { text, users: [] });
      texts.get(key).users.push(`${p.name} ${p.version}`);
      return key;
    });
  }
  // Number texts in order of first use, so the file is stable.
  const number = new Map([...texts.keys()].map((k, i) => [k, i + 1]));

  const counts = new Map();
  for (const p of packages.filter((p) => p.used)) for (const id of p.used) counts.set(id, (counts.get(id) ?? 0) + 1);

  const table = (list) => [
    "| Package | Version | Declared license | Used under | Source | Text |",
    "| --- | --- | --- | --- | --- | --- |",
    ...list.map((p) =>
      `| ${cell(p.name)}${p.note ? ` (${cell(p.note)})` : ""}${p.platforms && p.platforms !== "all" ? ` · ${cell(p.platforms)}` : ""} | ${cell(p.version)} | ${cell(p.license)} | ${cell((p.used ?? ["—"]).join(" AND "))} | ${cell(p.repository)} | ${(p.texts ?? []).map((k) => `[${number.get(k)}](#text-${number.get(k)})`).join(", ")} |`,
    ),
  ];
  const rust = packages.filter((p) => p.ecosystem === "rust");
  const npm = packages.filter((p) => p.ecosystem === "npm");

  const md = [
    "# Third-party licenses",
    "",
    "ResuFlow is licensed under the Apache License, Version 2.0 (see `LICENSE` and `NOTICE`). It is built",
    "from the open-source packages listed here, each under its own license, which are reproduced below.",
    "",
    "This file is generated by `npm run licenses` (`scripts/licenses.mjs`) from `Cargo.lock` and",
    "`package-lock.json`. Do not edit it by hand: CI fails when it is out of date.",
    "",
    `It covers release builds for ${Object.values(TARGETS).join(", ")}. A package used on some platforms`,
    "only says which. Where a package offers a choice of licenses, ResuFlow uses it under the one in",
    "\"Used under\".",
    "",
    "## Summary",
    "",
    "| License | Packages |",
    "| --- | --- |",
    ...[...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([id, n]) => `| ${id} | ${n} |`),
    "",
    "## Supplied by the operating system",
    "",
    "ResuFlow shows its interface in the system's web view and does not include it in its own code:",
    "WebKitGTK on Linux, WKWebView on macOS and Microsoft Edge WebView2 on Windows.",
    "",
    "The Linux AppImage also carries copies of shared libraries from the system it was built on",
    "(Ubuntu 22.04), among them WebKitGTK, GTK, GLib and libsoup, which are licensed under the GNU",
    "LGPL 2.1 or later. They are unmodified and dynamically linked, so you can replace them. Their",
    "source code is available from Ubuntu (https://packages.ubuntu.com/source/jammy/) and from each",
    "project; on request we will provide the exact source for any release.",
    "",
    `## Rust crates (${rust.length})`,
    "",
    ...table(rust),
    "",
    `## JavaScript packages (${npm.length})`,
    "",
    "Bundled into the app's interface.",
    "",
    ...table(npm),
    "",
    "## License texts",
    "",
    ...[...texts.entries()].flatMap(([key, { text, users }]) => [
      `<a id="text-${number.get(key)}"></a>`,
      "",
      `### ${number.get(key)}. ${kindOf(text) ?? "License"}`,
      "",
      `Used by: ${users.join(", ")}`,
      "",
      "````text",
      text,
      "````",
      "",
    ]),
  ].join("\n");
  return { md, problems, packages };
}

const { md, problems, packages } = build();
if (problems.length) {
  console.error(`License audit failed:\n  ${problems.join("\n  ")}\nAllowed: ${ALLOWED.join(", ")}`);
  process.exit(1);
}
if (process.argv.includes("--check")) {
  const current = existsSync(OUT) ? readFileSync(OUT, "utf8") : "";
  if (current !== md) {
    console.error("THIRD_PARTY_LICENSES.md is out of date: run `npm run licenses` and commit the result.");
    process.exit(1);
  }
  console.log(`Licenses OK: ${packages.length} packages, all allowed, notices up to date.`);
} else {
  writeFileSync(OUT, md);
  console.log(`Wrote THIRD_PARTY_LICENSES.md: ${packages.length} packages, ${(md.length / 1024).toFixed(0)} KB.`);
}
