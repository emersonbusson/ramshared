const body = `## Commits
`;
const pattern = /^##\s+Commits\s*$/im;
console.log(pattern.test(body));

const body2 = `## Commits table
`;
console.log(pattern.test(body2));
