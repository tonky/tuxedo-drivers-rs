Every project gets two files: AGENTS.md and WORKLOG.md.

AGENTS.md is project specific instructions and reference — how to build, deploy, what patterns to follow, where things live.
It rarely changes.

WORKLOG.md is the session diary.
Every time we work on a project, agents log what we investigated, what changed, what we decided, and why.
When I come back days or weeks later, agents read the worklog and pick up where we left off instead of starting cold.

Progress automonously through planned phases.
If some of the phases or order within the phase is unclear - investigate and clarify beforehand.

After each phase - launch 2 sub-agents to review implemented changes to make sure:
  a) They conform to phase specification and requirements, and nothing was missing
  b) See if anything can be improved, refactored or removed.

At the end of each phase - make sure that tests and linters are passing