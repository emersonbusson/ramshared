import { existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const root = join(__dirname, '../..');
const defaultTarget = join(root, 'packaging/arch/PKGBUILD');
const target = process.env.PKGBUILD_TARGET || defaultTarget;

/* node:coverage disable */
if (typeof process.env.NODE_TEST_CONTEXT !== 'undefined' || process.argv.includes('--test')) {
    // Prevent main execution during unit tests
}
/* node:coverage enable */

export function runChecks(targetPath) {
    if (!existsSync(targetPath)) {
        console.error(`ERROR: ${targetPath} not found.`);
        return false;
    }

    console.log(`==> Verifying ${targetPath}...`);
    // Allow overriding command for tests
    const namcapCmd = process.env.MOCK_NAMCAP_CMD || 'namcap';
    const bashCmd = process.env.MOCK_BASH_CMD || 'bash';

    // Simple check to see if we should try namcap
    const hasNamcap = process.env.MOCK_HAS_NAMCAP === '1' ||
                     (process.env.MOCK_HAS_NAMCAP !== '0' && spawnSync('command', ['-v', namcapCmd], { shell: true }).status === 0);

    if (hasNamcap) {
        console.log(`==> Running namcap lint...`);
        const namcap = spawnSync(namcapCmd, [targetPath], { stdio: 'inherit', shell: true });
        if (namcap.status !== 0) {
            console.error(`ERROR: namcap failed on ${targetPath}`);
            return false;
        } else {
            console.log(`✓ namcap found zero fatal errors.`);
            return true;
        }
    } else {
        console.log(`==> namcap unavailable, performing baseline syntax check (bash -n)...`);
        const bash = spawnSync(bashCmd, ['-n', targetPath], { stdio: 'inherit', shell: true });
        if (bash.status !== 0) {
            console.error(`ERROR: Baseline syntax check failed on ${targetPath}`);
            return false;
        } else {
            console.log(`✓ Baseline syntax check passed.`);
            return true;
        }
    }
}

// Ensure the CLI execution matches the new logic
/* node:coverage disable */
if (import.meta.url === `file://${process.argv[1]}` && !process.argv.includes('--test') && typeof process.env.NODE_TEST_CONTEXT === 'undefined') {
    if (!runChecks(target)) {
        process.exitCode = 1;
    }
}
/* node:coverage enable */
