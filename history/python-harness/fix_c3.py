"""Apply C3 fix: wrap bus.publish patch in try/finally."""
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as f:
    src = f.read()

# 1. Wrap the body in try: after the _publish_patched line
old_run_start = """    async def run(self, wf_id: str, user_request: str) -> OrchestratorResult:
        ctx = WorkflowCtx(
            wf_id=wf_id,
            actor="agent:chief",
            data={"user_request": user_request},
        )
        sm = StateMachine(ctx, self.bus, initial=State.REQ_RECEIVED)
        self._ctx = ctx
        self._sm = sm"""

new_run_start = """    async def run(self, wf_id: str, user_request: str) -> OrchestratorResult:
        ctx = WorkflowCtx(
            wf_id=wf_id,
            actor="agent:chief",
            data={"user_request": user_request},
        )
        sm = StateMachine(ctx, self.bus, initial=State.REQ_RECEIVED)
        self._ctx = ctx
        self._sm = sm
        # C3 fix: scope the bus.publish monkey-patch with try/finally
        # so the singleton EventBus isn't permanently patched across
        # workflow runs. Without this, each subsequent orchestrator
        # would chain an extra on_event handler on the bus.
        bus_patched = False
        if self.options.on_event:
            _orig_publish = self.bus.publish
            on_event = self.options.on_event

            async def _publish(event) -> None:
                await _orig_publish(event)
                await on_event(event)

            self.bus.publish = _publish  # type: ignore[method-assign]
            bus_patched = True

        try:"""
assert old_run_start in src, "run() start marker not found"
src = src.replace(old_run_start, new_run_start, 1)

# 2. Add the finally block at the end of run() — find the end first
start_idx = src.find("    async def run(")
lines = src.split("\n")
line_idx = src[:start_idx].count("\n")
end_line = len(lines)
for i in range(line_idx + 1, len(lines)):
    if lines[i] and not lines[i].startswith(" ") and not lines[i].startswith("#"):
        end_line = i
        break
    if lines[i].startswith("    ") and not lines[i].startswith("        "):
        end_line = i
        break

# Indent the finally block with 8 spaces
finally_block = (
    "        finally:\n"
    "            if bus_patched:\n"
    "                # Restore the original EventBus.publish so the\n"
    "                # singleton isn't permanently patched.\n"
    "                self.bus.publish = _orig_publish  # type: ignore[method-assign]\n"
    "\n"
)
lines.insert(end_line, finally_block.rstrip("\n"))
src = "\n".join(lines)

with open(path, "w", encoding="utf-8") as f:
    f.write(src)
print("C3 fix applied: bus.publish scoped with try/finally")
PYEOF