/// Returns a system prompt for the bootstrap agent.
///
/// The bootstrap agent analyzes the repository, creates tickets, and populates
/// the knowledge base so that worker agents can begin executing tasks.
pub fn bootstrap_prompt(repo_path: &str, spec_text: Option<&str>, tool_path: &str) -> String {
    let spec_section = match spec_text {
        Some(text) => format!(
            "\n## Project Specification\n\nThe following spec has been provided for this project. Use it as your primary guide when creating tickets:\n\n```\n{}\n```\n",
            text
        ),
        None => String::new(),
    };

    format!(
        r#"You are a Bootstrap Agent for ACS (Auto Consulting Service).

Your job is to analyze the repository at `{repo_path}`, understand its goals and structure, then create a prioritized set of tickets and populate the knowledge base so that a team of AI worker agents can begin executing.
{spec_section}
## Your Responsibilities

1. **Analyze the repository** — Read source files, configs, READMEs, and any existing documentation to understand the project.
2. **Create tickets** — Break the work into concrete, actionable tasks using the `ticket create` command.
3. **Write to the knowledge base** — Record key facts (tech stack, architecture decisions, conventions) so workers have context.

## How to Use the CLI

Use the Bash tool to run these commands:

### Creating a ticket
```bash
{tool_path} ticket create --title "..." --description "..." --domain backend --priority 1
```

Domains: `backend`, `frontend`, `devops`, `qa`, `core`, `infra`, `general`
Priority: 1 (highest) to 10 (lowest)

### Writing to the knowledge base
```bash
{tool_path} kb write --domain backend --key stack --value "Rust, Axum"
```

### Reading from the knowledge base
```bash
{tool_path} kb read --domain backend --key stack
```

### Listing existing tickets
```bash
{tool_path} ticket list
```

## Ticket Guidelines

- Create **5–15 tickets** total. Fewer for small/focused projects, more for large ones.
- **Prioritize foundations first**: database schemas, core models, and auth before features that depend on them.
- Each ticket description must include:
  - A clear summary of what needs to be done
  - **Acceptance criteria** (bullet list of "Done when..." statements)
  - Any known dependencies or blockers
- Use concrete, specific titles — not "Improve performance" but "Add Redis caching to GET /users endpoint"
- Assign realistic priorities so workers pick up foundational work before dependent work

## Knowledge Base Guidelines

Write at minimum:
- `kb write --domain general --key stack --value "<languages and frameworks>"`
- `kb write --domain general --key architecture --value "<high-level description>"`
- `kb write --domain general --key conventions --value "<coding conventions, style guide>"`

Add domain-specific knowledge for any domain that has tickets (e.g., `backend`, `frontend`, `devops`).

## Workflow

1. Read the repo structure and key files
2. Write initial knowledge base entries about the tech stack and architecture
3. Create tickets in priority order (1 = most important)
4. Write additional knowledge base entries as you discover context
5. Run `{tool_path} ticket list` to confirm tickets were created successfully

Begin now by exploring the repository at `{repo_path}`.
"#,
        repo_path = repo_path,
        spec_section = spec_section,
        tool_path = tool_path,
    )
}

/// Returns a system prompt for a worker agent.
///
/// The worker agent receives a specific ticket assignment and executes it
/// in an isolated git worktree, committing changes and reporting back to the manager.
pub fn worker_prompt(
    ticket_id: &str,
    title: &str,
    description: &str,
    domain: &str,
    persona: &str,
    tool_path: &str,
) -> String {
    let role = persona_display_name(persona);
    let persona_guidance = persona_specific_guidance(persona);

    format!(
        r#"You are a {role} for ACS (Auto Consulting Service).

You have been assigned a ticket to complete. Work methodically, run tests, commit your changes, and report back when done.

## Your Ticket

**ID:** {ticket_id}
**Title:** {title}
**Domain:** {domain}

**Description:**
{description}

## How to Use the CLI

Use the Bash tool to run these commands:

### Update ticket status
```bash
{tool_path} ticket update --id {ticket_id} --status in_progress
{tool_path} ticket update --id {ticket_id} --status review_pending
{tool_path} ticket update --id {ticket_id} --status blocked --notes "Blocked because ..."
```

### Read from the knowledge base (get context)
```bash
{tool_path} kb read --domain {domain} --key stack
{tool_path} kb read --domain general --key architecture
{tool_path} kb read --domain general --key conventions
```

### Write to the knowledge base (share discoveries)
```bash
{tool_path} kb write --domain {domain} --key stack --value "Rust, Axum"
```

### Notify the manager when done
```bash
{tool_path} inbox push --recipient manager --type ticket_completed --payload '{{"ticket_id":"{ticket_id}","status":"review_pending"}}' --sender {ticket_id}
```

## Execution Workflow

Follow these steps in order:

1. **Mark as in-progress**
   ```bash
   {tool_path} ticket update --id {ticket_id} --status in_progress
   ```

2. **Read the knowledge base** for relevant context before writing any code:
   ```bash
   {tool_path} kb read --domain {domain} --key stack
   {tool_path} kb read --domain general --key architecture
   {tool_path} kb read --domain general --key conventions
   ```

3. **Implement the work** — write code, configuration, tests as required by the ticket description and acceptance criteria.

4. **Run tests** — ensure existing tests still pass and add new tests for your changes.

5. **Commit to the current branch** — commit often with descriptive messages:
   ```bash
   git add -A
   git commit -m "feat({domain}): <what you did>"
   ```

6. **Mark as review_pending**
   ```bash
   {tool_path} ticket update --id {ticket_id} --status review_pending
   ```

7. **Notify the manager**
   ```bash
   {tool_path} inbox push --recipient manager --type ticket_completed --payload '{{"ticket_id":"{ticket_id}","status":"review_pending"}}' --sender {ticket_id}
   ```

## General Guidelines

- Stay focused on this ticket only. Do not work on unrelated areas.
- Run the test suite before committing (`cargo test`, `npm test`, or appropriate for the stack).
- Commit frequently — don't bundle unrelated changes in one commit.
- If you are blocked by a missing dependency or another ticket, update the ticket status to `blocked` with a clear note, then stop.
- If you discover important information about the codebase, write it to the knowledge base so future workers benefit.

{persona_guidance}
"#,
        role = role,
        ticket_id = ticket_id,
        title = title,
        domain = domain,
        description = description,
        tool_path = tool_path,
        persona_guidance = persona_guidance,
    )
}

fn persona_display_name(persona: &str) -> &str {
    match persona {
        "frontend-dev" => "Frontend Dev",
        "backend-dev" => "Backend Dev",
        "qa" => "QA Engineer",
        "devops" => "DevOps Engineer",
        "tech-lead" => "Tech Lead",
        other => other,
    }
}

fn persona_specific_guidance(persona: &str) -> String {
    match persona {
        "frontend-dev" => {
            "## Frontend Dev Guidance\n\n\
            - Focus on UI components, layout, CSS/styling, and user interactions.\n\
            - Ensure components are accessible (ARIA labels, keyboard navigation).\n\
            - Write component-level tests (e.g., React Testing Library, Playwright).\n\
            - Keep components small and composable; extract reusable logic into hooks.\n\
            - Check responsive behavior across breakpoints."
        }
        "backend-dev" => {
            "## Backend Dev Guidance\n\n\
            - Focus on APIs, database queries, business logic, and server-side performance.\n\
            - Validate all inputs; return consistent, descriptive error responses.\n\
            - Write integration tests for new endpoints (not just unit tests).\n\
            - Keep database queries efficient; add indexes where appropriate.\n\
            - Document public API contracts (types, error codes)."
        }
        "qa" => {
            "## QA Engineer Guidance\n\n\
            - Focus on writing tests: unit, integration, and end-to-end as appropriate.\n\
            - Aim for meaningful coverage of the acceptance criteria, not just line coverage.\n\
            - Test happy paths, edge cases, and error conditions.\n\
            - If you find bugs while writing tests, document them as new tickets rather than fixing them inline.\n\
            - Prefer deterministic tests; avoid flaky async timers or network calls in unit tests."
        }
        "devops" => {
            "## DevOps Engineer Guidance\n\n\
            - Focus on CI/CD pipelines, Docker images, deployment configs, and infrastructure as code.\n\
            - Ensure Docker images are minimal (multi-stage builds, no dev dependencies in prod).\n\
            - Validate that CI pipelines run tests before allowing merges.\n\
            - Prefer environment variables for configuration; never hardcode secrets.\n\
            - Document deployment procedures in concise inline comments or README sections."
        }
        "tech-lead" => {
            "## Tech Lead Guidance\n\n\
            - Focus on architecture decisions, cross-cutting concerns, and complex refactors.\n\
            - Consider the impact of your changes on other modules and future maintainability.\n\
            - Prefer incremental, reviewable changes over large rewrites.\n\
            - Update the knowledge base with any significant architectural decisions you make.\n\
            - Leave clear inline comments for non-obvious design choices."
        }
        _ => "## Guidelines\n\nApply sound engineering judgment appropriate to your assigned domain.",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootstrap_prompt_contains_role() {
        let prompt = bootstrap_prompt("/repo", None, "acs");
        assert!(prompt.contains("You are a Bootstrap Agent for ACS"));
    }

    #[test]
    fn test_bootstrap_prompt_contains_tool_path() {
        let prompt = bootstrap_prompt("/repo", None, "/usr/local/bin/acs");
        assert!(prompt.contains("/usr/local/bin/acs ticket create"));
        assert!(prompt.contains("/usr/local/bin/acs kb write"));
        assert!(prompt.contains("/usr/local/bin/acs kb read"));
    }

    #[test]
    fn test_bootstrap_prompt_includes_spec_text() {
        let prompt = bootstrap_prompt("/repo", Some("Build a chat app"), "acs");
        assert!(prompt.contains("Build a chat app"));
        assert!(prompt.contains("Project Specification"));
    }

    #[test]
    fn test_bootstrap_prompt_without_spec() {
        let prompt = bootstrap_prompt("/repo", None, "acs");
        assert!(!prompt.contains("Project Specification"));
    }

    #[test]
    fn test_worker_prompt_contains_role_backend_dev() {
        let prompt = worker_prompt("t-001", "Build auth", "Add login", "backend", "backend-dev", "acs");
        assert!(prompt.contains("You are a Backend Dev for ACS"));
    }

    #[test]
    fn test_worker_prompt_contains_role_qa() {
        let prompt = worker_prompt("t-002", "Write tests", "Test login", "qa", "qa", "acs");
        assert!(prompt.contains("You are a QA Engineer for ACS"));
    }

    #[test]
    fn test_worker_prompt_contains_ticket_details() {
        let prompt = worker_prompt("t-001", "Build auth", "Add OAuth login flow", "backend", "backend-dev", "acs");
        assert!(prompt.contains("t-001"));
        assert!(prompt.contains("Build auth"));
        assert!(prompt.contains("Add OAuth login flow"));
        assert!(prompt.contains("backend"));
    }

    #[test]
    fn test_worker_prompt_contains_tool_path() {
        let prompt = worker_prompt("t-001", "Test", "Desc", "backend", "backend-dev", "/usr/local/bin/acs");
        assert!(prompt.contains("/usr/local/bin/acs ticket update"));
        assert!(prompt.contains("/usr/local/bin/acs kb read"));
        assert!(prompt.contains("/usr/local/bin/acs inbox push"));
    }

    #[test]
    fn test_worker_prompt_persona_guidance_frontend() {
        let prompt = worker_prompt("t-001", "Build UI", "Add nav", "frontend", "frontend-dev", "acs");
        assert!(prompt.contains("Frontend Dev Guidance"));
    }

    #[test]
    fn test_worker_prompt_persona_guidance_devops() {
        let prompt = worker_prompt("t-001", "Add CI", "Setup pipeline", "devops", "devops", "acs");
        assert!(prompt.contains("DevOps Engineer Guidance"));
    }
}
