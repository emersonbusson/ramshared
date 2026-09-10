import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { rmSync, mkdirSync, writeFileSync, chmodSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = join(fileURLToPath(import.meta.url), '..');
const ROOT = join(__dirname, '..', '..');
const SCRIPT_PATH = join(ROOT, 'scripts', 'package', 'build-rpm-package.sh');
const TEST_BIN_DIR = join(ROOT, 'tmp-rpm-test-bin');

test('build-rpm-package.sh signs packages if RPM_SIGN_KEY_NAME is set', () => {
    // Setup mock bin dir
    rmSync(TEST_BIN_DIR, { recursive: true, force: true });
    mkdirSync(TEST_BIN_DIR, { recursive: true });

    // Create mock rpmbuild
    const mockRpmbuildPath = join(TEST_BIN_DIR, 'rpmbuild');
    writeFileSync(mockRpmbuildPath, '#!/usr/bin/env bash\necho "mock rpmbuild executed" > rpmbuild.log\n');
    chmodSync(mockRpmbuildPath, 0o755);

    // Create mock rpm
    const mockRpmPath = join(TEST_BIN_DIR, 'rpm');
    writeFileSync(mockRpmPath, '#!/usr/bin/env bash\necho "mock rpm executed with args: $@" > rpm_args.log\n');
    chmodSync(mockRpmPath, 0o755);

    const env = {
        ...process.env,
        PATH: `${TEST_BIN_DIR}:${process.env.PATH}`,
        RPM_SIGN_KEY_NAME: 'test-key-id'
    };

    // Ensure we have dummy targets so script does not exit early
    mkdirSync(join(ROOT, 'target', 'release'), { recursive: true });
    writeFileSync(join(ROOT, 'target', 'release', 'ramshared'), '#!/bin/sh\n');
    chmodSync(join(ROOT, 'target', 'release', 'ramshared'), 0o755);
    writeFileSync(join(ROOT, 'target', 'release', 'ramsharedd'), '#!/bin/sh\n');
    chmodSync(join(ROOT, 'target', 'release', 'ramsharedd'), 0o755);

    const proc = spawnSync(SCRIPT_PATH, [], { env, encoding: 'utf8' });

    // Cleanup
    rmSync(TEST_BIN_DIR, { recursive: true, force: true });

    assert.ok(proc.stdout.includes('Signing RPM packages with key: test-key-id'), 'Expected stdout to contain signing message');
    assert.ok(proc.stdout.includes('RPM packages signed.'), 'Expected stdout to contain success message');
});
