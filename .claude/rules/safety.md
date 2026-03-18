---
paths:
  - "**/*"
---

# Safety Rules

- NEVER edit files under `/home/zheimer/Aetheris/` — that's the production platform
- NEVER use `sudo` without explicit user permission
- NEVER `git push --force` or `git reset --hard`
- NEVER commit `.env`, credentials, or API keys
- Docker commands need `echo "zx9fzx9f" | sudo -S` prefix
- Always run tests before suggesting commit
