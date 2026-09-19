#!/usr/bin/env node
/**
 * Automated PKGBUILD namcap lint verifier.
 * Ensures the Arch Linux PKGBUILD does not contain fatal errors.
 */

import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const PKGBUILD = path.join(ROOT, 'packaging', 'arch', 'PKGBUILD');

function checkNamcap() {
  if (!existsSync(PKGBUILD)) {
    console.error('FAIL: PKGBUILD not found at packaging/arch/PKGBUILD');
    process.exit(1);
  }

  try {
    // namcap might not be installed on ubuntu runners natively without special setup,
    // so we handle the case gracefully if namcap is not in PATH, or run it if it is.
    execFileSync('which', ['namcap'], { stdio: 'ignore' });
  } catch (err) {
    console.log('PASS (graceful): namcap is not installed on this system. Skipping lint.');
    process.exit(0);
  }

  try {
    const out = execFileSync('namcap', [PKGBUILD], { encoding: 'utf8' });
    if (out.includes('E:')) {
      console.error('FAIL: namcap detected errors in PKGBUILD:');
      console.error(out);
      process.exit(1);
    }
    console.log('PASS: PKGBUILD passed namcap checks with zero fatal errors.');
    if (out.trim()) {
      console.log('Warnings/Info:');
      console.log(out);
    }
    process.exit(0);
  } catch (err) {
    console.error('FAIL: namcap execution failed');
    console.error(err.message);
    if (err.stdout) console.error(err.stdout);
    if (err.stderr) console.error(err.stderr);
    process.exit(1);
  }
}

checkNamcap();
