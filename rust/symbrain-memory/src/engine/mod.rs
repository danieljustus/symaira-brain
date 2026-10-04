//! Native memory-engine primitives; public CLI/MCP cutover remains gated.
//!
//! These algorithms do not perform HTTP, spawn providers or mutate stores.
//! The extraction API requires UTF-8 until the original byte-valued source and
//! grounded-evidence bridge has actual process acceptance.

mod aging;
mod budget;
mod extractor;
mod summarizer;
mod text;

#[cfg(test)]
mod tests;

pub use aging::{AgingConfig, decay_factor};
pub use budget::{BudgetPiece, BudgetReport, enforce_budget, estimate_tokens};
pub use extractor::{ExtractedFact, PatternExtractor};
pub use summarizer::summarize_session;
