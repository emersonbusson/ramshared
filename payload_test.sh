cat << 'JSON' > pr_body.json
{
  "title": "docs(findings): report safe code modification not possible for virtdisk.c",
  "body": "## Resumo\n\nReport safe code modification not possible for `drivers/block/ramshared/virtdisk.c` because it is a Windows StorPort driver and Linux already implements the logic in `queue.c` and `main.c`.\n\n## Commits\n\n| Commit | O que fez | Por que fez | Detalhes |\n|---|---|---|---|\n| %h | Add FINDING_ONLY.md | Safe modification not possible | Documented findings that virtdisk.c is a Windows file |\n\n## Issue\n\nFixes #1234\n\n## Responsavel/Owner\n\nUpstreamPkg-100\n\n## Labels\n\ntype:kernel, type:upstream, area:core\n\n## Validacao\n\n```bash\ncat docs/jules/findings/FINDING_ONLY.md\ncargo test\n```\n\n## Rollback trigger\n\nIf the findings are incorrect and safe modification was possible."
}
JSON
