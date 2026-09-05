//! Token budget + summarization.
//!
//! The context manager counts tokens (rough heuristic: 4 chars ≈
//! 1 token), enforces a budget, and trims old messages when the
//! budget is exceeded. Full LLM-based summarization is a
//! later-add; the v0.3 implementation just drops oldest
//! non-system messages until the budget fits.

use crate::message::{Message, Role};

/// Heuristic: ~4 chars per token. Real model-specific counts come
/// later via each provider's tokenizer.
pub const APPROX_CHARS_PER_TOKEN: usize = 4;

/// Configuration for the context window.
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Soft cap on tokens. Loop will compact at this point.
    pub budget: usize,
    /// Hard cap (e.g. provider's max context). Loop will refuse
    /// to send a request larger than this.
    pub hard_limit: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            budget: 100_000,
            hard_limit: 200_000,
        }
    }
}

/// Counts tokens, compacts the history when over budget.
#[derive(Debug, Clone)]
pub struct ContextManager {
    cfg: ContextConfig,
}

impl ContextManager {
    /// Build with a custom budget.
    pub fn new(cfg: ContextConfig) -> Self {
        Self { cfg }
    }

    /// Estimate token count of a message.
    pub fn count_message(m: &Message) -> usize {
        let chars = m.content.len()
            + m.tool_calls
                .iter()
                .map(|tc| tc.name.len() + tc.args.to_string().len())
                .sum::<usize>();
        chars.div_ceil(APPROX_CHARS_PER_TOKEN)
    }

    /// Estimate token count of a message list.
    pub fn count(messages: &[Message]) -> usize {
        messages.iter().map(Self::count_message).sum()
    }

    /// Compact the message list so it fits the budget. System
    /// messages are always preserved; user + assistant + tool
    /// messages are trimmed oldest-first.
    pub fn compact(&self, messages: Vec<Message>) -> Vec<Message> {
        let total = Self::count(&messages);
        if total <= self.cfg.budget {
            return messages;
        }
        let (system, rest): (Vec<_>, Vec<_>) =
            messages.into_iter().partition(|m| m.role == Role::System);
        let mut budget_left = self.cfg.budget.saturating_sub(Self::count(&system));

        // Group `rest` into atomic message blocks.
        // An assistant message with tool_calls and all subsequent tool messages
        // form a single atomic block that must never be severed.
        let mut blocks: Vec<Vec<Message>> = Vec::new();
        let mut current_block: Vec<Message> = Vec::new();

        for m in rest {
            if m.role == Role::Tool {
                // Tool result belongs to the preceding Assistant block
                current_block.push(m);
            } else {
                if !current_block.is_empty() {
                    blocks.push(current_block);
                    current_block = Vec::new();
                }
                current_block.push(m);
            }
        }
        if !current_block.is_empty() {
            blocks.push(current_block);
        }

        // Keep blocks from the tail (most recent context is most useful).
        blocks.reverse();
        let mut kept_blocks_rev = Vec::new();
        for block in blocks {
            let block_tokens: usize = block.iter().map(Self::count_message).sum();
            if budget_left >= block_tokens {
                budget_left -= block_tokens;
                kept_blocks_rev.push(block);
            } else if kept_blocks_rev.is_empty() {
                // If even the very latest block is larger than budget_left,
                // keep it anyway so we don't return an empty history.
                kept_blocks_rev.push(block);
                break;
            } else {
                // Cannot fit older blocks, stop
                break;
            }
        }
        kept_blocks_rev.reverse();
        let mut flattened: Vec<Message> = kept_blocks_rev.into_iter().flatten().collect();

        // Ensure the first non-system message is a User message if any message was kept.
        if let Some(first) = flattened.first() {
            if first.role != Role::User {
                flattened.insert(0, Message::user("[Earlier context truncated]"));
            }
        }

        system.into_iter().chain(flattened).collect()
    }

    /// Hard-limit check. Returns Err if the history exceeds the
    /// provider's max context.
    pub fn enforce_hard_limit(&self, messages: &[Message]) -> Result<(), crate::AgentError> {
        let used = Self::count(messages);
        if used > self.cfg.hard_limit {
            return Err(crate::AgentError::ContextBudgetExhausted {
                used,
                budget: self.cfg.hard_limit,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::ToolCall;

    #[test]
    fn count_increases_with_content() {
        let m1 = Message::user("hi");
        let m2 = Message::user("a".repeat(4000));
        assert!(ContextManager::count_message(&m2) > ContextManager::count_message(&m1));
    }

    #[test]
    fn compact_drops_oldest_first() {
        let cfg = ContextConfig {
            budget: 50,
            hard_limit: 200,
        };
        let m = ContextManager::new(cfg);
        let msgs = vec![
            Message::system("you are concise"),
            Message::user("a".repeat(400)),              // ~100 tokens
            Message::assistant("b".repeat(400), vec![]), // ~100 tokens
            Message::user("recent"),                     // 1 token
        ];
        let compacted = m.compact(msgs);
        assert_eq!(compacted[0].content, "you are concise");
        // The recent user message should survive; the bulk history may be dropped.
        assert!(compacted.iter().any(|x| x.content == "recent"));
    }

    #[test]
    fn compact_preserves_atomic_tool_call_pairs() {
        let cfg = ContextConfig {
            budget: 30,
            hard_limit: 200,
        };
        let m = ContextManager::new(cfg);
        let msgs = vec![
            Message::system("sys"),
            Message::user("old question"),
            Message::assistant(
                "calling tool",
                vec![ToolCall {
                    id: "call_1".into(),
                    name: "read".into(),
                    args: serde_json::json!({ "path": "foo.txt" }),
                }],
            ),
            Message::tool("call_1", "file content here"),
        ];
        let compacted = m.compact(msgs);
        // The assistant and its tool message must both be present or both absent.
        let has_assistant = compacted.iter().any(|x| x.role == Role::Assistant);
        let has_tool = compacted.iter().any(|x| x.role == Role::Tool);
        assert_eq!(has_assistant, has_tool);
        // The first non-system message must be a user message
        if compacted.len() > 1 {
            assert_eq!(compacted[1].role, Role::User);
        }
    }
}
