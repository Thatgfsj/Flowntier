"""Worker agent v2 — actually executes code and writes files.

Strategy:
1. Ask LLM to generate code in markdown code blocks
2. Parse code blocks from response
3. Write files to disk using file_ops plugin
4. Return TASK_RESULT

This is more robust than relying on LLM-generated tool calls,
because LLMs reliably produce markdown code blocks.
"""

from __future__ import annotations

import re
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
You are a Worker in Agent Company OS. You MUST write actual code files.

CRITICAL RULES:
1. Write ALL code in markdown code blocks with filenames
2. Use this EXACT format for each file:

```python:filename.py
def hello():
    return "Hello World"
```

3. After writing all code blocks, return a TASK_RESULT JSON:

```json
{"task_id": "TASK_ID_HERE", "status": "DONE", "summary": "what you did"}
```

Example - if asked to create hello.py with a hello function:

I will create hello.py with the hello function.

```python:hello.py
def hello(name="World"):
    """Return a greeting."""
    return f"Hello, {name}!"
```

```python:test_hello.py
from hello import hello

def test_hello_default():
    assert hello() == "Hello, World!"

def test_hello_name():
    assert hello("Alice") == "Hello, Alice!"
```

```json
{"task_id": "T1", "status": "DONE", "summary": "Created hello.py and test_hello.py"}
```

IMPORTANT:
- Always use the format ```language:filename.ext for code blocks
- Include the filename after the colon
- Write complete, runnable code
- Include tests when appropriate
"""


class WorkerAgentV2(Agent):
    """Worker that actually writes files from code blocks in LLM response."""

    role = AgentRole.WORKER

    def __init__(self, router: ModelRouter, work_dir: str = ".") -> None:
        self._router = router
        self._work_dir = work_dir
        self._registry = get_registry()

    async def run(self, ctx: dict[str, Any]) -> AgentResult:
        provider, ref = self._router.pick("worker")
        task_id = ctx.get("task_id", "unknown")

        request = ChatRequest(
            model=ref.model_id,
            messages=[
                ChatMessage(role="system", content=WORKER_SYSTEM_PROMPT_V2),
                ChatMessage(role="user", content=self._render_task_envelope(ctx)),
            ],
            max_tokens=4096,
            temperature=0.2,
        )

        try:
            response = await provider.chat(request)
        except ProviderError as e:
            return AgentResult(
                role=self.role,
                data={"error": "provider_error", "message": str(e), "retryable": e.retryable},
            )

        result = await self._process_response(response.content, task_id)
        return AgentResult(role=self.role, data=result)

    async def _process_response(self, content: str, task_id: str) -> dict[str, Any]:
        """Extract code blocks, write files, extract TASK_RESULT."""
        files_modified = []
        tests_run = {"passed": 0, "failed": 0, "skipped": 0}

        # 1. Extract code blocks with filenames: ```lang:filename\n...\n```
        code_blocks = self._extract_code_blocks(content)

        for filename, code in code_blocks:
            result = await self._registry.invoke("file_ops", {
                "action": "write_file",
                "path": filename,
                "content": code,
            })
            if result.get("status") == "ok":
                files_modified.append({
                    "path": filename,
                    "lines_added": result.get("lines_added", 0),
                    "lines_removed": result.get("lines_removed", 0),
                })

        # 2. Extract TASK_RESULT JSON (if present)
        json_objects = extract_all_json_objects(content)
        summary = ""
        for obj in json_objects:
            if "task_id" in obj and "status" in obj:
                summary = obj.get("summary", "")
                if obj.get("tests_run"):
                    tests_run = obj["tests_run"]

        # 3. If no explicit summary, generate one
        if not summary:
            if files_modified:
                paths = ", ".join(f["path"] for f in files_modified)
                summary = f"Created {len(files_modified)} file(s): {paths}"
            else:
                summary = "Task completed (no files written)"

        return {
            "task_id": task_id,
            "status": "DONE",
            "summary": summary,
            "files_modified": files_modified,
            "tests_run": tests_run,
        }

    def _extract_code_blocks(self, content: str) -> list[tuple[str, str]]:
        """Extract code blocks with filenames from markdown.

        Matches patterns like:
            ```python:hello.py
            code here
            ```
            ```js:src/app.js
            code here
            ```
            ```hello.py
            code here
            ```
        """
        # Pattern: ```lang:filename\n...\n```  OR  ```filename\n...\n```
        pattern = r'```(?:\w+:)?([^\n`]+)\n(.*?)```'
        matches = re.findall(pattern, content, re.DOTALL)

        results = []
        for filename, code in matches:
            filename = filename.strip()
            code = code.strip()
            # Skip JSON blocks (they're TASK_RESULT, not files)
            if filename.endswith('.json') or filename == 'json':
                continue
            # Skip if filename looks like a language name
            if filename in ('python', 'javascript', 'typescript', 'rust', 'bash', 'sh', 'sql', 'html', 'css'):
                continue
            if code:
                results.append((filename, code))

        return results

    def _render_task_envelope(self, ctx: dict[str, Any]) -> str:
        """Render task envelope as user prompt."""
        lines: list[str] = []
        lines.append(f"# Task: {ctx.get('title', '')}")
        lines.append(f"**Task ID:** {ctx.get('task_id', '')}")
        lines.append("")
        lines.append("## Objective")
        lines.append(ctx.get("objective", ""))
        lines.append("")
        lines.append("## Instructions")
        lines.append("Write the code in markdown code blocks with filenames.")
        lines.append("Use the format: ```language:filename.ext")
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
