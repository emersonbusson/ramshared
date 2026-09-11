const { execSync } = require('child_process');
const output = execSync('git log --format="%h" main..HEAD').toString().trim();
const commits = output.split('\n').filter(Boolean);
const tableRows = commits.map(c => `| ${c} | commit msg |`).join('\n');
console.log(tableRows);
