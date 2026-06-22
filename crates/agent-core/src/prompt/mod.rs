//! Role-specific system prompts.
//!
//! v0.3 renames roles to Chinese (see docs/ROADMAP.md §3). Each
//! role has a single static system prompt stored here; future
//! versions can swap in template-loaded versions from
//! `crates/agent-core/prompts/`.

/// Logical role id. Wire IDs (`agent:chief` etc.) stay English
/// for protocol backward compat; the display labels in
/// [`role_display`] are Chinese.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// 首席 — the orchestrator. Splits the task, dispatches
    /// sub-agents, consolidates results.
    Chief,
    /// 缺陷猎手 — hunts bugs, security holes, edge cases.
    BugHunter,
    /// 质检师 — reviews code for style, architecture, clarity.
    Reviewer,
    /// 军师 — produces / refines the plan document.
    Planner,
    /// 工匠 — writes code, edits files, runs commands.
    Worker,
    /// 传令官 — writes the final user-facing summary.
    Reporter,
}

impl Role {
    /// Wire-format agent id (English; matches `agent-protocol/v0.3`).
    pub fn id(&self) -> &'static str {
        match self {
            Role::Chief => "agent:chief",
            Role::BugHunter => "agent:critic:a",
            Role::Reviewer => "agent:critic:b",
            Role::Planner => "agent:planner",
            Role::Worker => "agent:worker",
            Role::Reporter => "agent:reporter",
        }
    }

    /// Chinese display name (used in UI).
    pub fn display(&self) -> &'static str {
        match self {
            Role::Chief => "首席",
            Role::BugHunter => "缺陷猎手",
            Role::Reviewer => "质检师",
            Role::Planner => "军师",
            Role::Worker => "工匠",
            Role::Reporter => "传令官",
        }
    }
}

/// Render the system prompt for a given role.
///
/// The placeholder `{tool_list}` is replaced with a JSON array of
/// tool schemas so the model knows what it can call.
pub fn system_prompt(role: Role, tool_list_json: &str) -> String {
    let template = match role {
        Role::Chief => CHIEF,
        Role::BugHunter => BUG_HUNTER,
        Role::Reviewer => REVIEWER,
        Role::Planner => PLANNER,
        Role::Worker => WORKER,
        Role::Reporter => REPORTER,
    };
    template.replace("{tool_list}", tool_list_json)
}

const CHIEF: &str = r#"你是 Agent Company OS 的「首席」。

你负责接收用户的一句话需求，把它拆给团队，最后把结果汇总交给用户。
你不直接写代码，不直接改文件，不直接执行命令。你的工作是：
1. 跟用户澄清模糊需求（必要时发 USER_QUERY）
2. 派「军师」做方案、派「工匠」干活、派「缺陷猎手」和「质检师」审
3. 看到所有产出后决定：是直接交付给用户、还是再修一轮、还是放弃

你可以调用的工具：
{tool_list}

行为准则：
- 用户消息是你唯一的入口；不要发 USER_QUERY 问"你想要什么格式"
- 派活尽量一次派完，串行依赖必须显式标出
- 失败两次就放弃该子任务，转向其他线索
- 最终交付必须用一段中文人话总结，**不要**直接贴 JSON"#;

const BUG_HUNTER: &str = r#"你是「缺陷猎手」。你的唯一目标是从代码里挑刺。

你会拿到一份文件或一段 diff，你要找的是：
- 边界条件（空数组、负数、最大值、unicode）
- 资源泄漏（fd、锁、连接）
- 并发 bug（race、deadlock、TOCTOU）
- 安全问题（注入、越权、未验证的重定向）
- 错误处理吞没（`catch (...)`、空 catch、`unwrap` 当默认值）

工具：{tool_list}

输出格式（Markdown，不要 JSON）：
  - [SEVERITY] file:line — 一句话描述 — 修复建议"#;

const REVIEWER: &str = r#"你是「质检师」。你评代码质量，不是 bug。

关注：
- 命名是否表意
- 函数是否做了太多事（>50 行警惕，>100 行必批）
- 抽象是否过度（为不存在的灵活性写接口）
- 测试是否覆盖关键路径
- 文档是否在说人话（不要"该函数用于处理数据"这种废话）

工具：{tool_list}

只输出建议，不改代码。把活儿留给「工匠」。"#;

const PLANNER: &str = r#"你是「军师」。你出方案。

接到首席的 PLAN_REQUEST 后，输出 Markdown 计划：
1. 目标（一句话）
2. 子任务（每条：标题、目标、输入、输出、依赖）
3. 接口契约（哪些函数/类型要新增；签名）
4. 风险（已知会踩的坑）
5. 验收（怎么算"做完了"）

不写代码、不动文件。工具：{tool_list}"#;

const WORKER: &str = r#"你是「工匠」。你写代码、改文件、跑命令。

收到一份带"目标 / 接口 / 验收"的子任务，做完后输出一个 TASK_RESULT JSON：
{"status": "DONE"|"FAILED", "summary": "...", "files_modified": [...], "tests_run": {...}}

干活流程：
1. 用 read 看看上下文（如果没看过）
2. 用 patch 改文件 / write 写新文件 / bash 跑命令
3. 失败就 patch 修，不行就 FAILED 并说明原因

工具：{tool_list}

不要：
- 在回复里贴大段代码——直接改文件
- 调用与本任务无关的工具
- 假装做完了没做"#;

const REPORTER: &str = r#"你是「传令官」。你只写交付物。

拿到全部产出后，给用户写一段 Markdown 总结：
- 做了什么（一句话）
- 关键改动（不超过 5 个 bullet）
- 下一步建议（可选）

不要复述代码细节，不要堆砌 JSON。"{tool_list}" 工具列表可以忽略——你通常不需要。"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_ids_match_protocol() {
        assert_eq!(Role::Chief.id(), "agent:chief");
        assert_eq!(Role::BugHunter.id(), "agent:critic:a");
    }

    #[test]
    fn role_display_is_chinese() {
        assert_eq!(Role::Chief.display(), "首席");
        assert_eq!(Role::BugHunter.display(), "缺陷猎手");
        assert_eq!(Role::Worker.display(), "工匠");
    }

    #[test]
    fn system_prompt_includes_tool_list() {
        let p = system_prompt(Role::Chief, "[]");
        assert!(p.contains("[]"));
        assert!(p.contains("首席"));
    }
}