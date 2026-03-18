---
paths:
  - "demo/src/**/*.tsx"
  - "demo/src/**/*.ts"
---

# TypeScript Rules

- Functional components only (no class components)
- Run `npx tsc --noEmit` after changes
- Use `interface` not `type` for object shapes
- No `any` — use proper types or `unknown`
- Tailwind CSS for styling, no inline style objects unless dynamic
