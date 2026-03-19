from synapse_os.prompts.bootstrap import build_bootstrap_prompt
from synapse_os.prompts.manager import build_manager_prompt
from synapse_os.prompts.worker import build_worker_prompt


def test_bootstrap_prompt_contains_key_instructions():
    prompt = build_bootstrap_prompt(repo_path="/tmp/myrepo", spec_text="Build a todo app")
    assert "analyze" in prompt.lower()
    assert "create_ticket" in prompt
    assert "/tmp/myrepo" in prompt
    assert "todo app" in prompt


def test_bootstrap_prompt_without_spec():
    prompt = build_bootstrap_prompt(repo_path="/tmp/myrepo")
    assert "/tmp/myrepo" in prompt
    assert "create_ticket" in prompt


def test_manager_prompt_contains_key_instructions():
    prompt = build_manager_prompt(domain="frontend", project_summary="React todo app")
    assert "frontend" in prompt
    assert "assign_ticket" in prompt
    assert "create_ticket" in prompt


def test_worker_prompt_contains_key_instructions():
    prompt = build_worker_prompt(
        ticket_id="t-1", title="Build login page",
        description="Create a login form", domain="frontend",
    )
    assert "t-1" in prompt
    assert "Build login page" in prompt
    assert "notify_manager" in prompt
