---
name: invalid
---
# Intentionally invalid

This skill is missing a `description` in its frontmatter; the `validate`
subcommand and its e2e test rely on that to exercise the failure path.
