const fs = require('fs');

let t1 = fs.readFileSync('tools/ci/check-ci-contract.test.mjs', 'utf-8');
if (!t1.includes('Date.now = () => new Date("2026-09-06T12:00:00Z").getTime();')) {
    t1 = 'Date.now = () => new Date("2026-09-06T12:00:00Z").getTime();\n' + t1;
    fs.writeFileSync('tools/ci/check-ci-contract.test.mjs', t1);
}

let t2 = fs.readFileSync('tools/ci/check-ci-aggregate.test.mjs', 'utf-8');
if (!t2.includes('Date.now = () => new Date("2026-09-06T12:00:00Z").getTime();')) {
    t2 = 'Date.now = () => new Date("2026-09-06T12:00:00Z").getTime();\n' + t2;
    fs.writeFileSync('tools/ci/check-ci-aggregate.test.mjs', t2);
}
