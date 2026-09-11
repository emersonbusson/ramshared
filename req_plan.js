const fs = require('fs');

let lines = fs.readFileSync('plan_for_real.txt', 'utf8').split('\n');
let plan = [];
let buffer = '';
for (let i = 0; i < lines.length; i++) {
    if (lines[i].match(/^\d+\.\s/)) {
        if (buffer) {
            plan.push(buffer);
        }
        buffer = lines[i];
    } else {
        buffer += '\n' + lines[i];
    }
}
if (buffer) {
    plan.push(buffer);
}

let numbered_plan = plan.map((p, i) => p.replace(/^\d+\./, `${i + 1}.`)).join('\n');
// Replace EOF_INTERNAL back to EOF
numbered_plan = numbered_plan.replace(/EOF_INTERNAL/g, 'EOF');

const data = { plan: numbered_plan };
fs.writeFileSync('tool_req19.json', JSON.stringify(data));
