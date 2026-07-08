//! Per-turn outbound budget: bounds how many drafts / ungated external sends a
//! single LLM turn may produce, keyed by `turn_id`. Rule automation and
//! human-gated sends are governed by plan-05's outbound gate, not this budget.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub const DRAFT_LIMIT_PER_TURN: usize = 5;
pub const UNGATED_EXTERNAL_LIMIT_PER_TURN: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundBudgetKind {
    Draft,
    UngatedExternal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundBudgetError {
    Exceeded,
}

#[derive(Debug, Default)]
struct TurnBudget {
    drafts: usize,
    ungated_external: usize,
}

pub struct OutboundBudgetRegistry {
    budgets: Mutex<HashMap<String, TurnBudget>>,
    draft_limit: usize,
    ungated_external_limit: usize,
}

impl OutboundBudgetRegistry {
    pub fn with_limits(draft_limit: usize, ungated_external_limit: usize) -> Self {
        Self {
            budgets: Mutex::new(HashMap::new()),
            draft_limit,
            ungated_external_limit,
        }
    }

    pub fn charge(
        &self,
        turn_id: &str,
        kind: OutboundBudgetKind,
    ) -> Result<(), OutboundBudgetError> {
        let mut budgets = self.budgets.lock().unwrap();
        let budget = budgets.entry(turn_id.to_string()).or_default();
        let (count, limit) = match kind {
            OutboundBudgetKind::Draft => (&mut budget.drafts, self.draft_limit),
            OutboundBudgetKind::UngatedExternal => {
                (&mut budget.ungated_external, self.ungated_external_limit)
            }
        };
        if *count >= limit {
            return Err(OutboundBudgetError::Exceeded);
        }
        *count += 1;
        Ok(())
    }

    pub fn clear_turn(&self, turn_id: &str) {
        self.budgets.lock().unwrap().remove(turn_id);
    }
}

static REGISTRY: OnceLock<OutboundBudgetRegistry> = OnceLock::new();

pub fn budget_registry() -> &'static OutboundBudgetRegistry {
    REGISTRY.get_or_init(|| {
        OutboundBudgetRegistry::with_limits(DRAFT_LIMIT_PER_TURN, UNGATED_EXTERNAL_LIMIT_PER_TURN)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_rejects_after_limit_and_clears_by_turn() {
        let registry = OutboundBudgetRegistry::with_limits(2, 1);
        registry
            .charge("turn-1", OutboundBudgetKind::Draft)
            .unwrap();
        registry
            .charge("turn-1", OutboundBudgetKind::Draft)
            .unwrap();
        assert_eq!(
            registry
                .charge("turn-1", OutboundBudgetKind::Draft)
                .unwrap_err(),
            OutboundBudgetError::Exceeded
        );
        registry
            .charge("turn-2", OutboundBudgetKind::Draft)
            .unwrap(); // other turn unaffected
        registry.clear_turn("turn-1");
        registry
            .charge("turn-1", OutboundBudgetKind::Draft)
            .unwrap();
    }
}
