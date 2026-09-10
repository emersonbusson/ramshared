import test from 'node:test';
import assert from 'node:assert';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { writeFileSync, unlinkSync } from 'node:fs';
import { runChecks } from './check-pkgbuild-namcap.mjs';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const root = join(__dirname, '../..');

test('check-pkgbuild-namcap.mjs: success on syntax check when namcap is absent', () => {
    const validTarget = join(root, 'packaging/arch/PKGBUILD');
    const originalEnv = { ...process.env };
    process.env.MOCK_HAS_NAMCAP = '0';
    process.env.MOCK_BASH_CMD = 'bash';

    try {
        const result = runChecks(validTarget);
        assert.strictEqual(result, true);
    } finally {
        process.env = originalEnv;
    }
});

test('check-pkgbuild-namcap.mjs: error on syntax check failure when namcap is absent', () => {
    const tmpTarget = join(root, 'PKGBUILD.tmp.test');
    writeFileSync(tmpTarget, 'pkgname=ramshared\nif [ $pkgname == "ramshared" ]; then\n  echo "missing fi"\n', 'utf8');

    const originalEnv = { ...process.env };
    process.env.MOCK_HAS_NAMCAP = '0';
    process.env.MOCK_BASH_CMD = 'bash';

    try {
        const result = runChecks(tmpTarget);
        assert.strictEqual(result, false);
    } finally {
        process.env = originalEnv;
        try { unlinkSync(tmpTarget); } catch (e) {}
    }
});

test('check-pkgbuild-namcap.mjs: error when target does not exist', () => {
    const tmpTarget = join(root, 'nonexistent-PKGBUILD');

    const originalEnv = { ...process.env };
    process.env.MOCK_HAS_NAMCAP = '0';

    try {
        const result = runChecks(tmpTarget);
        assert.strictEqual(result, false);
    } finally {
        process.env = originalEnv;
    }
});

test('check-pkgbuild-namcap.mjs: success when namcap is present and passes', () => {
    const validTarget = join(root, 'packaging/arch/PKGBUILD');

    const originalEnv = { ...process.env };
    process.env.MOCK_HAS_NAMCAP = '1';
    process.env.MOCK_NAMCAP_CMD = 'true';

    try {
        const result = runChecks(validTarget);
        assert.strictEqual(result, true);
    } finally {
        process.env = originalEnv;
    }
});

test('check-pkgbuild-namcap.mjs: failure when namcap is present and fails', () => {
    const tmpTarget = join(root, 'PKGBUILD.tmp.fail');
    writeFileSync(tmpTarget, 'bad stuff', 'utf8');

    const originalEnv = { ...process.env };
    process.env.MOCK_HAS_NAMCAP = '1';
    process.env.MOCK_NAMCAP_CMD = 'false';

    try {
        const result = runChecks(tmpTarget);
        assert.strictEqual(result, false);
    } finally {
        process.env = originalEnv;
        try { unlinkSync(tmpTarget); } catch (e) {}
    }
});
