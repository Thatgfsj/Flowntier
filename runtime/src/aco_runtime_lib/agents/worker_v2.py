"""Worker agent v2 — actually executes code and writes files.

This worker uses the plugin system to:
1. Write files to disk
2. Execute Python code
3. Run tests
4. Return real results

See `prompts/worker.md` and `docs/AGENT_PROTOCOL.md` §5.1-§5.3.
"""

from __future__ import annotations

import json
import os
from typing import Any

from aco_runtime_lib.agents._json_extract import extract_all_json_objects
from aco_runtime_lib.agents.base import Agent, AgentResult, AgentRole
from aco_runtime_lib.plugins.base import get_registry
from aco_runtime_lib.providers.base import (
    ChatMessage,
    ChatRequest,
    ProviderError,
)
from aco_runtime_lib.providers.router import ModelRouter

WORKER_SYSTEM_PROMPT_V2 = """\
You are a Worker in Agent Company OS. You have access to tools that
let you actually write files and execute code.

Available tools:
1. python - Execute Python code
   Usage: {"plugin": "python", "args": {"code": "print(1+1)"}}

2. write_file - Write content to a file
   Usage: {"plugin": "write_file", "args": {"path": "hello.py", "content": "..."}}

3. read_file - Read a file
   Usage: {"plugin": "read_file", "args": {"path": "hello.py"}}

4. run_tests - Run pytest
   Usage: {"plugin": "python", "args": {"code": "import subprocess; result = subprocess.run(['pytest', '-v'], capture_output=True, text=True); print(result.stdout); print(result.stderr)"}}

For each task, you should:
1. Write the actual code files
2. Run tests if applicable
3. Return a TASK_RESULT JSON

Output format (no prose, no markdown fences):

{
  "task_id": "<the id from the task envelope>",
  "status": "DONE" | "FAILED" | "PARTIAL",
  "summary": "<one-paragraph summary>",
  "files_modified": [
    {"path": "src/example.py", "lines_added": 5, "lines_removed": 2}
  ],
  "tests_run": {"passed": 0, "failed": 0, "skipped": 0}
}

IMPORTANT: Actually write the files and run the code. Don't just describe what you would do.
"""


class WorkerAgentV2(Agent):
    """Worker that actually executes code and writes files."""

    role = AgentRole.WORKER

    def __init__(self, router: ModelRouter, work_dir: str = ".") -> None:
        self._router = router
        self._work_dir = work_dir
        self._registry = get_registry()

    async def run(self, ctx: dict[str, Any]) -> AgentResult:
        provider, ref = self._router.pick("worker")
        task_id = ctx.get("task_id", "unknown")
        title = ctx.get("title", "unknown")

        # First, ask the LLM what to do
        request = ChatRequest(
            model=ref.model_id,
            messages=[
                ChatMessage(role="system", content=WORKER_SYSTEM_PROMPT_V2),
                ChatMessage(
                    role="user",
                    content=self._render_task_envelope(ctx),
                ),
            ],
            max_tokens=2048,
            temperature=0.2,
        )

        try:
            response = await provider.chat(request)
        except ProviderError as e:
            return AgentResult(
                role=self.role,
                data={"error": "provider_error", "message": str(e), "retryable": e.retryable},
            )

        # Parse the response to find tool calls and TASK_RESULT
        result = self._execute_response(response.content, task_id)
        return AgentResult(role=self.role, data=result)

    def _execute_response(self, content: str, task_id: str) -> dict[str, Any]:
        """Execute any tool calls in the response and extract TASK_RESULT."""
        import asyncio

        # Extract all JSON objects from the response
        json_objects = extract_all_json_objects(content)

        files_modified = []
        tests_run = {"passed": 0, "failed": 0, "skipped": 0}
        summary = ""

        for obj in json_objects:
            # Check if this is a tool call
            if "plugin" in obj:
                plugin_name = obj["plugin"]
                args = obj.get("args", {})

                # Execute the tool
                try:
                    result = asyncio.get_event_loop().run_until_complete(
                        self._registry.invoke(plugin_name, args)
                    )

                    # Track file modifications
                    if plugin_name == "write_file" and result.get("status") == "ok":
                        path = args.get("path", "")
                        content = args.get("content", "")
                        files_modified.append({
                            "path": path,
                            "lines_added": len(content.split("\n")),
                            "lines_removed": 0,
                        })

                    # Track test results
                    if plugin_name == "python" and "pytest" in args.get("code", ""):
                        stdout = result.get("stdout", "")
                        # Parse pytest output
                        if "passed" in stdout:
                            import re
                            match = re.search(r"(\d+) passed", stdout)
                            if match:
                                tests_run["passed"] = int(match.group(1))
                            match = re.search(r"(\d+) failed", stdout)
                            if match:
                                tests_run["failed"] = int(match.group(1))

                except Exception as e:
                    # Tool execution failed
                    return {
                        "task_id": task_id,
                        "status": "FAILED",
                        "summary": f"Tool execution failed: {e}",
                        "files_modified": [],
                        "tests_run": {"passed": 0, "failed": 1, "skipped": 0},
                    }

            # Check if this is the TASK_RESULT
            if "task_id" in obj and "status" in obj:
                summary = obj.get("summary", "")
                if not files_modified:
                    files_modified = obj.get("files_modified", [])
                if tests_run["passed"] == 0 and tests_run["failed"] == 0:
                    tests_run = obj.get("tests_run", {"passed": 0, "failed": 0, "skipped": 0})

        # If no TASK_RESULT found, create one from what we executed
        if not summary:
            if files_modified:
                summary = f"Modified {len(files_modified)} files: {', '.join(f['path'] for f in files_modified)}"
            else:
                summary = "Task completed"

        return {
            "task_id": task_id,
            "status": "DONE",
            "summary": summary,
            "files_modified": files_modified,
            "tests_run": tests_run,
        }

    def _render_task_envelope(self, ctx: dict[str, Any]) -> str:
        """Render a `TASK_ASSIGN` payload as the user-side prompt."""
        lines: list[str] = []
        lines.append(f"# Task: {ctx.get('title', '')}")
        lines.append(f"**Task ID:** {ctx.get('task_id', '')}")
        lines.append("")
        lines.append("## Objective")
        lines.append(ctx.get("objective", ""))
        lines.append("")
        lines.append("## Working Directory")
        lines.append(f"`{self._work_dir}`")
        lines.append("")
        lines.append("## Instructions")
        lines.append("1. Write the actual code files using write_file")
        lines.append("2. Run tests using python plugin if applicable")
        lines.append("3. Return TASK_RESULT JSON")
        lines.append("")
        interfaces = ctx.get("interfaces", {})
        if interfaces:
            lines.append("## Interfaces you consume")
            for c in interfaces.get("consumes", []):
                lines.append(f"- {c}")
            lines.append("")
            lines.append("## Interfaces you produce")
            for p in interfaces.get("produces", []):
                lines.append(f"- {p}")
            lines.append("")
        constraints = ctx.get("constraints", [])
        if constraints:
            lines.append("## Constraints")
            for c in constraints:
                lines.append(f"- {c}")
            lines.append("")
        deliverables = ctx.get("deliverables", [])
        if deliverables:
            lines.append("## Deliverables")
            for d in deliverables:
                lines.append(f"- {d}")
            lines.append("")
        return "\n".join(lines)
