# Agent Governance & Protocols

## Befehle & Workflow-Steuerung

- `/init [Idee/Feature]`:
  1. Starte Phase 0: Lies `.agent-system/rules/00_research_feasibility.md`.
  2. Führe die Recherche durch und erstelle `.agent-system/research/feasibility_analysis.md`.
  3. Besprich die Kernbefunde kurz mit dem Entwickler.

- `/plan`:
  1. Starte Phase 1: Lies `.agent-system/rules/01_orchestrator.md`.
  2. Erstelle `specs/spec_<feature>.md` und `tasks/backlog.md`.
  3. Starte direkt Phase 2: Lies `.agent-system/rules/02_preflight_check.md`.
  4. Führe die Detailrecherche durch und schreibe `.agent-system/research/preflight_risk_report.md`.
  5. Stelle alle offenen Detail- und Risiko-Fragen an den Entwickler.
  6. STOPPE. Warte auf die Antworten und die Freigabe durch "PLAN_APPROVED".

- `/work [task_id]`:
  Lies `.agent-system/rules/03_worker.md`. Umsetzung nur bei vorhandenem "PLAN_APPROVED".

- `/qa [task_id]`:
  Lies `.agent-system/rules/04_qa.md`. Verifiziere die Umsetzung und erstelle den QA-Report.