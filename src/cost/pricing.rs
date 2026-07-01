//! Pure pricing: convert token usage into USD via a per-model price map. The
//! map is configurable; unknown models fall back to a default price.

use super::model::{GroupModelTokens, ModelTokens};
use std::collections::HashMap;

/// Price for one model, in USD per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPrice {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
}

/// USD cost of an (input, output) token count at a given price.
pub fn cost(input_tokens: i64, output_tokens: i64, price: ModelPrice) -> f64 {
    (input_tokens as f64 / 1_000_000.0) * price.input_per_mtok
        + (output_tokens as f64 / 1_000_000.0) * price.output_per_mtok
}

/// Per-model price map with a default fallback for unknown models.
#[derive(Debug, Clone)]
pub struct PriceTable {
    prices: HashMap<String, ModelPrice>,
    default: ModelPrice,
}

impl PriceTable {
    pub fn new(prices: HashMap<String, ModelPrice>, default: ModelPrice) -> Self {
        Self { prices, default }
    }

    /// Price for a model. An exact (case-insensitive) match wins; otherwise the
    /// longest known-key prefix, so dated ids (`claude-opus-4-8-20260101`)
    /// resolve to their family price; failing that, the default.
    pub fn price_for(&self, model: &str) -> ModelPrice {
        let m = model.to_ascii_lowercase();
        if let Some(p) = self.prices.get(&m) {
            return *p;
        }
        let mut best: Option<(&String, ModelPrice)> = None;
        for (k, v) in &self.prices {
            if m.starts_with(k.as_str()) && best.as_ref().is_none_or(|(bk, _)| k.len() > bk.len()) {
                best = Some((k, *v));
            }
        }
        best.map(|(_, v)| v).unwrap_or(self.default)
    }

    /// Cost of one model's token totals.
    pub fn cost_for(&self, model: &str, input_tokens: i64, output_tokens: i64) -> f64 {
        cost(input_tokens, output_tokens, self.price_for(model))
    }

    /// Total cost of a set of per-model token totals.
    pub fn total(&self, rows: &[ModelTokens]) -> f64 {
        rows.iter().map(|r| self.cost_for(&r.model, r.input_tokens, r.output_tokens)).sum()
    }

    /// The built-in defaults with caller-supplied per-model overrides merged in
    /// (keys lower-cased), optionally replacing the unknown-model default price.
    pub fn with_overrides(overrides: HashMap<String, ModelPrice>, default: Option<ModelPrice>) -> Self {
        let mut table = Self::default();
        for (k, v) in overrides {
            table.prices.insert(k.to_ascii_lowercase(), v);
        }
        if let Some(d) = default {
            table.default = d;
        }
        table
    }
}

impl Default for PriceTable {
    fn default() -> Self {
        // Seeded with current Claude family prices (USD / million tokens).
        let opus = ModelPrice { input_per_mtok: 15.0, output_per_mtok: 75.0 };
        let mut prices = HashMap::new();
        prices.insert("claude-opus-4".into(), opus);
        prices.insert("claude-sonnet-4".into(), ModelPrice { input_per_mtok: 3.0, output_per_mtok: 15.0 });
        prices.insert("claude-haiku-4".into(), ModelPrice { input_per_mtok: 1.0, output_per_mtok: 5.0 });
        prices.insert("claude-fable-5".into(), ModelPrice { input_per_mtok: 3.0, output_per_mtok: 15.0 });
        Self { prices, default: opus }
    }
}

/// A priced rollup row for the cost panel.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CostRow {
    pub key: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: f64,
}

/// Price per-(key, model) token totals and aggregate them by key into cost
/// rows, sorted by descending cost. Pricing per model first keeps the cost
/// accurate even though the rollup groups across models.
pub fn price_rows(rows: Vec<GroupModelTokens>, prices: &PriceTable) -> Vec<CostRow> {
    let mut by_key: HashMap<String, CostRow> = HashMap::new();
    for r in rows {
        let c = prices.cost_for(&r.model, r.input_tokens, r.output_tokens);
        let entry = by_key.entry(r.key.clone()).or_insert(CostRow {
            key: r.key,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: 0.0,
        });
        entry.input_tokens += r.input_tokens;
        entry.output_tokens += r.output_tokens;
        entry.cost_usd += c;
    }
    let mut out: Vec<CostRow> = by_key.into_values().collect();
    out.sort_by(|a, b| b.cost_usd.partial_cmp(&a.cost_usd).unwrap_or(std::cmp::Ordering::Equal));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_math() {
        let price = ModelPrice { input_per_mtok: 10.0, output_per_mtok: 20.0 };
        // 1M input @ $10 + 0.5M output @ $20 = 10 + 10 = 20.
        assert!((cost(1_000_000, 500_000, price) - 20.0).abs() < 1e-9);
        assert_eq!(cost(0, 0, price), 0.0);
    }

    #[test]
    fn exact_and_prefix_and_default() {
        let t = PriceTable::default();
        // Prefix match: a dated opus id resolves to the opus family price.
        assert_eq!(t.price_for("claude-opus-4-8-20260101").output_per_mtok, 75.0);
        // Case-insensitive.
        assert_eq!(t.price_for("CLAUDE-SONNET-4-6").output_per_mtok, 15.0);
        // Unknown → default (opus).
        assert_eq!(t.price_for("gpt-4o"), t.price_for("claude-opus-4"));
    }

    #[test]
    fn price_rows_aggregates_by_key_sorted_by_cost() {
        let t = PriceTable::new(
            HashMap::from([
                ("cheap".into(), ModelPrice { input_per_mtok: 1.0, output_per_mtok: 1.0 }),
                ("dear".into(), ModelPrice { input_per_mtok: 100.0, output_per_mtok: 100.0 }),
            ]),
            ModelPrice { input_per_mtok: 0.0, output_per_mtok: 0.0 },
        );
        let rows = vec![
            GroupModelTokens { key: "a".into(), model: "cheap".into(), input_tokens: 1_000_000, output_tokens: 0 },
            GroupModelTokens { key: "a".into(), model: "dear".into(), input_tokens: 1_000_000, output_tokens: 0 },
            GroupModelTokens { key: "b".into(), model: "cheap".into(), input_tokens: 2_000_000, output_tokens: 0 },
        ];
        let out = price_rows(rows, &t);
        // a: 1 + 100 = 101 ; b: 2. Sorted by cost desc → a first.
        assert_eq!(out[0].key, "a");
        assert!((out[0].cost_usd - 101.0).abs() < 1e-9);
        assert_eq!(out[0].input_tokens, 2_000_000);
        assert_eq!(out[1].key, "b");
        assert!((out[1].cost_usd - 2.0).abs() < 1e-9);
    }

    #[test]
    fn total_sums_per_model() {
        let t = PriceTable::new(
            HashMap::from([("m".into(), ModelPrice { input_per_mtok: 1.0, output_per_mtok: 2.0 })]),
            ModelPrice { input_per_mtok: 0.0, output_per_mtok: 0.0 },
        );
        let rows = vec![
            ModelTokens { model: "m".into(), input_tokens: 1_000_000, output_tokens: 0 },
            ModelTokens { model: "unknown".into(), input_tokens: 5_000_000, output_tokens: 0 },
        ];
        // m: 1.0 ; unknown → default (0). Total = 1.0.
        assert!((t.total(&rows) - 1.0).abs() < 1e-9);
    }
}
