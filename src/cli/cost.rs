use anyhow::Result;
use crate::db::Db;
use crate::models::pricing;

pub fn execute() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let db = Db::open(&acs_dir.join("project.db"))?;

    let (total_input, total_output) = db.total_token_details()?;
    let total_tokens = total_input + total_output;

    println!("Token Usage & Cost Estimate");
    println!("===========================");
    println!("Total tokens:  {:>14}", fmt_tokens(total_tokens));
    println!("  Input:       {:>14}", fmt_tokens(total_input));
    println!("  Output:      {:>14}", fmt_tokens(total_output));

    // Per-ticket breakdown
    let per_ticket = db.token_breakdown_by_ticket()?;
    if !per_ticket.is_empty() {
        println!("\nPer-Ticket Breakdown:");
        println!("{:<8}  {:>14}  {:>14}  {:>12}  {:>12}",
            "Ticket", "Input", "Output", "Sonnet $", "Opus $");
        println!("{}", "-".repeat(68));
        for usage in &per_ticket {
            let sonnet_cost = pricing::estimate_cost(
                usage.input_tokens,
                usage.output_tokens,
                pricing::SONNET_INPUT_PER_M,
                pricing::SONNET_OUTPUT_PER_M,
            );
            let opus_cost = pricing::estimate_cost(
                usage.input_tokens,
                usage.output_tokens,
                pricing::OPUS_INPUT_PER_M,
                pricing::OPUS_OUTPUT_PER_M,
            );
            println!("{:<8}  {:>14}  {:>14}  {:>12}  {:>12}",
                usage.ticket_id,
                fmt_tokens(usage.input_tokens),
                fmt_tokens(usage.output_tokens),
                format!("${:.4}", sonnet_cost),
                format!("${:.4}", opus_cost),
            );
        }
    }

    // Total cost estimates
    let sonnet_total = pricing::estimate_cost(
        total_input,
        total_output,
        pricing::SONNET_INPUT_PER_M,
        pricing::SONNET_OUTPUT_PER_M,
    );
    let opus_total = pricing::estimate_cost(
        total_input,
        total_output,
        pricing::OPUS_INPUT_PER_M,
        pricing::OPUS_OUTPUT_PER_M,
    );

    println!("\nEstimated Cost — Sonnet ($3/1M in, $15/1M out):");
    println!("  Input:  ${:.4}  ({} tokens)", (total_input as f64 / 1_000_000.0) * pricing::SONNET_INPUT_PER_M, fmt_tokens(total_input));
    println!("  Output: ${:.4}  ({} tokens)", (total_output as f64 / 1_000_000.0) * pricing::SONNET_OUTPUT_PER_M, fmt_tokens(total_output));
    println!("  Total:  ${:.4}", sonnet_total);

    println!("\nEstimated Cost — Opus ($15/1M in, $75/1M out):");
    println!("  Input:  ${:.4}  ({} tokens)", (total_input as f64 / 1_000_000.0) * pricing::OPUS_INPUT_PER_M, fmt_tokens(total_input));
    println!("  Output: ${:.4}  ({} tokens)", (total_output as f64 / 1_000_000.0) * pricing::OPUS_OUTPUT_PER_M, fmt_tokens(total_output));
    println!("  Total:  ${:.4}", opus_total);

    Ok(())
}

/// Format a token count with comma separators.
pub fn fmt_tokens(n: i64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let s = n.abs().to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    if n < 0 {
        result.push('-');
    }
    result.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_tokens_zero() {
        assert_eq!(fmt_tokens(0), "0");
    }

    #[test]
    fn fmt_tokens_small() {
        assert_eq!(fmt_tokens(999), "999");
    }

    #[test]
    fn fmt_tokens_thousands() {
        assert_eq!(fmt_tokens(1_000), "1,000");
        assert_eq!(fmt_tokens(1_234_567), "1,234,567");
        assert_eq!(fmt_tokens(1_000_000), "1,000,000");
    }

    #[test]
    fn sonnet_cost_per_million_input() {
        let cost = pricing::estimate_cost(1_000_000, 0, pricing::SONNET_INPUT_PER_M, pricing::SONNET_OUTPUT_PER_M);
        assert!((cost - 3.0).abs() < 0.001, "1M input tokens = $3 Sonnet");
    }

    #[test]
    fn sonnet_cost_per_million_output() {
        let cost = pricing::estimate_cost(0, 1_000_000, pricing::SONNET_INPUT_PER_M, pricing::SONNET_OUTPUT_PER_M);
        assert!((cost - 15.0).abs() < 0.001, "1M output tokens = $15 Sonnet");
    }

    #[test]
    fn opus_cost_per_million_input() {
        let cost = pricing::estimate_cost(1_000_000, 0, pricing::OPUS_INPUT_PER_M, pricing::OPUS_OUTPUT_PER_M);
        assert!((cost - 15.0).abs() < 0.001, "1M input tokens = $15 Opus");
    }

    #[test]
    fn opus_cost_per_million_output() {
        let cost = pricing::estimate_cost(0, 1_000_000, pricing::OPUS_INPUT_PER_M, pricing::OPUS_OUTPUT_PER_M);
        assert!((cost - 75.0).abs() < 0.001, "1M output tokens = $75 Opus");
    }

    #[test]
    fn cost_zero_tokens() {
        let cost = pricing::estimate_cost(0, 0, pricing::SONNET_INPUT_PER_M, pricing::SONNET_OUTPUT_PER_M);
        assert_eq!(cost, 0.0);
    }
}
