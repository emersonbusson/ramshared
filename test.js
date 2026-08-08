const body1 = "## Resumo\nRemoves...";
const body2 = `## Resumo
Removes...`;
const body3 = "## Resumo\\nRemoves...";

const pattern = /^##\s+Resumo\s*$/im;
console.log(pattern.test(body1));
console.log(pattern.test(body2));
console.log(pattern.test(body3));
