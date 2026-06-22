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
        let (system, mut rest): (Vec<_>, Vec<_>) =
            messages.into_iter().partition(|m| m.role == Role::System);
        let mut budget_left = self.cfg.budget.saturating_sub(Self::count(&system));
        // Keep the *tail* (most recent context is most useful).
        rest.reverse();
        let mut kept_rev = Vec::new();
        for m in rest {
            let c = Self::count_message(&m);
            if budget_left >= c {
                budget_left -= c;
                kept_rev.push(m);
            }
        }
        kept_rev.reverse();
        system.into_iter().chain(kept_rev).collect()
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

    #[test]
    fn count_increases_with_content() {
        let m1 = Message::user("hi");
        let m2 = Message::user(&"a".repeat(4000));
        assert!(ContextManager::count_message(&m2) > ContextManager::count_message(&m1));
    }

    #[test]
    fn compact_drops_oldest_first() {
        let cfg = ContextConfig { budget: 50, hard_limit: 200 };
        let m = ContextManager::new(cfg);
        let msgs = vec![
            Message::system("you are concise"),
            Message::user(&"a".repeat(400)),  // ~100 tokens
            Message::assistant(&"b".repeat(400), vec![]), // ~100 tokens
            Message::user("recent"), // 1 token
        ];
        let compacted = m.compact(msgs);
        assert_eq!(compacted[0].content, "you are concise");
        // The recent user message should survive; the bulk history may be dropped.
        assert!(compacted.iter().any(|x| x.content == "recent"));
    }
}