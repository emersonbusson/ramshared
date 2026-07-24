const body = "## Resumo\r\nSome text\r\n## Commits\r\nText";
const pattern = /^##\s+Resumo\s*$/im;
console.log(pattern.test(body));
