#!/usr/bin/env node
/**
 * Static checker to enforce English-only comments in code files (AGENTS.md language policy).
 *
 * It checks comment lines for Portuguese stopwords. If at least 2 distinct Portuguese
 * stopwords are found on the same comment line, it flags it as a violation to prevent
 * Portuguese comments from being introduced.
 *
 * Modes:
 *   --diff [base]   Validates only added lines in diff vs base branch (default base: origin/main).
 *   --all           Validates all files (progress report).
 */
import { execFileSync } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const TARGET_EXTENSIONS = new Set([".rs", ".c", ".h", ".ps1", ".sh", ".mjs"]);

const DIRECTIVE_RE =
  /^(\/\/(go:|line |export |nolint|\s*eslint|\s*@ts-|\s*prettier-ignore|\s*biome-ignore|\s*#)|\/\*\s*eslint|--\s*\+goose|#!|#\s*syntax=)/;

// Portuguese stopwords that do not collides with common English identifiers
const PT_MARKERS = [
  "que", "para", "com", "uma", "mais", "sobre", "como", "esta", "entao",
  "tudo", "certo", "funcionando", "foi", "pelo", "pela", "nas", "nos",
  "dos", "das", "seu", "sua", "seus", "suas", "sem", "isso", "nao", "sao",
  "uma", "mais", "sobre", "como", "esta", "sao", "mais", "este", "esta",
  "estes", "estas", "aquele", "aquela", "aqueles", "aquelas", "isso", "isto",
  "aquilo", "porque", "porquê", "por", "para", "como", "onde", "quando",
  "quem", "qual", "quais", "quanto", "quantos", "quanta", "quantas", "mas",
  "porem", "todavia", "contudo", "entretanto", "enquanto", "depois", "antes",
  "desde", "ate", "contra", "entre", "perante", "atras", "depois", "sobre",
  "sob", "após", "durante", "exceto", "salvo", "conforme", "segundo",
  "mediante", "visto", "devido", "graças", "devido", "sendo", "tendo"
];

const LETTER = "A-Za-z_\\u00C0-\\u00D6\\u00D8-\\u00F6\\u00F8-\\u00FF";
const PT_MARKER_RE = new RegExp(
  `(?<![${LETTER}.\\-])(${PT_MARKERS.join("|")})(?![${LETTER}\\-])`,
  "gi",
);

function isCommentLine(line, ext) {
  const trimmed = line.trim();
  if (!trimmed) return false;

  // Skip compiler/linter directives
  if (DIRECTIVE_RE.test(trimmed)) return false;

  // PowerShell
  if (ext === ".ps1") {
    return trimmed.startsWith("#") || trimmed.startsWith("<#") || trimmed.startsWith(".#");
  }

  // Shell scripts
  if (ext === ".sh") {
    return trimmed.startsWith("#");
  }

  // Rust / C / C++ / JS
  if (ext === ".rs" || ext === ".c" || ext === ".h" || ext === ".mjs") {
    return trimmed.startsWith("//") || trimmed.startsWith("/*") || trimmed.startsWith("*");
  }

  return false;
}

function cleanCommentText(line, ext) {
  let cleaned = line.trim();
  if (ext === ".ps1") {
    cleaned = cleaned.replace(/^<#|^#|#>$|\.#/g, "");
  } else if (ext === ".sh") {
    cleaned = cleaned.replace(/^#/g, "");
  } else if (ext === ".rs" || ext === ".c" || ext === ".h" || ext === ".mjs") {
    cleaned = cleaned.replace(/^\/\/|^\/\*|^\*|\*\/$/g, "");
  }
  return cleaned.trim();
}

function getPtMarkers(text) {
  const matches = text.match(PT_MARKER_RE) || [];
  return new Set(matches.map(m => m.toLowerCase()));
}

function checkFile(filePath) {
  const ext = extname(filePath);
  if (!TARGET_EXTENSIONS.has(ext)) return [];

  const content = readFileSync(filePath, "utf8");
  const lines = content.split("\n");
  const violations = [];

  for (let i = 0; i < lines.length; i++) {
    const rawLine = lines[i];
    if (isCommentLine(rawLine, ext)) {
      const cleaned = cleanCommentText(rawLine, ext);
      const markers = getPtMarkers(cleaned);
      if (markers.size >= 2) {
        violations.push({
          line: i + 1,
          content: rawLine.trim(),
          markers: Array.from(markers),
        });
      }
    }
  }

  return violations;
}

function getDiffAddedLines(baseRef) {
  let diff;
  try {
    diff = execFileSync("git", ["diff", "--unified=0", baseRef], {
      cwd: REPO_ROOT,
      encoding: "utf8",
    });
  } catch {
    return {};
  }

  const added = {};
  let currentFile = null;
  let newLineNum = 0;

  for (const line of diff.split("\n")) {
    if (line.startsWith("+++ b/")) {
      currentFile = line.substring(6).trim();
      added[currentFile] = new Set();
      continue;
    }
    if (!currentFile) continue;

    const hunkHeader = line.match(/^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
    if (hunkHeader) {
      newLineNum = parseInt(hunkHeader[1], 10);
      continue;
    }

    if (line.startsWith("+") && !line.startsWith("+++")) {
      added[currentFile].add(newLineNum);
      newLineNum++;
    }
  }

  return added;
}

function main() {
  const argv = process.argv.slice(2);
  const all = argv.includes("--all");
  let baseRef = "origin/main";
  const di = argv.indexOf("--diff");
  if (di !== -1 && argv[di + 1]) {
    baseRef = argv[di + 1];
  }

  const filesToCheck = [];
  let diffAdded = null;

  if (all) {
    // Scan all tracked target files
    const tracked = execFileSync("git", ["ls-files"], {
      cwd: REPO_ROOT,
      encoding: "utf8",
    });
    for (const f of tracked.split("\n")) {
      const ext = extname(f);
      if (TARGET_EXTENSIONS.has(ext)) {
        filesToCheck.push(f);
      }
    }
  } else {
    diffAdded = getDiffAddedLines(baseRef);
    filesToCheck.push(...Object.keys(diffAdded));
  }

  let totalViolations = 0;

  for (const relPath of filesToCheck) {
    const absPath = join(REPO_ROOT, relPath);
    if (!existsSync(absPath)) continue;

    const fileViolations = checkFile(absPath);
    const filtered = fileViolations.filter(v => {
      if (all) return true;
      return diffAdded[relPath]?.has(v.line);
    });

    for (const v of filtered) {
      console.log(
        `${relPath}:${v.line} — pt-comment-detected: line has Portuguese stopwords [${v.markers.join(", ")}]`
      );
      totalViolations++;
    }
  }

  if (totalViolations > 0) {
    console.error(`\nFound ${totalViolations} line(s) with Portuguese comments. All comments must be in English.`);
    process.exit(1);
  }

  console.log("✓ Code comments language check passed.");
  process.exit(0);
}

main();
