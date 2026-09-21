// SPDX-License-Identifier: MIT
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, copyFileSync, chmodSync, rmSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const source = fileURLToPath(new URL('../../scripts/package/build-rpm-package.sh', import.meta.url));

function fixture({ binaries = true, rpmbuild = false } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'ramshared-rpm-test-'));
  const script = join(root, 'scripts/package/build-rpm-package.sh');
  const binDir = join(root, 'bin');
  mkdirSync(join(root, 'scripts/package'), { recursive: true });
  mkdirSync(join(root, 'target/release'), { recursive: true });
  mkdirSync(binDir);
  for (const command of ['dirname', 'sed', 'mkdir', 'rm', 'cat', 'cp']) {
    symlinkSync(join('/usr/bin', command), join(binDir, command));
  }
  copyFileSync(source, script);
  chmodSync(script, 0o755);
  if (binaries) {
    for (const name of ['ramshared', 'ramsharedd']) {
      const binary = join(root, 'target/release', name);
      writeFileSync(binary, '#!/bin/sh\nexit 0\n');
      chmodSync(binary, 0o755);
    }
  }
  if (rpmbuild) {
    const stub = join(binDir, 'rpmbuild');
    writeFileSync(stub, '#!/bin/sh\nexit 0\n');
    chmodSync(stub, 0o755);
  }
  const run = () => spawnSync('/usr/bin/bash', [script, 'v0.14.1'], {
    cwd: root,
    encoding: 'utf8',
    env: { PATH: binDir, RAMSHARED_PACKAGE_VERSION: 'v0.14.1' },
  });
  return { root, run };
}

test('RPM packaging refuses to build without prebuilt release binaries', () => {
  const { root, run } = fixture({ binaries: false, rpmbuild: true });
  try {
    const result = run();
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /release binaries|Target release binaries/i);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('RPM packaging refuses a spec-only result when rpmbuild is absent', () => {
  const { root, run } = fixture();
  try {
    const result = run();
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /rpmbuild/i);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('RPM packaging refuses a successful rpmbuild with no RPM artifact', () => {
  const { root, run } = fixture({ rpmbuild: true });
  try {
    const result = run();
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /RPM artifact/i);
    const spec = readFileSync(join(root, 'artifacts/packages/rpmbuild/SPECS/ramshared.spec'), 'utf8');
    assert.doesNotMatch(spec, /zero-copy direct PCIe DMA/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
